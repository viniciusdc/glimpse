//! The capture rectangle, and the coordinate space it is measured in.
//!
//! This is the type every frontend must produce and every capture provider must
//! consume, so the convention it carries is stated here rather than assumed at
//! either end — see [`ScreenPixelRect`].
//!
//! The chain that *computes* one is platform work and lives with the frontend
//! (`glimpse-x11::geometry` for X11). Only the destination type is shared.

/// A capture region in **global device pixels**, origin **top-left**, y
/// increasing **downward**.
///
/// Every part of that sentence is load-bearing, and none of it is guessable from
/// the field names:
///
/// * **Global**, not window-relative. Absolute across the whole desktop.
/// * **Device pixels**, not logical points. On a display with a scale factor of
///   2, a 640-point window is 1280 here. Only the integer scale factor is ever
///   trusted for that conversion — never a figure derived from a monitor's
///   reported physical size, because the monitor this was written against
///   reports 1mm x 1mm and lying EDIDs are common.
/// * **Top-left origin, y down.** X11 root coordinates already work this way, so
///   on X11 the convention costs nothing. AppKit does not: screen coordinates
///   put (0,0) at the bottom-left of the primary screen with y increasing
///   upward, so a macOS frontend owes a flip before it can build one of these.
///
/// Why state it at all: with the frontend and the capture provider in different
/// crates, agreement about the origin stops being obvious and becomes an
/// unwritten assumption between two pieces of code that no longer see each
/// other. An origin flip produces a rectangle that is the right size, lands on
/// the wrong pixels, and looks entirely plausible in a log line — which is the
/// failure this project has already paid for once (ADR 0000).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ScreenPixelRect {
    pub x: i32,
    pub y: i32,
    pub w: i32,
    pub h: i32,
}

impl ScreenPixelRect {
    /// The `WxH` form ffmpeg's `-video_size` takes.
    pub fn video_size(&self) -> String {
        format!("{}x{}", self.w, self.h)
    }

    pub fn is_capturable(&self) -> bool {
        self.w > 0 && self.h > 0
    }

    /// Clip to the bounds of the screen this rect is measured against.
    ///
    /// A frame dragged half off-screen must not ask a capture backend for pixels
    /// that do not exist.
    pub fn clipped_to(self, screen_w: i32, screen_h: i32) -> Self {
        let x = self.x.clamp(0, screen_w);
        let y = self.y.clamp(0, screen_h);
        Self {
            x,
            y,
            w: (self.w - (x - self.x)).min(screen_w - x).max(0),
            h: (self.h - (y - self.y)).min(screen_h - y).max(0),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clipping_keeps_rects_inside_the_screen() {
        let r = ScreenPixelRect {
            x: -50,
            y: -20,
            w: 200,
            h: 100,
        }
        .clipped_to(1920, 1080);
        assert_eq!(
            r,
            ScreenPixelRect {
                x: 0,
                y: 0,
                w: 150,
                h: 80
            }
        );
    }

    #[test]
    fn clipping_truncates_at_the_far_edge() {
        let r = ScreenPixelRect {
            x: 1800,
            y: 1000,
            w: 400,
            h: 200,
        }
        .clipped_to(1920, 1080);
        assert_eq!(
            r,
            ScreenPixelRect {
                x: 1800,
                y: 1000,
                w: 120,
                h: 80
            }
        );
    }

    #[test]
    fn fully_offscreen_rect_is_not_capturable() {
        let r = ScreenPixelRect {
            x: 5000,
            y: 5000,
            w: 100,
            h: 100,
        }
        .clipped_to(1920, 1080);
        assert!(
            !r.is_capturable(),
            "a capture backend must never be handed a zero/negative size"
        );
    }

    /// The origin convention is the one thing a new frontend can get wrong while
    /// producing rectangles that look right, so pin it down rather than leaving
    /// it to the doc comment.
    #[test]
    fn the_origin_is_top_left_so_clipping_the_top_edge_shrinks_downward() {
        let r = ScreenPixelRect {
            x: 100,
            y: -30,
            w: 200,
            h: 100,
        }
        .clipped_to(1920, 1080);
        // y increases downward: clipping 30 rows off the TOP leaves the rect
        // starting at 0 and 30 shorter. Under a bottom-left origin this same
        // input would have been off the *bottom* and clipped elsewhere.
        assert_eq!(
            r,
            ScreenPixelRect {
                x: 100,
                y: 0,
                w: 200,
                h: 70
            }
        );
    }
}
