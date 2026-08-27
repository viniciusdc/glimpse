//! Recording: an ffmpeg child, owned by exactly one worker.
//!
//! This is the first vertical slice through [`crate::session`]. The state machine
//! decides *what* should happen; this module does it, and reports back.
//!
//! ## What this module deliberately does not know
//!
//! It cannot name a capture backend. Which pixels get grabbed, and how the region
//! is expressed to ffmpeg, is the frontend's business and arrives as a
//! [`GrabCommand`] — see ADR 0010. That split is not tidiness: `x11grab` takes the
//! region on the input as `-grab_x`/`-grab_y`, while `avfoundation` has no such
//! option and must crop with a filter instead. Code that hard-codes either shape
//! excludes the other platform.
//!
//! **Every flag this module contributes is derived from ffmpeg's own
//! documentation** (<https://ffmpeg.org/ffmpeg.html>), not from another project's
//! source. That is a licensing requirement, not a style preference — see ADR 0003.
//! The same obligation applies to whoever builds the [`GrabCommand`].

use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

use anyhow::{anyhow, Context, Result};

use crate::geometry::ScreenPixelRect;
use crate::session::CapturedVideo;

/// How long ffmpeg gets to finalise the container after being asked politely,
/// before it is killed. Finalising an ffv1/Matroska file is fast; this is
/// generous, and bounded so shutdown cannot hang forever.
const GRACEFUL_STOP_TIMEOUT: Duration = Duration::from_secs(5);

/// Container extension for the intermediate recording.
///
/// Matroska, because it carries ffv1 without a pixel-format conversion —
/// verified with `ffprobe`, which reports `codec_name=ffv1, pix_fmt=bgr0` for a
/// file captured from x11grab. x11grab's native output *is* `bgr0`, so nothing is
/// converted on the way in and the intermediate is lossless by construction
/// rather than by assertion (ADR 0002 explicitly refuses to call it lossless on
/// any weaker basis).
///
/// That guarantee is per-provider, which is why the pixel format travels on
/// [`GrabCommand::pix_fmt`] rather than being fixed here.
const INTERMEDIATE_EXT: &str = "mkv";

/// What the user asked to capture, in terms no backend is implied by.
///
/// A frontend turns one of these into a [`GrabCommand`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GrabRequest {
    pub rect: ScreenPixelRect,
    /// Capture rate. `None` asks for a single frame — a snapshot sets no rate,
    /// and the flag that would express one differs between backends anyway.
    pub framerate: Option<u32>,
    pub capture_mouse: bool,
}

/// A backend's answer to "how do I grab that rectangle?", as pure data.
///
/// Kept as data rather than as a spawned process so it can be asserted on
/// without touching a screen — which is how the X11 argument construction is
/// tested today, and the property worth keeping when a second backend lands.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GrabCommand {
    /// The region this command was built to grab.
    ///
    /// Carried alongside the arguments rather than left implicit in them: core
    /// has to refuse a zero-area capture, and recovering the rect by parsing
    /// `input` back would mean core knowing each backend's flag spelling — which
    /// is the coupling this struct exists to remove.
    pub rect: ScreenPixelRect,
    /// ffmpeg input arguments, ending with the input specifier itself.
    pub input: Vec<String>,
    /// A video filter, for backends that cannot express the region on the input.
    ///
    /// `None` on X11, which crops by grabbing only the region it wants. `Some`
    /// on `avfoundation`, which captures a whole display and must crop after the
    /// fact. This field is the entire reason [`GrabCommand`] is a struct and not
    /// a `Vec<String>`.
    pub filter: Option<String>,
    /// The pixel format the source natively produces, when stating it avoids a
    /// conversion. `None` leaves the choice to ffmpeg.
    pub pix_fmt: Option<String>,
}

impl GrabCommand {
    /// The full argument list for a recording, into the lossless intermediate.
    ///
    /// Options contributed here, each from ffmpeg's own documentation:
    ///   `-c:v ffv1`    ffmpeg's own lossless codec
    ///   `-pix_fmt`     stated only when the backend named its native format
    pub fn recording_args(&self, output: &Path) -> Vec<String> {
        let mut args = vec![
            "-hide_banner".into(),
            "-loglevel".into(),
            "error".into(),
            "-y".into(),
        ];
        args.extend(self.input.iter().cloned());
        if let Some(filter) = &self.filter {
            args.push("-vf".into());
            args.push(filter.clone());
        }
        args.push("-c:v".into());
        args.push("ffv1".into());
        if let Some(pix_fmt) = &self.pix_fmt {
            args.push("-pix_fmt".into());
            args.push(pix_fmt.clone());
        }
        args.push(output.to_string_lossy().into_owned());
        args
    }

