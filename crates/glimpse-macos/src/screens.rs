//! Is this rectangle one Glimpse can actually record?
//!
//! Two things in the conversion to a `ScreenPixelRect` are per-screen, and only
//! one of them is handled correctly by using the primary display.
//!
//! The **height** is right: AppKit coordinates are global and relative to the
//! primary screen whatever display a window sits on, so flipping against its
//! height works anywhere. The **scale factor** is not — it belongs to the screen
//! the frame is on, and a 2x laptop beside a 1x external produces a rectangle
//! exactly twice the size it should be.
//!
//! And [`crate::grab`] captures one device. A frame on the second display gets
//! cropped out of the *first* display's capture, at coordinates that mean
//! something there too, so the result is a recording of the wrong screen at
//! plausible dimensions with no error anywhere.
//!
//! [ADR 0018](../../docs/adr/0018-multi-display-is-refused-not-guessed.md)
//! decides to refuse that rather than guess at it. A rectangle that is the right
//! size on the wrong pixels is the failure
//! [ADR 0000](../../docs/adr/0000-x11-framing-window-spike.md) exists to record.

use anyhow::{anyhow, Result};
use objc2_app_kit::NSScreen;
use objc2_foundation::MainThreadMarker;

use crate::geometry::AppKitRect;

/// Refuse unless `hole` lies entirely on the primary display.
///
/// `Ok(())` on any single-display machine, which is the common case and pays
/// nothing: with one screen the condition is trivially true.
///
/// Called when Record is pressed, before a session exists, so a refusal leaves
/// no workspace, no ffmpeg child and no half-started recording.
pub fn ensure_capturable(hole: AppKitRect, mtm: MainThreadMarker) -> Result<()> {
    let screens = NSScreen::screens(mtm);
    let count = screens.len();
    if count <= 1 {
        return Ok(());
    }

    let primary = screens.iter().next().ok_or_else(|| anyhow!("no screens"))?;
    let p = primary.frame();

    // Containment, not intersection. A frame straddling the edge has a half that
    // is capturable and a half that is not, and recording the pair would produce
    // a rectangle whose far side is not what the user framed. Refusing the whole
    // thing is the answer that cannot be subtly wrong.
    let inside = hole.x >= p.origin.x
        && hole.y >= p.origin.y
        && hole.x + hole.w <= p.origin.x + p.size.width
        && hole.y + hole.h <= p.origin.y + p.size.height;

    if inside {
        return Ok(());
    }

    Err(anyhow!(
        "The frame is not on the main display, and recording across displays is \
         not supported yet.\n\
         Glimpse captures one screen and converts using that screen's scale \
         factor, so a frame on another display would record the main display's \
         pixels instead — the wrong picture at the right size, which is worse \
         than refusing.\n\
         Move the frame back onto the main display, or follow \
         https://github.com/viniciusdc/glimpse/issues/14"
    ))
}

/// How many displays, for the self-test report.
///
/// Worth reporting unasked: a bug from a two-display machine should say so
/// without anyone having to think to ask, because almost every geometry
/// surprise on macOS starts there.
pub fn describe(mtm: MainThreadMarker) -> String {
    let screens = NSScreen::screens(mtm);
    let n = screens.len();
    let scales: Vec<String> = screens
        .iter()
        .map(|s| format!("{}x", s.backingScaleFactor()))
        .collect();
    format!(
        "displays     : {n} ({}){}\n",
        scales.join(", "),
        if n > 1 {
            " — recording is limited to the main one (ADR 0018)"
        } else {
            ""
        }
    )
}

#[cfg(test)]
mod tests {
    // The refusal needs two displays and cannot be exercised here, which ADR
    // 0018 records as a cost rather than hiding. What IS worth pinning is the
    // half that can go wrong on the machines everyone has: a check that refused
    // single-display users would be worse than the bug it prevents.
    //
    // `ensure_capturable` needs a MainThreadMarker and `NSScreen`, so the
    // single-screen short circuit is asserted through the app rather than here —
    // `make journeys-macos` records five times on this machine, and every one of
    // them goes through it.
    //
    // This module therefore has no unit tests on purpose, and says so rather
    // than carrying one that asserts nothing.
}
