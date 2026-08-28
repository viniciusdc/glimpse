//! The conversion from AppKit's coordinate space into the one the capture
//! rectangle is defined in.
//!
//! This module is deliberately **free of AppKit**, so it compiles and its tests
//! run on every platform. That is not tidiness: the flip below is the single
//! most likely thing in the macOS frontend to be silently wrong, and a wrong
//! flip produces a rectangle of exactly the right size on entirely the wrong
//! pixels — which looks completely plausible in a log line. It is the failure
//! [ADR 0000] was written about, and it should be guarded by tests that run
//! everywhere rather than only where it can be observed.
//!
//! Three coordinate spaces meet here, and it is worth naming them because two of
//! them differ only in ways that produce plausible wrong answers:
//!
//! | space | origin | unit |
//! |---|---|---|
//! | AppKit (`NSWindow.frame`) | bottom-left of the primary screen | points |
//! | CoreGraphics global display | top-left of the primary screen | points |
//! | [`ScreenPixelRect`] and ffmpeg `crop` | top-left | **pixels** |
//!
//! [ADR 0000]: ../../docs/adr/0000-x11-framing-window-spike.md

use glimpse_core::geometry::ScreenPixelRect;

/// A rectangle as AppKit reports it: bottom-left origin, y upward, in points.
///
/// A distinct type from [`ScreenPixelRect`] on purpose. They have the same four
/// fields and mean different things, so the only way to get from one to the
/// other should be a function that has to be called deliberately.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct AppKitRect {
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub h: f64,
}

/// Convert an AppKit rectangle into the capture rectangle's space.
///
/// Two things happen, and both are easy to get wrong in ways that still produce
/// a believable rectangle:
///
/// * **The origin flips.** AppKit measures y upward from the bottom of the
///   primary screen; the capture rect measures downward from the top. So the top
///   edge is `screen_height - (y + h)`, *not* `screen_height - y` — using the
///   latter gives a rect displaced by exactly its own height, which on a small
///   frame looks almost right.
/// * **Points become pixels**, by the integer backing scale factor and nothing
///   else. Never a figure derived from a monitor's reported physical size: the
///   monitor this project was written against reports 1mm x 1mm.
///
/// `screen_height_pt` is the height of the screen the rectangle's coordinates are
/// relative to, which on AppKit is always the **primary** screen regardless of
/// which display the window is on.
pub fn to_screen_pixels(r: AppKitRect, screen_height_pt: f64, scale: f64) -> ScreenPixelRect {
    let top_pt = screen_height_pt - (r.y + r.h);
    ScreenPixelRect {
        x: (r.x * scale).round() as i32,
        y: (top_pt * scale).round() as i32,
        w: (r.w * scale).round() as i32,
        h: (r.h * scale).round() as i32,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The numbers from the spike that established check 3, kept because they
    /// were verified against a real capture with a positive control: a 640x400pt
    /// hole at (240,200) on a 1512x982pt screen at 2x produced
    /// `crop=1280:800:480:764`, and the grab landed exactly on the hole.
    #[test]
    fn it_reproduces_the_verified_spike_conversion() {
        let got = to_screen_pixels(
            AppKitRect {
                x: 240.0,
                y: 200.0,
                w: 640.0,
                h: 400.0,
            },
            982.0,
            2.0,
        );
        assert_eq!(
            got,
            ScreenPixelRect {
                x: 480,
                y: 764,
                w: 1280,
                h: 800
            }
        );
    }

    /// The specific wrong flip: `screen_height - y` instead of
    /// `screen_height - (y + h)`. It displaces the rect by exactly its own
    /// height, which is why it survives a glance.
    #[test]
    fn the_flip_measures_the_top_edge_not_the_bottom() {
        let r = AppKitRect {
            x: 0.0,
            y: 100.0,
            w: 10.0,
            h: 300.0,
        };
        let got = to_screen_pixels(r, 1000.0, 1.0);
        assert_eq!(got.y, 600, "1000 - (100 + 300)");
        assert_ne!(got.y, 900, "1000 - 100 would be the bottom edge");
    }

    /// A window flush against the top of the screen must land at y = 0. Off-by-a
    /// -height errors are invisible in the middle of a screen and obvious here.
    #[test]
    fn a_rect_at_the_top_of_the_screen_lands_at_zero() {
        let got = to_screen_pixels(
            AppKitRect {
                x: 0.0,
                y: 700.0,
                w: 100.0,
                h: 300.0,
            },
            1000.0,
            2.0,
        );
        assert_eq!(got.y, 0);
    }

    /// Scale multiplies position as well as size. Applying it to only one is a
    /// classic Retina bug and yields a rect that is right on a 1x display.
    #[test]
    fn scale_applies_to_the_origin_as_well_as_the_size() {
        let r = AppKitRect {
            x: 50.0,
            y: 0.0,
            w: 100.0,
            h: 100.0,
        };
        let one = to_screen_pixels(r, 1000.0, 1.0);
        let two = to_screen_pixels(r, 1000.0, 2.0);
        assert_eq!((one.x, one.w), (50, 100));
        assert_eq!((two.x, two.w), (100, 200));
        assert_eq!(two.y, one.y * 2, "the flipped origin scales too");
    }

    /// A 1x display must be a no-op in the scale dimension, so the conversion
    /// cannot quietly depend on being Retina.
    #[test]
    fn a_one_x_display_changes_only_the_origin() {
        let got = to_screen_pixels(
            AppKitRect {
                x: 12.0,
                y: 34.0,
                w: 56.0,
                h: 78.0,
            },
            500.0,
            1.0,
        );
        assert_eq!(
            got,
            ScreenPixelRect {
                x: 12,
                y: 500 - 34 - 78,
                w: 56,
                h: 78
            }
        );
    }
}
