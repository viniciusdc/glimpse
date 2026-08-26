//! Recording: an ffmpeg child, owned by exactly one worker.
//!
//! This is the first vertical slice through [`crate::session`]. The state machine
//! decides *what* should happen; this module does it, and reports back.
//!
//! **Every flag here is derived from ffmpeg's own documentation**
//! (`ffmpeg -h demuxer=x11grab`, <https://ffmpeg.org/ffmpeg-devices.html>), not
//! from another project's source. That is a licensing requirement, not a style
//! preference — see ADR 0003.

use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

use anyhow::{anyhow, Context, Result};

use crate::geometry::RootPixelRect;
use crate::session::CapturedVideo;

/// How long ffmpeg gets to finalise the container after being asked politely,
/// before it is killed. Finalising an ffv1/Matroska file is fast; this is
/// generous, and bounded so shutdown cannot hang forever.
const GRACEFUL_STOP_TIMEOUT: Duration = Duration::from_secs(5);

/// Container extension for the intermediate recording.
///
/// Matroska, because it carries ffv1 with `bgr0` without a pixel-format
/// conversion — verified with `ffprobe`, which reports
/// `codec_name=ffv1, pix_fmt=bgr0` for a captured file. x11grab's native output
/// *is* `bgr0`, so nothing is converted on the way in and the intermediate is
/// lossless by construction rather than by assertion (ADR 0002 explicitly refuses
/// to call it lossless on any weaker basis).
const INTERMEDIATE_EXT: &str = "mkv";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RecorderConfig {
    /// The X display, taken from the same environment the geometry came from —
    /// never independently guessed, or ffmpeg could record a different screen
    /// than the one the rectangle was computed against.
    pub display: String,
    pub rect: RootPixelRect,
    pub framerate: u32,
    pub capture_mouse: bool,
}

impl RecorderConfig {
    /// Read the display from the environment. Returns an error rather than
    /// falling back to `:0`.
    pub fn display_from_env() -> Result<String> {
        std::env::var("DISPLAY").context("DISPLAY is not set; cannot record")
    }
}

