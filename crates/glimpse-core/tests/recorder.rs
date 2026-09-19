//! The recorder's ownership of a real ffmpeg child, on every platform.
//!
//! **WHY THIS EXISTS.** `Recorder` is the one part of Glimpse that owns an
//! operating-system process, and until now nothing in the suite started one.
//! `tests/capture.rs` checks the arguments a `GrabCommand` turns into and never
//! spawns anything; the user journeys spawn plenty, but they need a screen.
//! These need only ffmpeg, so they run anywhere `cargo test` does and pin the
//! start/stop/reap lifecycle down below the level of any UI.
//!
//! (This header once said a macOS CI runner has no screen, which was the reason
//! given for writing these. It has one — the probe that said otherwise was
//! reading the wrong device index, #56 — but the tests stand on their own: a
//! lifecycle bug is quicker to find here than through a journey.)
//!
//! **WHY A SYNTHETIC SOURCE IS NOT CHEATING HERE.** `GrabCommand` is plain data,
//! and [ADR 0010](../../docs/adr/0010-capture-providers-and-a-platform-free-core.md)
//! describes it as exactly that: "the platform hands over something describing
//! what to do, and the shared code does it". These tests are of the shared code.
//! Handing it a `lavfi` source rather than a screen is using the seam the way it
//! was designed, and `tests/encode.rs` already does the same thing for the same
//! reason — ffmpeg can synthesise its own input, so this needs no display.
//!
//! What this deliberately does **not** test is whether each backend builds the
//! right arguments for its own platform. That belongs to `tests/capture.rs`,
//! `glimpse-x11/tests/grab.rs` and `glimpse-macos`'s own tests, and conflating
//! the two is how a check ends up vouching for its own copy of the arguments —
//! the failure `grab_through_the_shipping_path` records.
//!
//! **WHAT WOULD HAPPEN IF THE THING THIS TESTS WERE BROKEN.** A recorder that
//! does not reap leaves an ffmpeg holding the capture device, which breaks the
//! *next* recording and reads as a bug somewhere else entirely. That is issue
//! #45, and it reached a user.

use std::time::Duration;

use glimpse_core::capture::{GrabCommand, Recorder, Workspace};
use glimpse_core::geometry::ScreenPixelRect;

