//! Progress reporting. Runs a real encode and watches the number move, because
//! the failure this guards against is a bar that renders confidently and means
//! nothing.

use glimpse_core::encode::{encode_reporting, Canceller, OutputFormat, Progress};
use std::path::{Path, PathBuf};

fn ffmpeg_available() -> bool {
    std::process::Command::new("ffmpeg")
        .arg("-version")
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

fn source(dir: &Path) -> PathBuf {
    let p = dir.join("src.mkv");
    let ok = std::process::Command::new("ffmpeg")
        .args([
            "-hide_banner",
            "-loglevel",
            "error",
            "-y",
            "-f",
            "lavfi",
            "-i",
            "testsrc=size=960x540:rate=25:duration=3",
            "-c:v",
            "ffv1",
            "-pix_fmt",
            "bgr0",
        ])
        .arg(&p)
        .output()
        .unwrap();
    assert!(ok.status.success());
    p
}

#[test]
fn progress_is_unknown_before_anything_has_been_reported() {
    // Distinct from zero: "no answer yet" and "no progress" are different claims
    // and the UI draws them differently.
    assert_eq!(Progress::new().fraction(), None);
}

#[test]
fn a_real_encode_reports_progress_that_advances_and_ends_complete() {
    if !ffmpeg_available() {
        eprintln!("skipping: ffmpeg not installed");
        return;
    }
    let dir = std::env::temp_dir().join(format!("glimpse-prog-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let src = source(&dir);

    let progress = Progress::new();
    let watcher = progress.clone();
    // Stop sampling when the encode returns rather than after a fixed number of
    // ticks — the fixed version kept the test alive for seconds after the work
    // was done.
    let done = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    let stop = done.clone();
    let samples = std::thread::spawn(move || {
        let mut seen: Vec<f64> = Vec::new();
        while !stop.load(std::sync::atomic::Ordering::Relaxed) {
            if let Some(f) = watcher.fraction() {
                if seen.last().copied() != Some(f) {
                    seen.push(f);
                }
            }
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        seen
    });

    let out = encode_reporting(
        &src,
        &dir.join("out.gif"),
        OutputFormat::Gif,
        &Canceller::new(),
        &progress,
    )
    .expect("encode");
    assert!(out.exists());

    done.store(true, std::sync::atomic::Ordering::Relaxed);
    let seen = samples.join().unwrap();
    assert!(
        seen.len() >= 2,
        "progress never moved — got {seen:?}; a bar that does not advance is worse than none"
    );
    assert!(
        seen.windows(2).all(|w| w[1] >= w[0]),
        "progress went backwards: {seen:?}"
    );
    assert!(
        seen.iter().all(|f| (0.0..=1.0).contains(f)),
        "progress left 0..=1: {seen:?}"
    );
    // The two GIF passes are weighted 0..0.35 and 0.35..1, so a completed encode
    // must have crossed the handover rather than stopping at the first pass.
    assert!(
        seen.iter().any(|f| *f > 0.4),
        "progress never passed the palette stage: {seen:?}"
    );

    std::fs::remove_dir_all(&dir).ok();
}
