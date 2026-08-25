//! Lifecycle policy tests.
//!
//! Every one of these encodes a decision from ADR 0002/0004 that would otherwise
//! only exist in prose. None of them spawn a process or open a display, which is
//! the point: CI has neither.

use glimpse::encode::OutputFormat;
use glimpse::geometry::RootPixelRect;
use glimpse::session::{
    transition, CaptureRequest, CapturedVideo, Effect, Event, State, StopReason,
};
use std::path::PathBuf;

fn request() -> CaptureRequest {
    CaptureRequest {
        rect: RootPixelRect {
            x: 10,
            y: 10,
            w: 640,
            h: 480,
        },
        framerate: 15,
        capture_mouse: true,
        destination: PathBuf::from("/home/u/out.gif"),
        format: OutputFormat::Gif,
    }
}

fn video() -> CapturedVideo {
    CapturedVideo {
        path: PathBuf::from("/tmp/glimpse-abc/recording.webm"),
        workspace: PathBuf::from("/tmp/glimpse-abc"),
    }
}

/// Drive a sequence of events, returning the final state and the last effect.
fn run(events: Vec<Event>) -> (State, Effect) {
    let mut state = State::Idle;
    let mut effect = Effect::None;
    for e in events {
        let (s, f) = transition(state, e);
        state = s;
        effect = f;
    }
    (state, effect)
}

#[test]
fn the_happy_path_reaches_completed_and_cleans_up() {
    let (state, effect) = run(vec![
        Event::Arm(request()),
        Event::Armed,
        Event::Stop,
        Event::RecorderFinished(video()),
        Event::EncoderFinished(PathBuf::from("/home/u/out.gif")),
    ]);
    assert_eq!(
        state,
        State::Completed {
            output: PathBuf::from("/home/u/out.gif")
        }
    );
    assert_eq!(
        effect,
        Effect::Cleanup {
            preserve_source: false
        }
    );
}

#[test]
fn arming_starts_the_recorder_with_the_rect_it_was_armed_with() {
    let (state, effect) = run(vec![Event::Arm(request()), Event::Armed]);
    assert_eq!(state, State::Recording { request: request() });
    assert_eq!(effect, Effect::StartRecorder(request()));
}

#[test]
fn a_failed_encode_keeps_the_recording_for_retry() {
    // The entire reason CapturedVideo exists: an encoding failure must not cost
    // the user the recording (ADR 0002).
    let (state, effect) = run(vec![
        Event::Arm(request()),
        Event::Armed,
        Event::Stop,
        Event::RecorderFinished(video()),
        Event::EncoderFailed("palettegen exited 1".into()),
    ]);
    assert_eq!(
        state,
        State::Failed {
            error: "palettegen exited 1".into(),
            retryable: Some(video())
        }
    );
    assert_eq!(
        effect,
        Effect::Cleanup {
            preserve_source: true
        }
    );
    assert_eq!(state.retryable(), Some(&video()));
}

#[test]
fn a_frame_that_moves_mid_recording_aborts_instead_of_producing_wrong_output() {
    // x11grab records a fixed root rectangle. Everything after the move is the
    // wrong region, and the file would still look plausible.
    let (state, effect) = run(vec![
        Event::Arm(request()),
        Event::Armed,
        Event::GeometryDrifted,
    ]);
    assert_eq!(
        state,
        State::Stopping {
            request: request(),
            reason: StopReason::Abort
        }
    );
    assert_eq!(
        effect,
        Effect::Terminate,
        "a drifted recording is not worth finishing cleanly"
    );
}

#[test]
fn drifting_while_arming_costs_nothing_because_no_child_exists() {
    let (state, effect) = run(vec![Event::Arm(request()), Event::GeometryDrifted]);
    assert!(matches!(
        state,
        State::Failed {
            retryable: None,
            ..
        }
    ));
    assert_eq!(
        effect,
        Effect::Unlock,
        "no process to reap, only the lock to release"
    );
}

