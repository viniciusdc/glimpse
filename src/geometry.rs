//! The conversion chain from a widget to a root-pixel capture rectangle.
//!
//! Per [ADR 0002] the stages are distinct types, so logical and device coordinates
//! cannot be mixed by accident:
//!
//! ```text
//! WidgetRect -> SurfaceRect -> device pixels -> RootPixelRect -> clipped
//! ```
//!
//! Two rules established by the spike ([ADR 0000]) live here:
//!
//! 1. `compute_bounds` returns the widget's **border box**. If the widget draws a
//!    border, that border lands inside the capture. Glimpse fixes this
//!    structurally — the capture widget draws nothing and the frame is painted by
//!    its parent — rather than by subtracting a magic number. `verify_against`
//!    exists to keep it honest.
//! 2. Never derive DPI from monitor physical size. The monitor this was written
//!    against reports 1mm x 1mm, and lying EDIDs are common. Only the integer
//!    scale factor is trusted.

use anyhow::{anyhow, Result};
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

/// Device pixels, absolute, relative to the X root window. This is the only
/// rectangle ffmpeg's `x11grab` ever sees.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RootPixelRect {
    pub x: i32,
    pub y: i32,
    pub w: i32,
    pub h: i32,
}

impl RootPixelRect {
    /// `x11grab` geometry: `-video_size WxH -i DISPLAY+X,Y`
    pub fn video_size(&self) -> String {
        format!("{}x{}", self.w, self.h)
    }

    pub fn is_capturable(&self) -> bool {
        self.w > 0 && self.h > 0
    }

    /// Clip to the root window. A frame dragged half off-screen must not ask
    /// x11grab for pixels that do not exist.
    pub fn clipped_to(self, root_w: i32, root_h: i32) -> Self {
        let x = self.x.clamp(0, root_w);
        let y = self.y.clamp(0, root_h);
        Self {
            x,
            y,
            w: (self.w - (x - self.x)).min(root_w - x).max(0),
            h: (self.h - (y - self.y)).min(root_h - y).max(0),
        }
    }
}

/// Run the full chain for `target` inside `window`.
///
/// `target` must be a widget that paints nothing of its own — see rule 1 above.
pub fn capture_rect(
    window: &impl IsA<gtk::Window>,
    target: &impl IsA<gtk::Widget>,
    probe: &X11Probe,
) -> Result<RootPixelRect> {
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

    // 4. surface origin -> root pixels
    let xid = crate::x11probe::window_xid(window)?;
    let (ox, oy) = probe.surface_origin(xid)?;

    let rect = RootPixelRect {
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clipping_keeps_rects_inside_the_root() {
        let r = RootPixelRect {
            x: -50,
            y: -20,
            w: 200,
            h: 100,
        }
        .clipped_to(1920, 1080);
        assert_eq!(
            r,
            RootPixelRect {
                x: 0,
                y: 0,
                w: 150,
                h: 80
            }
        );
    }

    #[test]
    fn clipping_truncates_at_the_far_edge() {
        let r = RootPixelRect {
            x: 1800,
            y: 1000,
            w: 400,
            h: 200,
        }
        .clipped_to(1920, 1080);
        assert_eq!(
            r,
            RootPixelRect {
                x: 1800,
                y: 1000,
                w: 120,
                h: 80
            }
        );
    }

    #[test]
    fn fully_offscreen_rect_is_not_capturable() {
        let r = RootPixelRect {
            x: 5000,
            y: 5000,
            w: 100,
            h: 100,
        }
        .clipped_to(1920, 1080);
        assert!(
            !r.is_capturable(),
            "x11grab must never be handed a zero/negative size"
        );
    }
}