    /// The full argument list for a single frame, written as a PNG.
    ///
    /// The CODEC must be stated, not just the container. `image2` is a container
    /// whose default encoder is mjpeg, and the staged file is named `.png.part`
    /// so ffmpeg cannot infer anything from the extension — without `-c:v png`,
    /// Glimpse writes a JPEG into a file called `.png`. It did exactly that once
    /// (ADR 0009).
    pub fn snapshot_args(&self, output: &Path) -> Vec<String> {
        let mut args = vec![
            "-hide_banner".into(),
            "-loglevel".into(),
            "error".into(),
            "-y".into(),
        ];
        args.extend(self.input.iter().cloned());
        if let Some(filter) = &self.filter {
            args.push("-vf".into());
            args.push(filter.clone());
        }
        args.extend([
            "-frames:v".into(),
            "1".into(),
            "-c:v".into(),
            "png".into(),
            "-f".into(),
            "image2".into(),
        ]);
        args.push(output.to_string_lossy().into_owned());
        args
    }
}

/// A per-session directory holding the intermediate recording.
///
/// Owns its bytes: [`Workspace::dispose`] is the only thing that deletes them,
/// and it refuses to when asked to preserve — a failed encode must not cost the
/// user the recording (ADR 0002).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Workspace {
    root: PathBuf,
}

