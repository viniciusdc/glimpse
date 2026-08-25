//! Encoding tests. The collision policy and argument shapes need nothing at all;
//! the end-to-end encode needs ffmpeg but no display, because ffmpeg can
//! synthesise its own input.

use glimpse::encode::{encode_args, encode_gif, free_destination, palette_args};
use std::path::{Path, PathBuf};

fn ffmpeg_available() -> bool {
    std::process::Command::new("ffmpeg")
        .arg("-version")
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

fn value_of<'a>(args: &'a [String], flag: &str) -> Option<&'a str> {
    let i = args.iter().position(|a| a == flag)?;
    args.get(i + 1).map(String::as_str)
}

// ------------------------------------------------------------- collisions --

#[test]
fn a_free_name_is_used_unchanged() {
    let p = free_destination(Path::new("/out/glimpse.gif"), |_| false);
    assert_eq!(p, PathBuf::from("/out/glimpse.gif"));
}

#[test]
fn a_taken_name_is_disambiguated_never_replaced() {
    // Silently replacing would destroy a file the user may still want.
    let taken = PathBuf::from("/out/glimpse.gif");
    let p = free_destination(&taken, |c| c == taken);
    assert_eq!(p, PathBuf::from("/out/glimpse-1.gif"));
}

#[test]
fn disambiguation_keeps_counting_past_the_first_clash() {
    let busy = [
        "/out/glimpse.gif",
        "/out/glimpse-1.gif",
        "/out/glimpse-2.gif",
    ];
    let p = free_destination(Path::new("/out/glimpse.gif"), |c| {
        busy.iter().any(|b| Path::new(b) == c)
    });
    assert_eq!(p, PathBuf::from("/out/glimpse-3.gif"));
}

#[test]
fn a_name_without_an_extension_still_disambiguates() {
    let taken = PathBuf::from("/out/capture");
    let p = free_destination(&taken, |c| c == taken);
    assert_eq!(p, PathBuf::from("/out/capture-1"));
}

#[test]
fn collision_handling_never_fails_the_recording() {
    // Failing on a name clash would lose a recording the user just made.
    let p = free_destination(Path::new("/out/x.gif"), |c| {
        c.to_string_lossy().ends_with("x.gif") || c.to_string_lossy().ends_with("x-1.gif")
    });
    assert_eq!(p, PathBuf::from("/out/x-2.gif"));
}

// ---------------------------------------------------------------- arguments --

#[test]
fn the_palette_pass_analyses_the_whole_clip() {
    let args = palette_args(Path::new("/tmp/in.mkv"), Path::new("/tmp/pal.png"));
    assert_eq!(value_of(&args, "-i"), Some("/tmp/in.mkv"));
    assert_eq!(value_of(&args, "-vf"), Some("palettegen"));
    assert_eq!(args.last().map(String::as_str), Some("/tmp/pal.png"));
}

#[test]
fn the_encode_pass_takes_two_inputs_through_lavfi() {
    // paletteuse consumes the video AND the palette, which -vf cannot express.
    let args = encode_args(
        Path::new("/tmp/in.mkv"),
        Path::new("/tmp/pal.png"),
        Path::new("/tmp/out.gif"),
    );
    let inputs: Vec<&String> = args
        .iter()
        .enumerate()
        .filter(|(i, _)| i > &0 && args[i - 1] == "-i")
        .map(|(_, a)| a)
        .collect();
    assert_eq!(inputs, vec!["/tmp/in.mkv", "/tmp/pal.png"]);
    assert_eq!(value_of(&args, "-lavfi"), Some("paletteuse"));
    // The muxer is stated, not inferred: the real output is staged as
    // `.gif.part` so the commit can be an atomic rename.
    assert_eq!(value_of(&args, "-f"), Some("gif"));
    assert_eq!(args.last().map(String::as_str), Some("/tmp/out.gif"));
}

#[test]
fn no_measured_free_options_are_smuggled_in() {
    // stats_mode and diff_mode were measured and earned nothing; a flag with no
    // demonstrated benefit does not belong here. See the module docs.
    let pal = palette_args(Path::new("/a"), Path::new("/b"));
    let enc = encode_args(Path::new("/a"), Path::new("/b"), Path::new("/c"));
    assert!(!pal.iter().any(|a| a.contains("stats_mode")));
    assert!(!enc.iter().any(|a| a.contains("diff_mode")));
}

// -------------------------------------------------------------- end to end --

#[test]
fn a_missing_recording_is_refused_before_ffmpeg_is_spawned() {
    let err = encode_gif(Path::new("/nope/missing.mkv"), Path::new("/tmp/out.gif"))
        .expect_err("should refuse");
    assert!(err.to_string().contains("no recording"), "got: {err}");
}

#[test]
fn encoding_produces_a_real_gif_and_commits_it_atomically() {
    if !ffmpeg_available() {
        eprintln!("skipping: ffmpeg not installed");
        return;
    }
    let dir = std::env::temp_dir().join(format!("glimpse-enc-test-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let source = dir.join("source.mkv");

    // ffmpeg can synthesise its own input, so this needs no display.
    let made = std::process::Command::new("ffmpeg")
        .args([
            "-hide_banner",
            "-loglevel",
            "error",
            "-y",
            "-f",
            "lavfi",
            "-i",
            "testsrc=size=160x120:rate=10:duration=1",
            "-c:v",
            "ffv1",
        ])
        .arg(&source)
        .output()
        .unwrap();
    assert!(made.status.success(), "could not synthesise a source clip");

    let out = encode_gif(&source, &dir.join("result.gif")).expect("encode");
    assert_eq!(out, dir.join("result.gif"));

    // A real GIF, not an empty file.
    let bytes = std::fs::read(&out).unwrap();
    assert!(
        bytes.len() > 100,
        "suspiciously small: {} bytes",
        bytes.len()
    );
    assert_eq!(&bytes[..6], b"GIF89a", "not a GIF header");

    // Nothing staged or left behind: no .part files, no palette.
    let leftovers: Vec<_> = std::fs::read_dir(&dir)
        .unwrap()
        .filter_map(|e| e.ok())
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .filter(|n| n.starts_with(".glimpse-"))
        .collect();
    assert!(
        leftovers.is_empty(),
        "left temp files behind: {leftovers:?}"
    );

    // A second encode to the same destination must not replace the first.
    let again = encode_gif(&source, &dir.join("result.gif")).expect("second encode");
    assert_eq!(again, dir.join("result-1.gif"));
    assert!(dir.join("result.gif").exists(), "the original must survive");

    std::fs::remove_dir_all(&dir).ok();
}
