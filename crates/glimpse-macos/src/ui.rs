//! Build the framing window: the shared chrome, in macOS's window model.
//!
//! The counterpart of `glimpse_x11::ui::build`, and now nearly identical to it,
//! because the window models finally agree. Both are one window with the hole
//! inside it ([ADR 0017](../../docs/adr/0017-click-through-is-a-mode-not-a-window.md)).
//! What still differs is how the hole passes clicks: X11 punches an input
//! region, macOS turns the whole window click-through for as long as the user
//! needs to reach what is behind it.

use anyhow::anyhow;
use gtk::prelude::*;
use gtk4 as gtk;
use objc2_foundation::MainThreadMarker;
use std::rc::Rc;

use glimpse_ui::{Chrome, Hole, PlatformHooks};

use crate::grab::AvfCapture;
use crate::stop::StopPaths;
use crate::window::{capture_rect_of, set_passthrough, window_nswindow};

/// The chrome, assembled into one window.
///
/// `stop` is filled in by the caller *after* this returns, because the stop
/// paths need something to stop and the chrome has to exist first. The hook
/// reads it on every refresh, so it picks up whatever installed itself.
pub fn build(app: &gtk::Application, stop: StopPaths) -> Rc<Chrome> {
    Chrome::new(
        app,
        // Same as X11 now. The hole is a widget in this window, and clicks reach
        // what is behind it because the window stops taking any.
        Hole::InChrome,
        move |window, hole| {
            let (w, h) = (window.clone(), hole.clone());
            let (dw, dh) = (w.clone(), h.clone());
            let pw = w.clone();
            PlatformHooks {
                capture_rect: {
                    let (w, h) = (w.clone(), h.clone());
                    Box::new(move || {
                        // Every call arrives from a GTK callback, so the marker
                        // is always available. `ok_or_else` rather than
                        // `expect`: the chrome shows this as a status message,
                        // and a panic in a callback takes the app down.
                        let mtm = MainThreadMarker::new()
                            .ok_or_else(|| anyhow!("capture_rect called off the main thread"))?;
                        capture_rect_of(w.upcast_ref(), &h, mtm)
                    })
                },

                // Discovered per call rather than cached. `-list_devices` is how
                // the screen index is found, and it sits BEHIND the cameras, so
                // the index is not a constant and can move when a camera is
                // plugged in mid-session.
                grab: Box::new(|req| Ok(AvfCapture::discover()?.grab(req))),

                // Nothing to re-punch. X11 recomputes its input region on every
                // geometry change because its hole is a region of a window that
                // takes clicks everywhere else. macOS has no shape to maintain:
                // the window either takes clicks or it does not, and which one
                // is decided by the session, not by the layout.
                geometry_settled: Box::new(|| {}),

                // The whole point of ADR 0017: the window stops taking clicks
                // rather than the hole being carved out of it.
                offers_passthrough: true,

                set_passthrough: Box::new(move |on| {
                    match window_nswindow(pw.upcast_ref()) {
                        Ok(ns) => set_passthrough(&ns, on),
                        // Saying nothing here would leave the window opaque to
                        // clicks during a recording, which looks like a frozen
                        // application and has no other symptom.
                        Err(e) => eprintln!(
                            "glimpse: could not {} click-through: {e:#}",
                            if on { "enable" } else { "disable" }
                        ),
                    }
                }),

                // Whatever actually installed itself, and nothing otherwise.
                // Read on every refresh rather than captured once, so a stop
                // path registered after the window came up is picked up — and
                // so one that failed to register is never claimed.
                stop_hint: Box::new(move || stop.hint()),

                diagnostics: Box::new(move || diagnostics(&dw, &dh)),

                // avfoundation ignores `-capture_cursor`, measured, and in the
                // OFF direction: the pointer is never drawn. ADR 0012 decides
                // that a setting a backend cannot honour is not offered on that
                // platform, so the chrome hides the switch rather than showing a
                // dead one.
                honours_pointer_capture: false,
            }
        },
        |window, parts| {
            // One window, in the order ADR 0006 designed, exactly as X11 leaves
            // it. No re-parenting into other windows, no strips, no seams.
            //
            // No resize edges yet: issue #10. One window is their precondition
            // and GDK already gives this one the Resizable style mask, but
            // whether the Quartz backend forwards `begin_resize` is unmeasured.
            window.set_child(Some(parts.shell));
        },
    )
}

/// The macOS half of the self-test report.
///
/// X11 prints an X window id, the shape bands read back from the server, and an
/// `xwininfo` cross-check. There is no shape to read here — the window takes
/// clicks or it does not — so what is worth reporting instead is the rectangle
/// itself and the fact that it was derived rather than remembered.
fn diagnostics(window: &gtk::ApplicationWindow, hole: &gtk::Box) -> String {
    let mut out = String::from("window model : one window, click-through is a mode (ADR 0017)\n");
    match MainThreadMarker::new() {
        Some(mtm) => match capture_rect_of(window.upcast_ref(), hole, mtm) {
            Ok(r) => out.push_str(&format!(
                "capture rect : {}x{} at {},{} (device pixels, from compute_bounds)\n",
                r.w, r.h, r.x, r.y
            )),
            Err(e) => out.push_str(&format!("capture rect : UNAVAILABLE — {e:#}\n")),
        },
        None => out.push_str("capture rect : UNAVAILABLE — not on the main thread\n"),
    }
    out
}
