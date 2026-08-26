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
        let child = Command::new("ffmpeg")
            .args(x11grab_args(cfg, &output))
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
