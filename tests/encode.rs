//! Encoding tests. The collision policy and argument shapes need nothing at all;
//! the end-to-end encode needs ffmpeg but no display, because ffmpeg can
//! synthesise its own input.

use glimpse::encode::{
    encode, encode_args, encode_cancellable, free_destination, mp4_args, palette_args, Canceller,
    OutputFormat,
};
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
    let err = encode(
        Path::new("/nope/missing.mkv"),
        Path::new("/tmp/out.gif"),
        OutputFormat::Gif,
    )
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

    let out = encode(&source, &dir.join("result.gif"), OutputFormat::Gif).expect("encode");
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
    let again = encode(&source, &dir.join("result.gif"), OutputFormat::Gif).expect("second encode");
    assert_eq!(again, dir.join("result-1.gif"));
    assert!(dir.join("result.gif").exists(), "the original must survive");

    std::fs::remove_dir_all(&dir).ok();
}

// ---------------------------------------------------------------------- mp4 --

#[test]
fn mp4_always_crops_to_even_dimensions() {
    // H.264 with yuv420p REQUIRES even dimensions, and a framing window produces
    // odd ones constantly — the first real capture this project made was 754x437.
    // Without this filter ffmpeg fails outright.
    let args = mp4_args(Path::new("/tmp/in.mkv"), Path::new("/tmp/out.mp4"));
    assert_eq!(
        value_of(&args, "-vf"),
        Some("crop=trunc(iw/2)*2:trunc(ih/2)*2")
    );
}

#[test]
fn mp4_never_rescales_because_rescaling_blurs_text() {
    // crop drops at most one row and leaves every other pixel untouched. scale
    // resamples the whole frame, which is the worst outcome for a screencast.
    let args = mp4_args(Path::new("/tmp/in.mkv"), Path::new("/tmp/out.mp4"));
    assert!(
        !args.iter().any(|a| a.contains("scale=")),
        "must not rescale: {args:?}"
    );
}

#[test]
fn mp4_targets_the_pixel_format_everything_can_play() {
    let args = mp4_args(Path::new("/tmp/in.mkv"), Path::new("/tmp/out.mp4"));
    assert_eq!(value_of(&args, "-c:v"), Some("libx264"));
    assert_eq!(value_of(&args, "-pix_fmt"), Some("yuv420p"));
    assert_eq!(value_of(&args, "-movflags"), Some("+faststart"));
    assert_eq!(value_of(&args, "-f"), Some("mp4"));
}

#[test]
fn formats_map_to_their_own_extensions() {
    assert_eq!(OutputFormat::Gif.extension(), "gif");
    assert_eq!(OutputFormat::Mp4.extension(), "mp4");
    assert_eq!(OutputFormat::default(), OutputFormat::Gif);
    assert_eq!(OutputFormat::all().len(), 2);
}

#[test]
fn encoding_an_odd_sized_clip_to_mp4_produces_a_playable_file() {
    if !ffmpeg_available() {
        eprintln!("skipping: ffmpeg not installed");
        return;
    }
    let dir = std::env::temp_dir().join(format!("glimpse-mp4-test-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let source = dir.join("source.mkv");

    // 754x437 — the odd height that broke the first naive implementation.
    let made = std::process::Command::new("ffmpeg")
        .args([
            "-hide_banner",
            "-loglevel",
            "error",
            "-y",
            "-f",
            "lavfi",
            "-i",
            "testsrc=size=754x437:rate=15:duration=1",
            "-c:v",
            "ffv1",
            "-pix_fmt",
            "bgr0",
        ])
        .arg(&source)
        .output()
        .unwrap();
    assert!(
        made.status.success(),
        "could not synthesise an odd-sized clip"
    );

    let out = encode(&source, &dir.join("result.mp4"), OutputFormat::Mp4).expect("mp4 encode");
    assert!(
        std::fs::metadata(&out).unwrap().len() > 1000,
        "suspiciously small"
    );

    let probe = std::process::Command::new("ffprobe")
        .args([
            "-v",
            "error",
            "-select_streams",
            "v:0",
            "-show_entries",
            "stream=codec_name,width,height,pix_fmt",
            "-of",
            "csv=p=0",
        ])
        .arg(&out)
        .output()
        .unwrap();
    let info = String::from_utf8_lossy(&probe.stdout).trim().to_string();
    assert!(info.starts_with("h264,"), "not h264: {info}");
    assert!(
        info.contains("754,436"),
        "height should be cropped to even: {info}"
    );
    assert!(info.contains("yuv420p"), "wrong pixel format: {info}");

    std::fs::remove_dir_all(&dir).ok();
}

// ------------------------------------------------------------- cancellation --

#[test]
fn a_canceller_that_already_fired_refuses_before_spawning_anything() {
    if !ffmpeg_available() {
        eprintln!("skipping: ffmpeg not installed");
        return;
    }
    let dir = std::env::temp_dir().join(format!("glimpse-cancel-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let source = dir.join("source.mkv");
    std::process::Command::new("ffmpeg")
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

    let cancel = Canceller::new();
    cancel.cancel();
    assert!(cancel.is_cancelled());

    let err = encode_cancellable(&source, &dir.join("out.gif"), OutputFormat::Gif, &cancel)
        .expect_err("a cancelled encode must not succeed");
    assert!(
        err.to_string().contains("cancel") || format!("{err:#}").contains("cancel"),
        "got: {err:#}"
    );

    // The recording is the expensive thing; cancelling must never cost it.
    assert!(source.exists(), "source must survive a cancelled encode");
    assert!(
        !dir.join("out.gif").exists(),
        "no partial output should be committed"
    );
    let leftovers: Vec<_> = std::fs::read_dir(&dir)
        .unwrap()
        .filter_map(|e| e.ok())
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .filter(|n| n.starts_with(".glimpse-"))
        .collect();
    assert!(leftovers.is_empty(), "left staging behind: {leftovers:?}");

    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn cancelling_is_safe_before_after_and_twice() {
    // The UI cancels on every cleanup path, including ones where no encode ever
    // ran, so this must be inert rather than a panic.
    let cancel = Canceller::new();
    assert!(!cancel.is_cancelled());
    cancel.cancel();
    cancel.cancel();
    assert!(cancel.is_cancelled());

    let clone = cancel.clone();
    clone.cancel();
    assert!(cancel.is_cancelled(), "clones share one encode");
}

#[test]
fn a_fresh_canceller_does_not_block_an_encode() {
    if !ffmpeg_available() {
        eprintln!("skipping: ffmpeg not installed");
        return;
    }
    let dir = std::env::temp_dir().join(format!("glimpse-nocancel-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let source = dir.join("source.mkv");
    std::process::Command::new("ffmpeg")
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

    let out = encode_cancellable(
        &source,
        &dir.join("out.gif"),
        OutputFormat::Gif,
        &Canceller::new(),
    )
    .expect("an untouched canceller must not interfere");
    assert_eq!(&std::fs::read(&out).unwrap()[..6], b"GIF89a");

    std::fs::remove_dir_all(&dir).ok();
}
