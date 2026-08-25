//! Record a fixed region for a few seconds, with no GTK window involved.
//!
//! ```sh
//! cargo run --example record -- 2        # seconds
//! ```
//!
//! Exercises the whole capture path — workspace creation, ffmpeg spawn, graceful
//! stop, container finalisation — without needing the framing window. Useful for
//! confirming that a file produced this way is *valid*, not merely present.

use anyhow::Result;
use glimpse::capture::{Recorder, RecorderConfig, Workspace};
use glimpse::geometry::RootPixelRect;
use std::time::Duration;

fn main() -> Result<()> {
    let secs: u64 = std::env::args()
        .nth(1)
        .and_then(|a| a.parse().ok())
        .unwrap_or(2);

    let cfg = RecorderConfig {
        display: RecorderConfig::display_from_env()?,
        rect: RootPixelRect {
            x: 100,
            y: 100,
            w: 640,
            h: 480,
        },
        framerate: 15,
        capture_mouse: true,
    };

    let workspace = Workspace::create()?;
    println!("workspace : {}", workspace.root().display());

    let recorder = Recorder::start(&cfg, workspace)?;
    println!(
        "recording : {}x{} at {},{} for {secs}s",
        cfg.rect.w, cfg.rect.h, cfg.rect.x, cfg.rect.y
    );
    std::thread::sleep(Duration::from_secs(secs));

    let video = recorder.stop()?;
    let bytes = std::fs::metadata(&video.path)?.len();
    println!("captured  : {} ({bytes} bytes)", video.path.display());
    println!("\nNow verify it is VALID, not merely present:");
    println!(
        "  ffprobe -v error -show_entries format=duration,format_name -of default=nk=0 {}",
        video.path.display()
    );
    Ok(())
}
