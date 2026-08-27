//! Record a fixed region for a few seconds, with no GTK window involved.
//!
//! ```sh
//! cargo run -p glimpse-x11 --example record -- 2        # seconds
//! ```
//!
//! Exercises the whole capture path — workspace creation, ffmpeg spawn, graceful
//! stop, container finalisation — without needing the framing window. Useful for
//! confirming that a file produced this way is *valid*, not merely present.

use anyhow::Result;
use glimpse_core::capture::{GrabRequest, Recorder, Workspace};
use glimpse_core::geometry::ScreenPixelRect;
use glimpse_x11::grab::X11Capture;
use std::time::Duration;

fn main() -> Result<()> {
    let secs: u64 = std::env::args()
        .nth(1)
        .and_then(|a| a.parse().ok())
        .unwrap_or(2);

    let grab = X11Capture::from_env()?.grab(&GrabRequest {
        rect: ScreenPixelRect {
            x: 100,
            y: 100,
            w: 640,
            h: 480,
        },
        framerate: Some(15),
        capture_mouse: true,
    });

    let workspace = Workspace::create()?;
    println!("workspace : {}", workspace.root().display());

    let recorder = Recorder::start(&grab, workspace)?;
    println!(
        "recording : {}x{} at {},{} for {secs}s",
        grab.rect.w, grab.rect.h, grab.rect.x, grab.rect.y
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
