//! The recording lifecycle.
//!
//! This lands **before** capture, per
//! [ADR 0004](../docs/adr/0004-review-corrections-and-the-lifecycle-spine.md).
//! Stopping, cancellation, shutdown and child reaping decide how the ffmpeg
//! command is owned; writing capture first means writing it twice.
//!
//! The machine is pure. It holds no process handles, no file descriptors and no
//! clock — it maps `(State, Event)` to a new state plus an [`Effect`] describing
//! what the caller must *do*. The worker that owns the ffmpeg child performs
//! effects and feeds results back as events. That split is what makes every
//! policy below testable without spawning anything, which matters because CI has
//! no display and no X server.
//!
//! ```text
//! Idle → Arming → Recording → Stopping → Encoding → Completed | Failed | Cancelled
//! ```

use std::path::PathBuf;

use crate::encode::OutputFormat;
use crate::geometry::RootPixelRect;

/// What to record, fixed at arming time and never re-read afterwards.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CaptureRequest {
    pub rect: RootPixelRect,
    pub framerate: u32,
    pub capture_mouse: bool,
    pub destination: PathBuf,
    pub format: OutputFormat,
}

/// A finished recording that has not been encoded yet.
///
/// This is the artifact that must survive an encoding failure so the user can
/// retry without re-recording (ADR 0002). It owns its workspace, so whoever holds
/// it decides when the bytes go away.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CapturedVideo {
    pub path: PathBuf,
    pub workspace: PathBuf,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StopReason {
    /// The user asked for the recording to end normally; the output is wanted.
    Finish,
    /// The user abandoned the recording; the output is not wanted.
    Cancel,
    /// An invariant broke — the frame moved, or the app is going down.
    Abort,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum State {
    Idle,
    /// Geometry is being settled. No child exists yet, so failure here is cheap.
    Arming {
        request: CaptureRequest,
    },
    Recording {
        request: CaptureRequest,
    },
    /// ffmpeg has been asked to stop but has not exited.
    ///
    /// Not decoration: ffmpeg can have received `q` without having finalised the
    /// container, and a video read during this window is truncated while looking
    /// entirely plausible.
    Stopping {
        request: CaptureRequest,
        reason: StopReason,
    },
    Encoding {
        source: CapturedVideo,
        destination: PathBuf,
    },
    Completed {
        output: PathBuf,
    },
    /// Carries the source when one survived, so a retry does not mean re-recording.
    Failed {
        error: String,
        retryable: Option<CapturedVideo>,
    },
    Cancelled {
        preserved: Option<CapturedVideo>,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Event {
    Arm(CaptureRequest),
    /// Geometry settled and the recorder may start.
    Armed,
    /// The checked invariant fired: the frame moved after being locked.
    GeometryDrifted,
    Stop,
    Cancel,
    /// The child exited and the container is finalised.
    RecorderFinished(CapturedVideo),
    RecorderFailed(String),
    EncoderFinished(PathBuf),
    EncoderFailed(String),
    /// Encode the preserved capture again, without re-recording. Carries the
    /// destination because the machine holds no settings — the caller knows
    /// where output goes now, which may differ from where it would have gone.
    Retry {
        destination: PathBuf,
    },
    /// The application is going down. Nothing may outlive this.
    Shutdown,
}

/// What the caller must do. The machine never does these itself.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Effect {
    None,
    /// Create the workspace and spawn ffmpeg against `request.rect`.
    StartRecorder(CaptureRequest),
    /// Ask ffmpeg to finish cleanly — write `q` to its stdin.
    GracefulStop,
    /// Graceful stop is not appropriate or did not land: terminate, then wait.
    /// Escalation is bounded by the worker, never unbounded.
    Terminate,
    StartEncoder {
        source: CapturedVideo,
        destination: PathBuf,
    },
    /// Release the workspace. `preserve_source` keeps the recording for a retry.
    Cleanup {
        preserve_source: bool,
    },
    /// Release the frame's geometry lock.
    Unlock,
}

impl State {
    pub fn is_active(&self) -> bool {
        matches!(
            self,
            State::Arming { .. }
                | State::Recording { .. }
                | State::Stopping { .. }
                | State::Encoding { .. }
        )
    }

    /// The recording that could still be salvaged from this state, if any.
    pub fn retryable(&self) -> Option<&CapturedVideo> {
        match self {
            State::Failed {
                retryable: Some(v), ..
            }
            | State::Cancelled { preserved: Some(v) } => Some(v),
            State::Encoding { source, .. } => Some(source),
            _ => None,
        }
    }
}

/// Advance the machine.
///
/// Unknown `(state, event)` pairs are ignored rather than panicking: events
/// arrive from a subprocess worker and a UI thread that cannot be perfectly
/// ordered, and a late `RecorderFinished` after a `Shutdown` must not crash the
/// application.
pub fn transition(state: State, event: Event) -> (State, Effect) {
    use Event as E;
    use State as S;

    match (state, event) {
        // ---- arming -------------------------------------------------------
        (S::Idle, E::Arm(request)) => (S::Arming { request }, Effect::None),
        (S::Arming { request }, E::Armed) => (
            S::Recording {
                request: request.clone(),
            },
            Effect::StartRecorder(request),
        ),
        // No child exists yet, so this costs nothing but the lock.
        (S::Arming { .. }, E::GeometryDrifted) => (
            S::Failed {
                error: "the frame moved while arming".into(),
                retryable: None,
            },
            Effect::Unlock,
        ),
        (S::Arming { .. }, E::Cancel) => (S::Cancelled { preserved: None }, Effect::Unlock),

        // ---- recording ----------------------------------------------------
        (S::Recording { request }, E::Stop) => (
            S::Stopping {
                request,
                reason: StopReason::Finish,
            },
            Effect::GracefulStop,
        ),
        (S::Recording { request }, E::Cancel) => (
            S::Stopping {
                request,
                reason: StopReason::Cancel,
            },
            Effect::GracefulStop,
        ),
        // The frame moved under a running recorder. Everything captured after the
        // move is of the wrong region, so the recording is not trustworthy.
        (S::Recording { request }, E::GeometryDrifted) => (
            S::Stopping {
                request,
                reason: StopReason::Abort,
            },
            Effect::Terminate,
        ),
        (S::Recording { .. }, E::RecorderFailed(error)) => (
            S::Failed {
                error,
                retryable: None,
            },
            Effect::Cleanup {
                preserve_source: false,
            },
        ),
        // ffmpeg exited on its own — disk full, display vanished. Whatever it
        // wrote was never finalised, so it is not offered as retryable.
        (S::Recording { .. }, E::RecorderFinished(_)) => (
            S::Failed {
                error: "the recorder exited without being asked to".into(),
                retryable: None,
            },
            Effect::Cleanup {
                preserve_source: false,
            },
        ),

        // ---- stopping -----------------------------------------------------
        (S::Stopping { request, reason }, E::RecorderFinished(video)) => match reason {
            StopReason::Finish => (
                S::Encoding {
                    source: video.clone(),
                    destination: request.destination.clone(),
                },
                Effect::StartEncoder {
                    source: video,
                    destination: request.destination,
                },
            ),
            // Cancelled and aborted recordings are still preserved: the bytes
            // exist, and deleting a user's only copy on their behalf is a worse
            // default than leaving a file behind.
            StopReason::Cancel | StopReason::Abort => (
                S::Cancelled {
                    preserved: Some(video),
                },
                Effect::Cleanup {
                    preserve_source: true,
                },
            ),
        },
        (S::Stopping { .. }, E::RecorderFailed(error)) => (
            S::Failed {
                error,
                retryable: None,
            },
            Effect::Cleanup {
                preserve_source: false,
            },
        ),
        // Pressing stop twice must not start a second teardown.
        (S::Stopping { request, reason }, E::Stop | E::Cancel) => {
            (S::Stopping { request, reason }, Effect::None)
        }

        // ---- encoding -----------------------------------------------------
        (S::Encoding { .. }, E::EncoderFinished(output)) => (
            S::Completed { output },
            Effect::Cleanup {
                preserve_source: false,
            },
        ),
        // The whole point of CapturedVideo: a failed encode must not cost the
        // recording.
        (S::Encoding { source, .. }, E::EncoderFailed(error)) => (
            S::Failed {
                error,
                retryable: Some(source),
            },
            Effect::Cleanup {
                preserve_source: true,
            },
        ),
        (S::Encoding { source, .. }, E::Cancel) => (
            S::Cancelled {
                preserved: Some(source),
            },
            Effect::Cleanup {
                preserve_source: true,
            },
        ),

        // ---- retry --------------------------------------------------------
        // The whole point of preserving the capture: a failed or cancelled
        // encode can be re-run without asking the user to record again.
        (
            S::Failed {
                retryable: Some(video),
                ..
            },
            E::Retry { destination },
        )
        | (
            S::Cancelled {
                preserved: Some(video),
            },
            E::Retry { destination },
        ) => (
            S::Encoding {
                source: video.clone(),
                destination: destination.clone(),
            },
            Effect::StartEncoder {
                source: video,
                destination,
            },
        ),

        // ---- shutdown -----------------------------------------------------
        // Nothing may outlive the application. A live child is terminated rather
        // than asked politely, because there is no one left to wait for it.
        (s, E::Shutdown) if s.is_active() => {
            let preserved = s.retryable().cloned();
            let effect = match s {
                S::Recording { .. } | S::Stopping { .. } | S::Encoding { .. } => Effect::Terminate,
                _ => Effect::Unlock,
            };
            (S::Cancelled { preserved }, effect)
        }
        (s, E::Shutdown) => (s, Effect::None),

        // ---- anything else is a late or duplicate event --------------------
        (s, _) => (s, Effect::None),
    }
}
