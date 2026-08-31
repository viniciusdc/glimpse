//! Where the two windows of the macOS frame go.
//!
//! Kept free of AppKit and GTK so it compiles and its tests run everywhere, for
//! the same reason [`crate::geometry`] is: this is arithmetic, the arithmetic is
//! where the frame silently comes out wrong, and arithmetic is cheap to test on
//! a machine with no screen.
//!
//! Two windows, per
//! [ADR 0015](../../docs/adr/0015-the-frame-is-two-windows.md): a chrome window
//! that takes clicks, and a frame window that takes none anywhere because it
//! sets `ignoresMouseEvents`. The frame window *does* cover the hole — that is
//! the difference from the five-window composition ADR 0011 proposed, and it is
//! allowed precisely because the window is not interactive.

use crate::geometry::AppKitRect;

/// Where each window of the frame sits, given the hole it surrounds.
///
/// All rectangles are AppKit: bottom-left origin, y upward, in points.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Layout {
    /// The chrome, above the frame. ADR 0006's header, as its own window.
    /// Interactive: this is what drags, resizes and holds the controls.
    pub chrome: AppKitRect,
    /// The visual frame: the border, and a transparent middle.
    ///
    /// **Covers the hole**, and takes no clicks anywhere.
    pub frame: AppKitRect,
    /// The status bar and the sheet, BELOW the frame.
    ///
    /// Below, because that is where they are on X11 and the layout is part of the
    /// design ([ADR 0016](../../docs/adr/0016-the-chrome-is-above-and-below.md)).
    /// The header carries what you set before recording; the status line carries
    /// what happened after, and the sheet opens directly under the region that
    /// was just recorded.
    ///
    /// Its **top** edge is the anchor — glued to the frame's bottom — so it grows
    /// downward when the sheet appears. The header is the opposite: its bottom
    /// edge is the anchor and it grows upward. Getting that backwards puts the
    /// sheet over the recording area.
    pub status: AppKitRect,
    /// The region that gets recorded. Not a window, and not the frame window's
    /// bounds: it is the frame inset by the border on every side.
    pub hole: AppKitRect,
}

/// Lay out a frame around `hole`.
///
/// `border` is the visible frame thickness, `chrome_height` the bar above it.
pub fn lay_out(hole: AppKitRect, border: f64, chrome_height: f64, status_height: f64) -> Layout {
    let b = border;
    let frame = AppKitRect {
        x: hole.x - b,
        y: hole.y - b,
        w: hole.w + 2.0 * b,
        h: hole.h + 2.0 * b,
    };
    Layout {
        chrome: AppKitRect {
            x: frame.x,
            y: frame.y + frame.h,
            w: frame.w,
            h: chrome_height,
        },
        // AppKit y counts up, so "below the frame" means a LOWER y. The status
        // window's top edge is at the frame's bottom, and its origin sits
        // status_height further down.
        status: AppKitRect {
            x: frame.x,
            y: frame.y - status_height,
            w: frame.w,
            h: status_height,
        },
        frame,
        hole,
    }
}

impl Layout {
    /// The hole, derived back from the frame window and the border.
    ///
    /// Exists so the two can be checked against each other. The recorded region
    /// is computed from `hole`, and the border is drawn by insetting the frame
    /// window — if those two ever disagree, the recording and what the user sees
    /// framed are different rectangles, which is the whole failure this project
    /// keeps guarding against.
    pub fn hole_from_frame(&self, border: f64) -> AppKitRect {
        AppKitRect {
            x: self.frame.x + border,
            y: self.frame.y + border,
            w: self.frame.w - 2.0 * border,
            h: self.frame.h - 2.0 * border,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const HOLE: AppKitRect = AppKitRect {
        x: 300.0,
        y: 200.0,
        w: 640.0,
        h: 400.0,
    };
    const B: f64 = 3.0;
    const H: f64 = 44.0;

    /// The two descriptions of the recorded region must agree. One comes from
    /// the hole the caller asked for, the other from insetting the window that
    /// draws the border. If they drift, the user frames one rectangle and
    /// records another.
    #[test]
    fn the_hole_and_the_inset_frame_describe_the_same_rectangle() {
        let l = lay_out(HOLE, B, H, 20.0);
        assert_eq!(l.hole_from_frame(B), HOLE);
    }

    /// The frame window surrounds the hole by exactly the border on each side.
    #[test]
    fn the_frame_surrounds_the_hole_by_the_border() {
        let l = lay_out(HOLE, B, H, 20.0);
        assert_eq!(l.frame.x, HOLE.x - B);
        assert_eq!(l.frame.y, HOLE.y - B);
        assert_eq!(l.frame.w, HOLE.w + 2.0 * B);
        assert_eq!(l.frame.h, HOLE.h + 2.0 * B);
    }

    /// The chrome sits directly on top of the frame with no gap and no overlap.
    /// A gap shows desktop between the bar and the border; an overlap hides part
    /// of one of them.
    #[test]
    fn the_chrome_sits_flush_on_top_of_the_frame() {
        let l = lay_out(HOLE, B, H, 20.0);
        assert_eq!(l.chrome.y, l.frame.y + l.frame.h);
        assert_eq!(l.chrome.x, l.frame.x);
        assert_eq!(l.chrome.w, l.frame.w);
    }

    /// The chrome must never overlap the hole, because it is opaque and would be
    /// recorded. The frame window covering the hole is fine — its middle is
    /// transparent — but the chrome is not transparent anywhere.
    #[test]
    fn the_chrome_never_overlaps_the_hole() {
        let l = lay_out(HOLE, B, H, 20.0);
        assert!(!overlaps(l.chrome, l.hole));
    }

    /// Control for the test above. Without it, `overlaps` could return `false`
    /// unconditionally and the invariant would pass while proving nothing.
    #[test]
    fn an_overlapping_chrome_is_detected() {
        let mut l = lay_out(HOLE, B, H, 20.0);
        l.chrome = HOLE;
        assert!(overlaps(l.chrome, l.hole));
    }

    /// A zero border is reachable from CSS and must not produce a frame smaller
    /// than the hole it is meant to surround.
    #[test]
    fn a_zero_border_leaves_the_frame_equal_to_the_hole() {
        let l = lay_out(HOLE, 0.0, H, 20.0);
        assert_eq!(l.frame, HOLE);
        assert_eq!(l.hole_from_frame(0.0), HOLE);
        assert!(!overlaps(l.chrome, l.hole));
    }

    fn overlaps(a: AppKitRect, b: AppKitRect) -> bool {
        a.x < b.x + b.w && b.x < a.x + a.w && a.y < b.y + b.h && b.y < a.y + a.h
    }
}
