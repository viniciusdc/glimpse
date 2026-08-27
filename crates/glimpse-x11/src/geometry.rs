//! The conversion chain from a widget to a capture rectangle, on X11.
//!
//! Per [ADR 0002] the stages are distinct types, so logical and device coordinates
//! cannot be mixed by accident:
//!
//! ```text
//! WidgetRect -> SurfaceRect -> device pixels -> ScreenPixelRect -> clipped
//! ```
//!
//! The destination type lives in `glimpse-core` because every frontend has to
//! produce one; everything on the way there is GTK and X11, so it lives here.
//!
//! Two rules established by the spike ([ADR 0000]) live in this module:
//!
//! 1. `compute_bounds` returns the widget's **border box**. If the widget draws a
//!    border, that border lands inside the capture. Glimpse fixes this
//!    structurally — the capture widget draws nothing and the frame is painted by
//!    its parent — rather than by subtracting a magic number.
//!    `verify_against_xwininfo` exists to keep it honest.
//! 2. Never derive DPI from monitor physical size. The monitor this was written
//!    against reports 1mm x 1mm, and lying EDIDs are common. Only the integer
//!    scale factor is trusted.

use anyhow::{anyhow, Result};
use glimpse_core::geometry::ScreenPixelRect;
use gtk::prelude::*;
use gtk4 as gtk;

use crate::x11probe::X11Probe;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct WidgetRect {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SurfaceRect {
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub h: f64,
}

/// Run the full chain for `target` inside `window`.
///
/// `target` must be a widget that paints nothing of its own — see rule 1 above.
pub fn capture_rect(
    window: &impl IsA<gtk::Window>,
    target: &impl IsA<gtk::Widget>,
    probe: &X11Probe,
) -> Result<ScreenPixelRect> {
    let window = window.as_ref();
    let surface = window
        .surface()
        .ok_or_else(|| anyhow!("window has no surface"))?;

    // 1. widget/logical coordinates
    let b = target
        .as_ref()
        .compute_bounds(window)
        .ok_or_else(|| anyhow!("compute_bounds failed — is the widget realized?"))?;
    let wr = WidgetRect {
        x: b.x(),
        y: b.y(),
        w: b.width(),
        h: b.height(),
    };

    // 2. widget -> native surface coordinates
    let (tx, ty) = window.surface_transform();
    let sr = SurfaceRect {
        x: wr.x as f64 + tx,
        y: wr.y as f64 + ty,
        w: wr.w as f64,
        h: wr.h as f64,
    };

    // 3. logical -> device pixels (integer scale factor ONLY)
    let scale = surface.scale_factor();
    let dev = (
        (sr.x * scale as f64).round() as i32,
        (sr.y * scale as f64).round() as i32,
        (sr.w * scale as f64).round() as i32,
        (sr.h * scale as f64).round() as i32,
    );

    // 4. surface origin -> global pixels.
    //
    // No flip is needed here: X11 root coordinates are already top-left with y
    // increasing downward, which is the convention `ScreenPixelRect` documents.
    // A macOS frontend does not get this for free.
    let xid = crate::x11probe::window_xid(window)?;
    let (ox, oy) = probe.surface_origin(xid)?;

    let rect = ScreenPixelRect {
        x: ox + dev.0,
        y: oy + dev.1,
        w: dev.2,
        h: dev.3,
    };

    // 5. clip to something that actually exists
    let (rw, rh) = probe.root_size()?;
    Ok(rect.clipped_to(rw, rh))
}

/// Cross-check the computed origin against an independent source.
///
/// Kept because during the spike the arithmetic agreed with `xwininfo` exactly
/// while still being wrong — agreement here is necessary, never sufficient. The
/// sufficient check is grabbing the rect and looking at the image.
pub fn verify_against_xwininfo(xid: u32) -> Option<(i32, i32)> {
    let out = std::process::Command::new("xwininfo")
        .args(["-id", &xid.to_string()])
        .output()
        .ok()?;
    let text = String::from_utf8_lossy(&out.stdout);
    let grab = |key: &str| -> Option<i32> {
        text.lines()
            .find(|l| l.trim_start().starts_with(key))?
            .rsplit(':')
            .next()?
            .trim()
            .parse()
            .ok()
    };
    Some((
        grab("Absolute upper-left X")?,
        grab("Absolute upper-left Y")?,
    ))
}
