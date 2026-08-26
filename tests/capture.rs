//! Capture tests that need neither a display nor an ffmpeg process.
//!
//! The argument builder is the highest-value thing to pin here: it is the front
//! door of the whole recording pipeline, and every flag in it is a licensing
//! commitment (ADR 0003 — derived from ffmpeg's docs, not from Peek).

use glimpse::capture::{snapshot_args, x11grab_args, RecorderConfig, Workspace};
use glimpse::geometry::RootPixelRect;
use std::path::{Path, PathBuf};

fn cfg() -> RecorderConfig {
    RecorderConfig {
        display: ":0".into(),
        rect: RootPixelRect {
            x: 100,
            y: 200,
            w: 640,
            h: 480,
        },
        framerate: 15,
        capture_mouse: true,
    }
}

/// Value following `flag`, if present.
fn value_of<'a>(args: &'a [String], flag: &str) -> Option<&'a str> {
    let i = args.iter().position(|a| a == flag)?;
    args.get(i + 1).map(String::as_str)
}

/// `-f` appears twice: once for the input demuxer and once for the output muxer.
/// Taking the first would assert on x11grab, which is how this helper's absence
/// produced a confidently wrong test.
fn last_value_of<'a>(args: &'a [String], flag: &str) -> Option<&'a str> {
    let i = args.iter().rposition(|a| a == flag)?;
    args.get(i + 1).map(String::as_str)
}

#[test]
fn the_region_is_passed_as_documented_options_not_baked_into_the_url() {
    // ffmpeg documents -grab_x/-grab_y. Encoding the origin into the input URL
    // works too, but is fragile against display names containing punctuation.
    let args = x11grab_args(&cfg(), Path::new("/tmp/out.mkv"));
    assert_eq!(value_of(&args, "-grab_x"), Some("100"));
    assert_eq!(value_of(&args, "-grab_y"), Some("200"));
    assert_eq!(
        value_of(&args, "-i"),
        Some(":0"),
        "the input is the display alone"
    );
}

#[test]
fn size_and_framerate_come_from_the_request() {
    let args = x11grab_args(&cfg(), Path::new("/tmp/out.mkv"));
    assert_eq!(value_of(&args, "-video_size"), Some("640x480"));
    assert_eq!(value_of(&args, "-framerate"), Some("15"));
}

#[test]
fn the_mouse_toggle_maps_to_draw_mouse() {
    let with = x11grab_args(&cfg(), Path::new("/tmp/out.mkv"));
    assert_eq!(value_of(&with, "-draw_mouse"), Some("1"));

    let without = x11grab_args(
        &RecorderConfig {
            capture_mouse: false,
            ..cfg()
        },
        Path::new("/tmp/out.mkv"),
    );
    assert_eq!(value_of(&without, "-draw_mouse"), Some("0"));
}

#[test]
fn the_intermediate_is_conversion_free() {
    // x11grab emits bgr0 and ffv1 stores bgr0, so nothing is converted. This is
    // why ADR 0002's ban on calling the intermediate "lossless" without measuring
    // can now be satisfied — verified with ffprobe on a real capture.
    let args = x11grab_args(&cfg(), Path::new("/tmp/out.mkv"));
    assert_eq!(value_of(&args, "-c:v"), Some("ffv1"));
    assert_eq!(value_of(&args, "-pix_fmt"), Some("bgr0"));
}

#[test]
fn the_output_path_is_last_so_ffmpeg_reads_it_as_the_destination() {
    let args = x11grab_args(&cfg(), Path::new("/tmp/glimpse/out.mkv"));
    assert_eq!(
        args.last().map(String::as_str),
        Some("/tmp/glimpse/out.mkv")
    );
}

#[test]
fn a_workspace_owns_a_real_directory_and_removes_it_on_dispose() {
    let ws = Workspace::create().expect("create workspace");
    let root = ws.root().to_path_buf();
    assert!(root.is_dir(), "workspace should exist after create");

    std::fs::write(ws.video_path(), b"pretend recording").unwrap();
    assert!(ws.video_path().exists());

    assert_eq!(ws.dispose(false), None);
    assert!(!root.exists(), "dispose(false) must remove the directory");
}

#[test]
fn preserving_a_workspace_keeps_the_recording_and_reports_where_it_is() {
    // A failed encode must not cost the user the capture (ADR 0002).
    let ws = Workspace::create().expect("create workspace");
    let root = ws.root().to_path_buf();
    std::fs::write(ws.video_path(), b"the only copy").unwrap();

    let kept = ws.dispose(true);
    assert_eq!(
        kept.as_deref(),
        Some(root.as_path()),
        "the caller is told where it is"
    );
    assert!(
        root.join("recording.mkv").exists(),
        "the bytes must survive"
    );

    std::fs::remove_dir_all(&root).ok();
}

#[test]
fn two_workspaces_in_one_process_do_not_collide() {
    let a = Workspace::create().unwrap();
    let b = Workspace::create().unwrap();
    assert_ne!(a.root(), b.root());
    a.dispose(false);
    b.dispose(false);
}

#[test]
fn the_display_is_read_from_the_environment_never_guessed() {
    // Guessing :0 could point ffmpeg at a different screen than the one the
    // rectangle was computed against.
    let path: PathBuf = "/tmp/out.mkv".into();
    let args = x11grab_args(
        &RecorderConfig {
            display: ":7.1".into(),
            ..cfg()
        },
        &path,
    );
    assert_eq!(value_of(&args, "-i"), Some(":7.1"));
}

// ----------------------------------------------------------------- snapshot --

#[test]
fn a_snapshot_states_the_png_codec_not_just_the_container() {
    // image2 is a container whose default encoder is mjpeg, and the output is
    // staged as `.png.part` so ffmpeg can infer nothing from the extension.
    // Without an explicit codec Glimpse writes a JPEG into a file called .png —
    // which it did, until someone ran `identify` on the result instead of
    // trusting the filename.
    let args = snapshot_args(&cfg(), Path::new("/tmp/shot.png"));
    assert_eq!(value_of(&args, "-c:v"), Some("png"));
    assert_eq!(last_value_of(&args, "-f"), Some("image2"));
    assert_eq!(
        value_of(&args, "-f"),
        Some("x11grab"),
        "input demuxer unchanged"
    );
}

#[test]
fn a_snapshot_is_exactly_one_frame() {
    let args = snapshot_args(&cfg(), Path::new("/tmp/shot.png"));
    assert_eq!(value_of(&args, "-frames:v"), Some("1"));
    assert!(
        !args.iter().any(|a| a == "-framerate"),
        "a still has no framerate: {args:?}"
    );
}

#[test]
fn a_snapshot_grabs_the_same_region_a_recording_would() {
    let args = snapshot_args(&cfg(), Path::new("/tmp/shot.png"));
    assert_eq!(value_of(&args, "-grab_x"), Some("100"));
    assert_eq!(value_of(&args, "-grab_y"), Some("200"));
    assert_eq!(value_of(&args, "-video_size"), Some("640x480"));
    assert_eq!(value_of(&args, "-i"), Some(":0"));
}

#[test]
fn the_snapshot_mouse_toggle_follows_the_same_setting() {
    let without = snapshot_args(
        &RecorderConfig {
            capture_mouse: false,
            ..cfg()
        },
        Path::new("/tmp/shot.png"),
    );
    assert_eq!(value_of(&without, "-draw_mouse"), Some("0"));
}