/// The ffmpeg command line, as pure data so it can be asserted on without
/// spawning anything.
///
/// Options, each from `ffmpeg -h demuxer=x11grab`:
///   `-video_size`  frame size
///   `-framerate`   capture rate
///   `-draw_mouse`  0 or 1, draw the pointer
///   `-grab_x` / `-grab_y`   region origin
///
/// The documented `-grab_x`/`-grab_y` options are used rather than encoding the
/// origin into the input URL, because they are explicit and cannot be mangled by
/// a display name that already contains punctuation.
pub fn x11grab_args(cfg: &RecorderConfig, output: &Path) -> Vec<String> {
    let s = |v: &str| v.to_string();
    vec![
        s("-hide_banner"),
        s("-loglevel"),
        s("error"),
        s("-y"),
        s("-f"),
        s("x11grab"),
        s("-framerate"),
        cfg.framerate.to_string(),
        s("-video_size"),
        cfg.rect.video_size(),
        s("-draw_mouse"),
        if cfg.capture_mouse { s("1") } else { s("0") },
        s("-grab_x"),
        cfg.rect.x.to_string(),
        s("-grab_y"),
        cfg.rect.y.to_string(),
        s("-i"),
        cfg.display.clone(),
        // ffv1 is ffmpeg's own lossless codec; bgr0 is what x11grab produces, so
        // stating it here keeps the pipeline conversion-free.
        s("-c:v"),
        s("ffv1"),
        s("-pix_fmt"),
        s("bgr0"),
        output.to_string_lossy().into_owned(),
    ]
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

/// Grab a single frame of `cfg.rect` and commit it as a PNG.
///
/// A snapshot is not a one-frame recording: there is no session, no lifecycle and
/// nothing to stop. It is one ffmpeg invocation and an atomic rename, so it lives
/// here rather than going through [`crate::session`].
///
/// It does share the two rules that matter: the file is staged in the
/// destination's own directory and renamed into place, so a reader never sees a
/// half-written image; and a taken name is disambiguated rather than overwritten
/// ([ADR 0005](../docs/adr/0005-gif-encoding-and-the-atomic-commit.md)).
pub fn snapshot(cfg: &RecorderConfig, destination: &Path) -> Result<PathBuf> {
    if !cfg.rect.is_capturable() {
        return Err(anyhow!("nothing to capture: {}x{}", cfg.rect.w, cfg.rect.h));
    }
    let dir = destination.parent().unwrap_or_else(|| Path::new("."));
    std::fs::create_dir_all(dir).with_context(|| format!("creating {}", dir.display()))?;

    let final_path = crate::encode::free_destination(destination, |p| p.exists());
    let staged = dir.join(format!(".glimpse-{}-snapshot.png.part", std::process::id()));

    let out = Command::new("ffmpeg")
        .args(snapshot_args(cfg, &staged))
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

/// One frame of x11grab, written as a PNG.
///
/// Same documented options as the recorder — `-video_size`, `-grab_x`, `-grab_y`,
/// `-draw_mouse` — with `-frames:v 1` instead of a framerate. The muxer is stated
/// because the output is staged under a `.part` suffix.
pub fn snapshot_args(cfg: &RecorderConfig, output: &Path) -> Vec<String> {
    let s = |v: &str| v.to_string();
    vec![
        s("-hide_banner"),
        s("-loglevel"),
        s("error"),
        s("-y"),
        s("-f"),
        s("x11grab"),
        s("-video_size"),
        cfg.rect.video_size(),
        s("-draw_mouse"),
        if cfg.capture_mouse { s("1") } else { s("0") },
        s("-grab_x"),
        cfg.rect.x.to_string(),
        s("-grab_y"),
        cfg.rect.y.to_string(),
        s("-i"),
        cfg.display.clone(),
        s("-frames:v"),
        s("1"),
        // The CODEC must be stated, not just the container. `image2` is a
        // container whose default encoder is mjpeg, and the staged file is named
        // `.png.part` so ffmpeg cannot infer anything from the extension — without
        // this, Glimpse writes a JPEG into a file called .png.
        s("-c:v"),
        s("png"),
        s("-f"),
        s("image2"),
        output.to_string_lossy().into_owned(),
    ]
}

/// Remove session directories left behind by Glimpse processes that are gone.
///
/// `PR_SET_PDEATHSIG` stops a hard kill from orphaning ffmpeg, but nothing can
/// remove the workspace on that path — deleting a directory needs code to run,
/// and on `SIGKILL` none does. So the tidying happens at the next startup
/// instead, which is the only moment it reliably can.
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

#[cfg(target_os = "linux")]
pub(crate) fn process_is_alive(pid: u32) -> bool {
    std::path::Path::new(&format!("/proc/{pid}")).exists()
}

#[cfg(not(target_os = "linux"))]
pub(crate) fn process_is_alive(_pid: u32) -> bool {
    // Without /proc, assume alive: leaving a directory behind is a much smaller
    // mistake than deleting a running session's recording.
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
    pub fn start(cfg: &RecorderConfig, workspace: Workspace) -> Result<Self> {
        if !cfg.rect.is_capturable() {
            return Err(anyhow!(
                "refusing to record a {}x{} region",
                cfg.rect.w,
                cfg.rect.h
            ));
        }
        let output = workspace.video_path();
        let mut command = Command::new("ffmpeg");
        command.args(x11grab_args(cfg, &output));
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
/// `Drop` covers every exit path the process controls, but not `SIGKILL` or an
/// X server disconnect — and that gap is not theoretical. Killing Glimpse during
/// development left ffmpeg processes still recording into directories that had
/// already been deleted.
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
    use super::stale_workspace_pid;

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
}