fn ffmpeg_available() -> bool {
    std::process::Command::new("ffmpeg")
        .arg("-version")
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

/// A command that records something real without needing a screen.
///
/// `testsrc` is ffmpeg's own generator. The rect is carried because core refuses
/// a zero-area capture from it, and it has to agree with the frame size or the
/// refusal would be about the wrong thing.
fn synthetic(rect: ScreenPixelRect) -> GrabCommand {
    GrabCommand {
        rect,
        input: vec![
            "-f".into(),
            "lavfi".into(),
            "-i".into(),
            format!("testsrc=size={}x{}:rate=10", rect.w, rect.h),
        ],
        filter: None,
        pix_fmt: None,
    }
}

/// Is a process still alive? Used to prove reaping rather than assume it.
#[cfg(unix)]
fn alive(pid: u32) -> bool {
    // SAFETY: signal 0 sends nothing and only inspects whether the process
    // exists. Note a zombie answers "alive" here, which is the point: a child
    // that was killed but never waited on is exactly the leak being checked for.
    unsafe { libc::kill(pid as libc::pid_t, 0) == 0 }
}

fn rect() -> ScreenPixelRect {
    ScreenPixelRect {
        x: 0,
        y: 0,
        w: 320,
        h: 240,
    }
}

#[test]
fn a_recording_stops_cleanly_and_leaves_a_decodable_file() {
    if !ffmpeg_available() {
        eprintln!("skipping: ffmpeg not installed");
        return;
    }
    let workspace = Workspace::create().expect("workspace");
    let recorder = Recorder::start(&synthetic(rect()), workspace).expect("start");

    // Long enough for ffmpeg to write frames. The point is a file with content,
    // not a file that merely exists — a truncated container still opens.
    std::thread::sleep(Duration::from_millis(1200));
    let video = recorder.stop().expect("stop should finalise the recording");

    assert!(video.path.exists(), "no file at {}", video.path.display());
    let bytes = std::fs::metadata(&video.path).expect("metadata").len();
    assert!(bytes > 0, "the recording is empty");

    // Decodable, not merely present. `stop` writes `q` so the muxer finishes its
    // index, and the difference between that and a kill is a file that opens but
    // has no frames in it.
    let probe = std::process::Command::new("ffprobe")
        .args([
            "-v",
            "error",
            "-count_frames",
            "-select_streams",
            "v:0",
            "-show_entries",
            "stream=nb_read_frames",
            "-of",
            "csv=p=0",
        ])
        .arg(&video.path)
        .output()
        .expect("ffprobe");
    let frames: u32 = String::from_utf8_lossy(&probe.stdout)
        .trim()
        .parse()
        .unwrap_or(0);
    assert!(frames > 0, "the recording decoded no frames");

    std::fs::remove_dir_all(&video.workspace).ok();
}

#[test]
fn terminating_reaps_the_child_and_removes_the_workspace() {
    if !ffmpeg_available() {
        eprintln!("skipping: ffmpeg not installed");
        return;
    }
    let workspace = Workspace::create().expect("workspace");
    let root = workspace.root().to_path_buf();
    let recorder = Recorder::start(&synthetic(rect()), workspace).expect("start");
    std::thread::sleep(Duration::from_millis(300));

    recorder.terminate().expect("terminate");

    assert!(
        !root.exists(),
        "terminate left the workspace behind at {}",
        root.display()
    );
}

/// The `Drop` backstop, which is the one that matters for issue #45.
///
/// `stop` and `terminate` are the paths a caller takes deliberately. This is the
/// path taken by a panic, an early return, or an application shutting down — and
/// on macOS there is no `PR_SET_PDEATHSIG` to catch what it misses, so a child
/// that survives here survives forever, holding the screen capture device and
/// breaking the next recording.
#[cfg(unix)]
#[test]
fn dropping_a_recorder_kills_and_reaps_its_child() {
    if !ffmpeg_available() {
        eprintln!("skipping: ffmpeg not installed");
        return;
    }
    let workspace = Workspace::create().expect("workspace");
    let root = workspace.root().to_path_buf();
    let recorder = Recorder::start(&synthetic(rect()), workspace).expect("start");
    std::thread::sleep(Duration::from_millis(300));

    // Read the pid out of the process table rather than from the Recorder, which
    // does not expose it — and asserting on something the type hands you would
    // be asserting on bookkeeping rather than on the operating system.
    let pid = child_pid(&root).expect("ffmpeg should be running for this workspace");
    assert!(alive(pid), "the control is wrong: ffmpeg was not running");

    drop(recorder);

    // A zombie answers `kill(pid, 0)`, so this fails for a child that was killed
    // and never waited on — which is the leak, not a lesser version of it.
    assert!(
        !alive(pid),
        "ffmpeg {pid} survived the Recorder being dropped"
    );
    std::fs::remove_dir_all(&root).ok();
}

/// The pid of the ffmpeg writing into `root`, found the way the start-up sweep
/// finds an orphan: by the workspace path in its argument list, never by name.
#[cfg(unix)]
fn child_pid(root: &std::path::Path) -> Option<u32> {
    let out = std::process::Command::new("pgrep")
        .arg("-f")
        .arg(root.to_str()?)
        .output()
        .ok()?;
    String::from_utf8_lossy(&out.stdout)
        .split_whitespace()
        .find_map(|p| p.parse().ok())
}

#[test]
fn a_region_with_no_area_is_refused_before_ffmpeg_is_spawned() {
    // No ffmpeg guard: the point is that nothing is spawned, so this has to pass
    // on a machine without it too.
    let workspace = Workspace::create().expect("workspace");
    let root = workspace.root().to_path_buf();
    let zero = ScreenPixelRect {
        x: 0,
        y: 0,
        w: 0,
        h: 0,
    };

    let err = Recorder::start(&synthetic(zero), workspace)
        .err()
        .expect("a zero-area region must be refused");
    assert!(
        err.to_string().contains("refusing"),
        "unhelpful refusal: {err}"
    );

    std::fs::remove_dir_all(&root).ok();
}