impl Workspace {
    /// Create a fresh session directory. The name carries the process id and a
    /// monotonic counter, so two sessions in one process cannot collide.
    pub fn create() -> Result<Self> {
        use std::sync::atomic::{AtomicU32, Ordering};
        static SEQ: AtomicU32 = AtomicU32::new(0);
        let n = SEQ.fetch_add(1, Ordering::Relaxed);
        let root = std::env::temp_dir().join(format!("glimpse-{}-{n}", std::process::id()));
        std::fs::create_dir_all(&root)
            .with_context(|| format!("creating session workspace {}", root.display()))?;
        Ok(Self { root })
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn video_path(&self) -> PathBuf {
        self.root.join(format!("recording.{INTERMEDIATE_EXT}"))
    }

    /// Release the workspace. With `preserve` the directory is left alone and its
    /// path returned, so the caller can tell the user where their recording is.
    ///
    /// Takes `&self` rather than consuming, because [`Recorder`] implements `Drop`
    /// as its reaping backstop and Rust will not let a `Drop` type be moved out of
    /// piecemeal.
    pub fn dispose(&self, preserve: bool) -> Option<PathBuf> {
        if preserve {
            return Some(self.root.clone());
        }
        if let Err(e) = std::fs::remove_dir_all(&self.root) {
            eprintln!("glimpse: could not remove {}: {e}", self.root.display());
        }
        None
    }
}

/// Grab a single frame and commit it as a PNG.
///
/// A snapshot is not a one-frame recording: there is no session, no lifecycle and
/// nothing to stop. It is one ffmpeg invocation and an atomic rename, so it lives
/// here rather than going through [`crate::session`].
///
/// It does share the two rules that matter: the file is staged in the
/// destination's own directory and renamed into place, so a reader never sees a
/// half-written image; and a taken name is disambiguated rather than overwritten
/// (ADR 0005).
pub fn snapshot(grab: &GrabCommand, destination: &Path) -> Result<PathBuf> {
    if !grab.rect.is_capturable() {
        return Err(anyhow!(
            "nothing to capture: {}x{}",
            grab.rect.w,
            grab.rect.h
        ));
    }
    let dir = destination.parent().unwrap_or_else(|| Path::new("."));
    std::fs::create_dir_all(dir).with_context(|| format!("creating {}", dir.display()))?;

    let final_path = crate::encode::free_destination(destination, |p| p.exists());
    let staged = dir.join(format!(".glimpse-{}-snapshot.png.part", std::process::id()));

    let out = Command::new("ffmpeg")
        .args(grab.snapshot_args(&staged))
        .output()
        .context("spawning ffmpeg — is it installed?")?;

    if !out.status.success() {
        let _ = std::fs::remove_file(&staged);
        let detail = String::from_utf8_lossy(&out.stderr);
        return Err(anyhow!(
            "ffmpeg exited with {}: {}",
            out.status,
            detail.lines().last().unwrap_or("no stderr")
        ));
    }

    std::fs::rename(&staged, &final_path)
        .with_context(|| format!("committing {}", final_path.display()))?;
    Ok(final_path)
}

/// Remove session directories left behind by Glimpse processes that are gone.
///
/// `PR_SET_PDEATHSIG` stops a hard kill from orphaning ffmpeg on Linux, but
/// nothing can remove the workspace on that path — deleting a directory needs
/// code to run, and on `SIGKILL` none does. So the tidying happens at the next
/// startup instead, which is the only moment it reliably can.
///
/// Only directories matching Glimpse's own naming are considered, and only when
/// their process id is no longer alive — otherwise a second Glimpse recording
/// right now would have its workspace deleted out from under it.
///
/// Returns how many were removed. Failure is reported and otherwise ignored: not
/// tidying up is never a reason to refuse to start.
pub fn sweep_stale_workspaces() -> usize {
    let me = std::process::id();
    let Ok(entries) = std::fs::read_dir(std::env::temp_dir()) else {
        return 0;
    };

    let mut removed = 0;
    for entry in entries.flatten() {
        let name = entry.file_name();
        let Some(name) = name.to_str() else { continue };
        let Some(pid) = stale_workspace_pid(name, me) else {
            continue;
        };
        if process_is_alive(pid) {
            continue;
        }
        match std::fs::remove_dir_all(entry.path()) {
            Ok(()) => removed += 1,
            Err(e) => eprintln!("glimpse: could not remove {}: {e}", entry.path().display()),
        }
    }
    removed
}

/// The pid encoded in a `glimpse-<pid>-<n>` directory name, if it is one and it
/// is not ours.
pub(crate) fn stale_workspace_pid(name: &str, own_pid: u32) -> Option<u32> {
    let rest = name.strip_prefix("glimpse-")?;
    let (pid, seq) = rest.split_once('-')?;
    // Both halves must be numeric, so unrelated `glimpse-*` directories — the
    // encode test's scratch dirs, someone's notes — are left alone.
    let pid: u32 = pid.parse().ok()?;
    seq.parse::<u32>().ok()?;
    (pid != own_pid).then_some(pid)
}

/// Is this process still running?
///
/// `kill(pid, 0)` performs the permission and existence checks and delivers no
/// signal, which POSIX documents precisely for this purpose. It is used rather
/// than `/proc` because `/proc` is Linux-only, and the `cfg(not(linux))` fallback
/// this replaced answered "alive" unconditionally — which meant
/// [`sweep_stale_workspaces`] found every candidate alive and removed **nothing**
/// on macOS, leaking a temp directory per killed session, permanently and
/// silently.
///
/// `EPERM` counts as alive: the process exists, it is simply not ours to signal.
#[cfg(unix)]
pub(crate) fn process_is_alive(pid: u32) -> bool {
    // `kill` reserves low pids for broadcast: 0 means "every process in my own
    // process group", so `kill(0, 0)` succeeds and would report a `glimpse-0-*`
    // directory alive forever. No Glimpse instance ever has pid 0, so answer
    // directly rather than asking. (`-1`, the other broadcast value, cannot
    // arrive here — the pid is parsed as `u32`.)
    if pid == 0 {
        return false;
    }
    // SAFETY: `kill` with signal 0 sends nothing and only inspects process state.
    let rc = unsafe { libc::kill(pid as libc::pid_t, 0) };
    if rc == 0 {
        return true;
    }
    std::io::Error::last_os_error().raw_os_error() == Some(libc::EPERM)
}

#[cfg(not(unix))]
pub(crate) fn process_is_alive(_pid: u32) -> bool {
    // Windows has no `kill(pid, 0)`; until a frontend exists there, assume alive.
    // Leaving a directory behind is a much smaller mistake than deleting a
    // running session's recording.
    true
}

/// A running ffmpeg child. Exactly one of these owns the process, and every exit
/// path from this type waits on it.
pub struct Recorder {
    child: Child,
    workspace: Workspace,
    output: PathBuf,
}

impl Recorder {
    pub fn start(grab: &GrabCommand, workspace: Workspace) -> Result<Self> {
        if !grab.rect.is_capturable() {
            return Err(anyhow!(
                "refusing to record a {}x{} region",
                grab.rect.w,
                grab.rect.h
            ));
        }
        let output = workspace.video_path();
        let mut command = Command::new("ffmpeg");
        command.args(grab.recording_args(&output));
        die_with_parent(&mut command);
        let child = command
            // stdin stays open: writing `q` to it is the documented way to ask
            // ffmpeg to stop and finalise the container.
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::piped())
            .spawn()
            .context("spawning ffmpeg — is it installed?")?;

