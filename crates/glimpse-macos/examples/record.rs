//! Record a fixed region of the screen on macOS, end to end, with no window
//! involved.
//!
//! ```sh
//! cargo run -p glimpse-macos --example record -- 3        # seconds
//! ```
//!
//! This is the first thing that proves macOS can record at all. It goes through
//! the real seam — `AvfCapture` builds a `GrabCommand`, `glimpse-core` owns the
//! ffmpeg child and writes the lossless intermediate, and the same encoder the
//! X11 app uses turns it into a GIF. Nothing here is a mock.
//!
//! It deliberately does not involve a framing window. The rectangle is
//! hardcoded, so this is independent of every open question in
//! [ADR 0011](../../../docs/adr/0011-why-the-macos-frame-is-more-than-one-window.md)
//! about how the frame is composed. Those questions can be answered later
//! without invalidating anything below.
//!
//! **macOS will ask for Screen Recording permission, and it grants it to the
//! program running this — your terminal — not to the binary.** A refusal shows up
//! as ffmpeg failing to open the device, which reads exactly like a bug in the
//! code.

use anyhow::{Context, Result};
use glimpse_core::capture::{GrabRequest, Recorder, Workspace};
use glimpse_core::encode::{encode, Canceller, OutputFormat};
use glimpse_core::geometry::ScreenPixelRect;
use glimpse_macos::AvfCapture;
use std::time::Duration;

fn main() -> Result<()> {
    let secs: u64 = std::env::args()
        .nth(1)
        .and_then(|a| a.parse().ok())
        .unwrap_or(3);

    // Global device pixels, top-left origin, y down — the convention
    // `ScreenPixelRect` documents. On a 2x display this is a 640x400 point
    // region near the top-left. Nothing flips it: `crop` counts the same way.
    let rect = ScreenPixelRect {
        x: 200,
        y: 200,
        w: 1280,
        h: 800,
    };

    let backend = AvfCapture::discover().context("finding the screen capture device")?;
    println!("device    : {} (discovered, not assumed)", backend.device());

    let grab = backend.grab(&GrabRequest {
        rect,
        framerate: Some(15),
        capture_mouse: false,
    });
    println!("filter    : {}", grab.filter.as_deref().unwrap_or("none"));
    println!(
        "recording : {}x{} at {},{} for {secs}s",
        rect.w, rect.h, rect.x, rect.y
    );

    let workspace = Workspace::create()?;
    println!("workspace : {}", workspace.root().display());

    let recorder = Recorder::start(&grab, workspace)?;
    std::thread::sleep(Duration::from_secs(secs));
    let video = recorder.stop().context("stopping the recorder")?;

    let bytes = std::fs::metadata(&video.path)?.len();
    println!("captured  : {} ({bytes} bytes)", video.path.display());

    let destination = std::env::temp_dir().join("glimpse-macos-example.gif");
    let out = encode(
        &video.path,
        &destination,
        OutputFormat::Gif,
        &Canceller::new(),
    )
    .context("encoding the capture to GIF")?;
    let gif_bytes = std::fs::metadata(&out)?.len();
    println!("encoded   : {} ({gif_bytes} bytes)", out.display());

    println!();
    println!("Now verify both are VALID rather than merely present:");
    println!(
        "  ffprobe -v error -show_entries stream=codec_name,pix_fmt -of csv=p=0 {}",
        video.path.display()
    );
    println!("  open {}", out.display());
    println!();
    println!("The intermediate should report ffv1,bgr0 — nothing converted on the way in.");
    println!("And LOOK at the GIF: it should show the region described above and no more.");

    // The workspace is preserved on purpose so the intermediate can be probed.
    Ok(())
}
