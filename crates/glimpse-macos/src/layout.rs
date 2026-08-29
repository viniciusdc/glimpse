//! Where the five windows of the macOS frame go.
//!
//! Kept free of AppKit and GTK so it compiles and its tests run everywhere, for
//! the same reason [`crate::geometry`] is: this is arithmetic, the arithmetic is
//! where the frame silently comes out wrong, and arithmetic is cheap to test on
//! a machine that has no screen.
//!
//! The frame is five windows because GTK cannot make a covered region
//! click-through on macOS, so nothing is placed over the hole — see
//! [ADR 0011](../../docs/adr/0011-why-the-macos-frame-is-more-than-one-window.md).
//! The hole is not a window; it is the gap the other five leave.

use crate::geometry::AppKitRect;

/// The four sides, in a fixed order so callers can index them meaningfully.
pub const TOP: usize = 0;
pub const BOTTOM: usize = 1;
pub const LEFT: usize = 2;
pub const RIGHT: usize = 3;

/// Where every window of the frame sits, given the hole it should surround.
///
/// All rectangles are AppKit: bottom-left origin, y upward, in points.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Layout {
    /// The chrome, above the hole. ADR 0006's header, as its own window.
    pub header: AppKitRect,
    /// Top, bottom, left, right — index with [`TOP`] and friends.
    pub strips: [AppKitRect; 4],
    /// The gap in the middle. **Not a window.** This is what gets recorded.
    pub hole: AppKitRect,
}

/// Lay a frame out around `hole`.
///
/// `border` is the visible frame thickness and `header_height` the chrome above
/// it. The border runs all the way round, including between the header and the
/// hole, so the hole is bounded by frame on every side rather than by chrome on
/// one of them.
pub fn lay_out(hole: AppKitRect, border: f64, header_height: f64) -> Layout {
    let (x, y, w, h) = (hole.x, hole.y, hole.w, hole.h);
    let b = border;
    Layout {
        header: AppKitRect {
            x: x - b,
            y: y + h + b,
            w: w + 2.0 * b,
            h: header_height,
        },
        strips: [
            // TOP: between the header and the hole.
            AppKitRect {
                x: x - b,
                y: y + h,
                w: w + 2.0 * b,
                h: b,
            },
            // BOTTOM
            AppKitRect {
                x: x - b,
                y: y - b,
                w: w + 2.0 * b,
                h: b,
            },
            // LEFT: spans only the hole's height, so the corners belong to the
            // horizontal strips and no two windows overlap.
            AppKitRect {
                x: x - b,
                y,
                w: b,
                h,
            },
            // RIGHT
            AppKitRect {
                x: x + w,
                y,
                w: b,
                h,
            },
        ],
        hole,
    }
}

impl Layout {
    /// Every window in the frame, header first.
    pub fn windows(&self) -> impl Iterator<Item = &AppKitRect> {
        std::iter::once(&self.header).chain(self.strips.iter())
    }

    /// Does any window overlap the hole?
    ///
    /// The invariant the whole composition exists to preserve. Anything covering
    /// the hole takes clicks, which is precisely what could not be avoided
    /// within a single GTK window.
    pub fn anything_covers_the_hole(&self) -> bool {
        self.windows().any(|r| overlaps(*r, self.hole))
    }
}

fn overlaps(a: AppKitRect, b: AppKitRect) -> bool {
    a.x < b.x + b.w && b.x < a.x + a.w && a.y < b.y + b.h && b.y < a.y + a.h
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

    /// The invariant. If anything ever covers the hole the frame stops being
    /// click-through, and on macOS there is no input shape to rescue it.
    #[test]
    fn nothing_covers_the_hole() {
        let l = lay_out(HOLE, B, H);
        assert!(!l.anything_covers_the_hole());
    }

    /// The control for the test above: a layout that DOES cover the hole must be
    /// reported. Without this, `anything_covers_the_hole` could return `false`
    /// unconditionally and the invariant test would still pass.
    #[test]
    fn a_window_over_the_hole_is_detected() {
        let mut l = lay_out(HOLE, B, H);
        l.strips[TOP] = HOLE;
        assert!(l.anything_covers_the_hole());
    }

    /// The border is continuous: each strip touches the hole's edge exactly,
    /// with no gap for the desktop to show through and no overlap.
    #[test]
    fn the_strips_meet_the_hole_exactly() {
        let l = lay_out(HOLE, B, H);
        assert_eq!(l.strips[BOTTOM].y + l.strips[BOTTOM].h, HOLE.y, "bottom");
        assert_eq!(l.strips[TOP].y, HOLE.y + HOLE.h, "top");
        assert_eq!(l.strips[LEFT].x + l.strips[LEFT].w, HOLE.x, "left");
        assert_eq!(l.strips[RIGHT].x, HOLE.x + HOLE.w, "right");
    }

    /// Corners belong to the horizontal strips, so no two windows overlap. Two
    /// opaque windows overlapping would be invisible; two semi-transparent ones
    /// would show a seam, and the frame would look subtly wrong at each corner.
    #[test]
    fn the_strips_do_not_overlap_each_other() {
        let l = lay_out(HOLE, B, H);
        for i in 0..4 {
            for j in (i + 1)..4 {
                assert!(
                    !overlaps(l.strips[i], l.strips[j]),
                    "strips {i} and {j} overlap"
                );
            }
        }
    }

    /// The header sits above the top strip, not on it, so the border thickness
    /// between chrome and hole is the same as everywhere else.
    #[test]
    fn the_header_sits_above_the_border_not_on_it() {
        let l = lay_out(HOLE, B, H);
        assert_eq!(l.header.y, l.strips[TOP].y + l.strips[TOP].h);
        assert!(!overlaps(l.header, l.strips[TOP]));
        assert!(!overlaps(l.header, l.hole));
    }

    /// The frame's outer width is the hole plus a border on each side, and the
    /// header spans exactly that. A header narrower or wider than the frame is
    /// the kind of thing that looks deliberate and is not.
    #[test]
    fn the_header_spans_the_full_frame_width() {
        let l = lay_out(HOLE, B, H);
        assert_eq!(l.header.w, HOLE.w + 2.0 * B);
        assert_eq!(l.header.x, l.strips[TOP].x);
        assert_eq!(l.header.w, l.strips[TOP].w);
    }

    /// A zero border still has to produce a coherent layout rather than strips
    /// of negative size, because the setting is reachable from CSS.
    #[test]
    fn a_zero_border_degenerates_without_going_negative() {
        let l = lay_out(HOLE, 0.0, H);
        assert!(l.strips.iter().all(|s| s.w >= 0.0 && s.h >= 0.0));
        assert!(!l.anything_covers_the_hole());
    }
}