        Ok(Self {
            child,
            workspace,
            output,
        })
    }

    /// Ask ffmpeg to finish and finalise the file, then wait — escalating to a
    /// kill if it does not exit within [`GRACEFUL_STOP_TIMEOUT`].
    ///
    /// Escalation is bounded on purpose. An unbounded wait here would hang
    /// application shutdown behind a wedged subprocess.
    pub fn stop(mut self) -> Result<CapturedVideo> {
        if let Some(mut stdin) = self.child.stdin.take() {
            // `q` is ffmpeg's documented interactive quit; unlike a kill it lets
            // the muxer write its index, which is the difference between a valid
            // file and a truncated one that still opens.
            let _ = stdin.write_all(b"q");
            let _ = stdin.flush();
            // Dropping stdin closes it, which ffmpeg also treats as end of input.
        }

        let deadline = Instant::now() + GRACEFUL_STOP_TIMEOUT;
        loop {
            match self.child.try_wait()? {
                Some(status) if status.success() => return self.into_captured(),
                Some(status) => {
                    let err = self.drain_stderr();
                    self.workspace.dispose(false);
                    return Err(anyhow!("ffmpeg exited with {status}: {err}"));
                }
                None if Instant::now() >= deadline => {
                    eprintln!(
                        "glimpse: ffmpeg did not stop within {GRACEFUL_STOP_TIMEOUT:?}; killing"
                    );
                    return self.terminate().and_then(|_| {
                        Err(anyhow!(
                            "ffmpeg had to be killed; the recording may be truncated"
                        ))
                    });
                }
                None => std::thread::sleep(Duration::from_millis(25)),
            }
        }
    }

    /// Kill the child and wait for it. Used for abort and for shutdown, where
    /// there is no one left to wait politely.
    ///
    /// The file is *not* finalised by this path, so whatever is on disk is not
    /// offered as a valid recording.
    pub fn terminate(mut self) -> Result<()> {
        let _ = self.child.kill();
        // Always reap. A killed child that is never waited on becomes a zombie.
        self.child.wait().context("waiting for killed ffmpeg")?;
        self.workspace.dispose(false);
        Ok(())
    }

    fn into_captured(self) -> Result<CapturedVideo> {
        if !self.output.exists() {
            return Err(anyhow!("ffmpeg reported success but wrote no file"));
        }
        Ok(CapturedVideo {
            path: self.output.clone(),
            workspace: self.workspace.root.clone(),
        })
    }

    fn drain_stderr(&mut self) -> String {
        use std::io::Read;
        let mut buf = String::new();
        if let Some(mut e) = self.child.stderr.take() {
            let _ = e.read_to_string(&mut buf);
        }
        buf.lines().last().unwrap_or("no stderr").to_string()
    }
}

/// Ask the kernel to kill ffmpeg if Glimpse dies without cleaning up.
///
/// `Drop` covers every exit path the process controls, but not `SIGKILL` or a
/// display server disconnect — and that gap is not theoretical. Killing Glimpse
/// during development left ffmpeg processes still recording into directories that
/// had already been deleted.
///
/// Gated on **Linux specifically, not `unix`**. `PR_SET_PDEATHSIG` is a Linux
/// interface; macOS is also `unix` and does not have it, so `cfg(unix)` would
/// compile there and fail to link.
///
/// The signal fires when the *thread* that called `prctl` exits, which is why
/// this is set in `pre_exec` on the spawning thread: that thread owns the
/// recorder for the whole recording, so its death really does mean nobody is
/// left to reap the child.
#[cfg(target_os = "linux")]
fn die_with_parent(command: &mut Command) {
    use std::os::unix::process::CommandExt;
    // SAFETY: prctl with PR_SET_PDEATHSIG is async-signal-safe and touches only
    // this process's own kernel state, which is the contract pre_exec requires.
    unsafe {
        command.pre_exec(|| {
            libc::prctl(libc::PR_SET_PDEATHSIG, libc::SIGKILL);
            Ok(())
        });
    }
}

