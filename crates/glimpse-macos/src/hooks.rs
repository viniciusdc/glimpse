//! The four platform facts the shared chrome needs, answered for macOS.
//!
//! The counterpart of the block at the top of `glimpse_x11::ui::build`. Both are
//! short, which is the point: `glimpse-ui` holds 1757 lines of chrome and asks
//! each platform for four things ([ADR 0014](../../docs/adr/0014-the-chrome-is-shared-the-window-model-is-not.md)).

use anyhow::anyhow;
use glimpse_ui::PlatformHooks;
use objc2_foundation::MainThreadMarker;
use std::rc::Rc;

use crate::frame::Frame;
use crate::grab::AvfCapture;

/// Build the hooks for a frame that already exists.
///
/// The frame has to exist first, because `capture_rect` asks it what it would
/// record. That is why `Frame::new` no longer builds the chrome window — see the
/// note on [`Frame`].
pub fn for_frame(frame: Rc<Frame>) -> PlatformHooks {
    let rect_frame = frame.clone();
    let diag_frame = frame;

    PlatformHooks {
        capture_rect: Box::new(move || {
            // GTK runs its main loop on the main thread and every call here
            // arrives from a widget callback, so the marker is always available.
            // `ok_or_else` rather than `expect`: the chrome shows this as a
            // status message, and a panic in a callback takes the app down.
            let mtm = MainThreadMarker::new()
                .ok_or_else(|| anyhow!("capture_rect called off the main thread"))?;
            rect_frame.capture_rect(mtm)
        }),

        // Discovered per call rather than cached. `-list_devices` is how the
        // screen index is found, and it sits BEHIND the cameras, so the index is
        // not a constant and can move when a camera is plugged in mid-session.
        grab: Box::new(|req| Ok(AvfCapture::discover()?.grab(req))),

        // Nothing. The frame window set `ignoresMouseEvents` once, when it was
        // created, and takes no clicks anywhere ever after
        // ([ADR 0015](../../docs/adr/0015-the-frame-is-two-windows.md)).
        //
        // X11 must re-punch its input region on every geometry change because
        // its hole is a region of a window that takes clicks everywhere else.
        // macOS has no equivalent to re-do, and an empty implementation is the
        // honest answer rather than a stub waiting to be filled in.
        geometry_settled: Box::new(|| {}),

        diagnostics: Box::new(move || {
            // No xid, no input shape: there is no shape to read, because nothing
            // over the hole takes clicks in the first place. What IS worth
            // reporting is the check X11 cannot make — that the windows are
            // where the layout asked, read back from the window server rather
            // than from our own arithmetic.
            let l = diag_frame.layout();
            let agreement = if l.hole_from_frame(crate::frame::BORDER) == l.hole {
                "PASS — the recorded region and the drawn frame agree"
            } else {
                "FAIL — the hole and the frame inset by its border disagree"
            };
            format!(
                "window model : two windows, frame is click-through (ADR 0015)\n\
                 hole/frame   : {agreement}\n"
            )
        }),
    }
}