#[test]
fn pressing_stop_twice_does_not_start_a_second_teardown() {
    let (state, effect) = run(vec![
        Event::Arm(request()),
        Event::Armed,
        Event::Stop,
        Event::Stop,
    ]);
    assert_eq!(
        state,
        State::Stopping {
            request: request(),
            reason: StopReason::Finish
        }
    );
    assert_eq!(effect, Effect::None, "the second stop must be inert");
}

#[test]
fn cancelling_a_recording_still_preserves_the_bytes() {
    // Deleting the user's only copy on their behalf is a worse default than
    // leaving a file behind.
    let (state, effect) = run(vec![
        Event::Arm(request()),
        Event::Armed,
        Event::Cancel,
        Event::RecorderFinished(video()),
    ]);
    assert_eq!(
        state,
        State::Cancelled {
            preserved: Some(video())
        }
    );
    assert_eq!(
        effect,
        Effect::Cleanup {
            preserve_source: true
        }
    );
}

#[test]
fn shutdown_terminates_a_live_child_rather_than_asking_politely() {
    // Nothing may outlive the application; there is no one left to wait.
    let (state, effect) = run(vec![Event::Arm(request()), Event::Armed, Event::Shutdown]);
    assert_eq!(state, State::Cancelled { preserved: None });
    assert_eq!(effect, Effect::Terminate);
}

#[test]
fn shutdown_mid_encode_preserves_the_source() {
    let (state, effect) = run(vec![
        Event::Arm(request()),
        Event::Armed,
        Event::Stop,
        Event::RecorderFinished(video()),
        Event::Shutdown,
    ]);
    assert_eq!(
        state,
        State::Cancelled {
            preserved: Some(video())
        }
    );
    assert_eq!(effect, Effect::Terminate);
}

#[test]
fn shutdown_when_idle_is_inert() {
    let (state, effect) = transition(State::Idle, Event::Shutdown);
    assert_eq!(state, State::Idle);
    assert_eq!(effect, Effect::None);
}

#[test]
fn a_recorder_that_exits_unasked_is_not_offered_as_retryable() {
    // Whatever it wrote was never finalised — a truncated container that looks
    // like a valid one is the failure mode being guarded against.
    let (state, _) = run(vec![
        Event::Arm(request()),
        Event::Armed,
        Event::RecorderFinished(video()),
    ]);
    assert!(matches!(
        state,
        State::Failed {
            retryable: None,
            ..
        }
    ));
}

#[test]
fn late_events_after_a_terminal_state_are_ignored_not_fatal() {
    // The worker thread and the UI thread cannot be perfectly ordered.
    let (state, effect) = run(vec![
        Event::Arm(request()),
        Event::Armed,
        Event::Stop,
        Event::RecorderFinished(video()),
        Event::EncoderFinished(PathBuf::from("/home/u/out.gif")),
        Event::RecorderFinished(video()),
        Event::Stop,
    ]);
    assert_eq!(
        state,
        State::Completed {
            output: PathBuf::from("/home/u/out.gif")
        }
    );
    assert_eq!(effect, Effect::None);
}

#[test]
fn only_active_states_are_active() {
    assert!(State::Recording { request: request() }.is_active());
    assert!(State::Encoding {
        source: video(),
        destination: PathBuf::new()
    }
    .is_active());
    assert!(!State::Idle.is_active());
    assert!(!State::Completed {
        output: PathBuf::new()
    }
    .is_active());
}

#[test]
fn the_format_is_fixed_at_arming_and_carried_through_the_session() {
    // The destination extension and the encoder must agree, so the format is
    // captured in the request rather than read live when encoding starts.
    let mp4 = CaptureRequest {
        destination: PathBuf::from("/home/u/out.mp4"),
        format: OutputFormat::Mp4,
        ..request()
    };
    let (state, effect) = run(vec![Event::Arm(mp4.clone()), Event::Armed]);
    assert_eq!(
        state,
        State::Recording {
            request: mp4.clone()
        }
    );
    assert_eq!(effect, Effect::StartRecorder(mp4));
}
