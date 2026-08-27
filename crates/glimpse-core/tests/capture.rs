//! Capture tests that need neither a display nor an ffmpeg process.
//!
//! What core contributes to an ffmpeg invocation is the output half — the codec,
//! the container, and where the filter lands. Every flag in it is a licensing
//! commitment (ADR 0003 — derived from ffmpeg's docs, not from Peek).
//!
//! The *input* half belongs to whichever backend built the [`GrabCommand`], and
//! is tested beside it in `glimpse-x11`.

use glimpse_core::capture::{GrabCommand, Workspace};
use glimpse_core::geometry::ScreenPixelRect;
use std::path::Path;

fn command(filter: Option<&str>) -> GrabCommand {
    GrabCommand {
        rect: ScreenPixelRect {
            x: 100,
            y: 200,
            w: 640,
            h: 480,
        },
        input: vec!["-f".into(), "somegrab".into(), "-i".into(), ":0".into()],
        filter: filter.map(str::to_string),
        pix_fmt: Some("bgr0".into()),
    }
}

/// Value following `flag`, if present.
fn value_of<'a>(args: &'a [String], flag: &str) -> Option<&'a str> {
    let i = args.iter().position(|a| a == flag)?;
    args.get(i + 1).map(String::as_str)
}

/// `-f` appears twice: once for the input demuxer and once for the output muxer.
/// Taking the first would assert on the input, which is how this helper's absence
/// produced a confidently wrong test.
fn last_value_of<'a>(args: &'a [String], flag: &str) -> Option<&'a str> {
    let i = args.iter().rposition(|a| a == flag)?;
    args.get(i + 1).map(String::as_str)
}

#[test]
fn the_backends_input_arguments_are_passed_through_untouched() {
    let args = command(None).recording_args(Path::new("/tmp/out.mkv"));
    assert_eq!(value_of(&args, "-i"), Some(":0"));
    assert_eq!(
        value_of(&args, "-f"),
        Some("somegrab"),
        "core must not rewrite the demuxer the backend chose"
    );
}

#[test]
fn the_intermediate_is_conversion_free() {
    // x11grab emits bgr0 and ffv1 stores bgr0, so nothing is converted. This is
    // why ADR 0002's ban on calling the intermediate "lossless" without measuring
    // can now be satisfied — verified with ffprobe on a real capture. The format
    // travels on the GrabCommand because it is a fact about the *source*.
    let args = command(None).recording_args(Path::new("/tmp/out.mkv"));
    assert_eq!(value_of(&args, "-c:v"), Some("ffv1"));
    assert_eq!(value_of(&args, "-pix_fmt"), Some("bgr0"));
}

/// The reason `GrabCommand` is a struct rather than a `Vec<String>`: x11grab
/// crops by grabbing, avfoundation captures a whole display and must crop after
/// the fact. A flat argument list could only express the first.
#[test]
fn a_crop_filter_is_emitted_only_when_the_backend_supplied_one() {
    let plain = command(None).recording_args(Path::new("/tmp/out.mkv"));
    assert!(!plain.iter().any(|a| a == "-vf"), "{plain:?}");

    let cropped = command(Some("crop=640:480:100:200")).recording_args(Path::new("/tmp/out.mkv"));
    assert_eq!(value_of(&cropped, "-vf"), Some("crop=640:480:100:200"));
}

/// A filter after the output path would be read as a second output.
#[test]
fn the_filter_precedes_the_output_path() {
    let args = command(Some("crop=1:2:3:4")).recording_args(Path::new("/tmp/out.mkv"));
    let vf = args.iter().position(|a| a == "-vf").unwrap();
    assert!(vf < args.len() - 1);
    assert_eq!(args.last().map(String::as_str), Some("/tmp/out.mkv"));
}

#[test]
fn the_output_path_is_last_so_ffmpeg_reads_it_as_the_destination() {
    let args = command(None).recording_args(Path::new("/tmp/glimpse/out.mkv"));
    assert_eq!(
        args.last().map(String::as_str),
        Some("/tmp/glimpse/out.mkv")
    );
}

// ----------------------------------------------------------------- snapshot --

#[test]
fn a_snapshot_states_the_png_codec_not_just_the_container() {
    // image2 is a container whose default encoder is mjpeg, and the output is
    // staged as `.png.part` so ffmpeg can infer nothing from the extension.
    // Without an explicit codec Glimpse writes a JPEG into a file called .png —
    // which it did, until someone ran `identify` on the result instead of
    // trusting the filename.
    let args = command(None).snapshot_args(Path::new("/tmp/shot.png.part"));
    assert_eq!(value_of(&args, "-c:v"), Some("png"));
    assert_eq!(last_value_of(&args, "-f"), Some("image2"));
    assert_eq!(
        value_of(&args, "-f"),
        Some("somegrab"),
        "input demuxer unchanged"
    );
}

#[test]
fn a_snapshot_is_exactly_one_frame() {
    let args = command(None).snapshot_args(Path::new("/tmp/shot.png.part"));
    assert_eq!(value_of(&args, "-frames:v"), Some("1"));
}

/// A still is a PNG whatever the source's native format is, so stating a pixel
/// format here would only constrain the encoder for no reason.
#[test]
fn a_snapshot_does_not_inherit_the_sources_pixel_format() {
    let args = command(None).snapshot_args(Path::new("/tmp/shot.png.part"));
    assert!(!args.iter().any(|a| a == "-pix_fmt"), "{args:?}");
}

#[test]
fn a_snapshot_crops_the_same_way_a_recording_does() {
    let args = command(Some("crop=640:480:100:200")).snapshot_args(Path::new("/tmp/shot.png.part"));
    assert_eq!(value_of(&args, "-vf"), Some("crop=640:480:100:200"));
}

// ---------------------------------------------------------------- workspace --

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