/// No equivalent exists off Linux, and this is a real gap rather than a stub.
///
/// macOS has no `PR_SET_PDEATHSIG`. `SIGKILL`ing Glimpse there orphans a
/// recording ffmpeg, which keeps writing into a workspace nobody will collect
/// until the next start-up sweep — which is precisely why
/// [`process_is_alive`] had to stop answering "alive" unconditionally.
#[cfg(not(target_os = "linux"))]
fn die_with_parent(_command: &mut Command) {}

impl Drop for Recorder {
    /// The backstop for "reap on every exit path". If a `Recorder` is dropped
    /// without `stop` or `terminate` — a panic, an early return, application
    /// teardown — the child is still killed and waited on.
    fn drop(&mut self) {
        if let Ok(None) = self.child.try_wait() {
            let _ = self.child.kill();
            let _ = self.child.wait();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const RECT: ScreenPixelRect = ScreenPixelRect {
        x: 0,
        y: 0,
        w: 640,
        h: 480,
    };

    #[test]
    fn recognises_our_own_workspace_naming() {
        assert_eq!(stale_workspace_pid("glimpse-1234-0", 999), Some(1234));
        assert_eq!(stale_workspace_pid("glimpse-1234-7", 999), Some(1234));
    }

    #[test]
    fn never_touches_the_running_process_workspace() {
        assert_eq!(stale_workspace_pid("glimpse-999-0", 999), None);
    }

    #[test]
    fn leaves_unrelated_directories_alone() {
        // Both halves must be numeric, so these are not ours to delete.
        for name in [
            "glimpse-enc-test-4321",
            "glimpse-demo-abcd",
            "glimpse",
            "glimpse-",
            "glimpse-notes-2026",
            "not-glimpse-1234-0",
            "glimpse-12x4-0",
        ] {
            assert_eq!(stale_workspace_pid(name, 999), None, "{name}");
        }
    }

    /// The regression that made the sweep a no-op off Linux: the old
    /// `cfg(not(linux))` stub answered "alive" for everything, so
    /// [`sweep_stale_workspaces`] skipped every candidate and removed nothing.
    ///
    /// A reaped child is the only pid a test can call dead without racing
    /// something else on the machine.
    #[cfg(unix)]
    #[test]
    fn liveness_distinguishes_a_live_pid_from_a_dead_one() {
        assert!(
            process_is_alive(std::process::id()),
            "this process is running"
        );

        let mut child = Command::new("true").spawn().expect("spawn /usr/bin/true");
        let pid = child.id();
        child.wait().expect("reap it");
        assert!(
            !process_is_alive(pid),
            "a reaped child must read as dead, or the sweep never collects anything"
        );
    }

    /// `kill` treats 0 as "my whole process group", so asking about it succeeds
    /// and a `glimpse-0-*` directory would never be swept.
    #[cfg(unix)]
    #[test]
    fn pid_zero_is_not_mistaken_for_a_live_process() {
        assert!(!process_is_alive(0));
    }

    #[test]
    fn a_filter_is_only_emitted_when_the_backend_asked_for_one() {
        let base = GrabCommand {
            rect: RECT,
            input: vec!["-i".into(), "src".into()],
            filter: None,
            pix_fmt: None,
        };
        assert!(!base
            .recording_args(Path::new("o.mkv"))
            .contains(&"-vf".to_string()));

        let cropped = GrabCommand {
            filter: Some("crop=10:20:30:40".into()),
            ..base
        };
        let args = cropped.recording_args(Path::new("o.mkv"));
        let vf = args
            .iter()
            .position(|a| a == "-vf")
            .expect("filter emitted");
        assert_eq!(args[vf + 1], "crop=10:20:30:40");
    }

    /// `image2`'s default encoder is mjpeg and the staged name is `.png.part`,
    /// so the codec has to be stated or a JPEG lands in a file called `.png`.
    #[test]
    fn a_snapshot_always_states_the_png_codec() {
        let grab = GrabCommand {
            rect: RECT,
            input: vec!["-i".into(), "src".into()],
            filter: None,
            pix_fmt: None,
        };
        let args = grab.snapshot_args(Path::new("out.png.part"));
        let c = args.iter().position(|a| a == "-c:v").expect("codec stated");
        assert_eq!(args[c + 1], "png");
    }
}
