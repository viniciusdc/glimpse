//! The X11 window model, and the platform facts the chrome needs.
//!
//! The chrome itself lives in `glimpse-ui` — the header, the status bar and the
//! controller are identical on both platforms, which is
//! [ADR 0014](../../../docs/adr/0014-the-chrome-is-shared-the-window-model-is-not.md).
//! What is left here is genuinely X11:
//!
//! * **one window.** The header, the frame and the status bar are stacked in a
//!   single `ApplicationWindow`, and the hole is a region punched out of its
//!   input shape. macOS cannot do that, which is why it has two windows
//!   ([ADR 0015](../../../docs/adr/0015-the-frame-is-two-windows.md)).
//! * **the input region**, re-punched whenever the geometry settles.
//! * **resize edges**, because an undecorated GTK window has none.
//! * **the self-test's X11 half**: an xid, an `xwininfo` cross-check, and the
//!   shape read back from the server.

use anyhow::{anyhow, Result};
use gtk::cairo;
use gtk::prelude::*;
use gtk4 as gtk;
use std::cell::Cell;
use std::rc::Rc;

use glimpse_ui::{Chrome, PlatformHooks};

use crate::geometry::capture_rect;
use crate::grab::X11Capture;
use crate::x11probe::{self, shape_covers, X11Probe};

/// Build the framing window: the shared chrome, in X11's single-window model.
///
/// The two closures are the whole platform boundary. `make_hooks` answers the
/// four questions the controller asks (ADR 0014), and `assemble` decides what
/// the window actually contains — here, everything in one window with the hole
/// inside it.
pub fn build(app: &gtk::Application, probe: Rc<X11Probe>) -> Rc<Chrome> {
    Chrome::new(
        app,
        move |window, hole| {
            let (w, h, pr) = (window.clone(), hole.clone(), probe.clone());
            let (dw, dh, dpr) = (w.clone(), h.clone(), pr.clone());
            // The memo lives in the closure, so "nothing moved, skip the round
            // trip" survives across calls without a field for it.
            let last = Rc::new(Cell::new((0, 0, 0, 0)));
            let (sw, sh) = (w.clone(), h.clone());
            PlatformHooks {
                capture_rect: {
                    let (w, h, pr) = (w.clone(), h.clone(), pr.clone());
                    Box::new(move || capture_rect(&w, &h, &pr))
                },
                // `from_env` rather than a `:0` fallback: the rectangle was
                // computed against whatever display the window is on, and
                // guessing a different one here would grab the wrong screen
                // while reporting success.
                grab: Box::new(|req| Ok(X11Capture::from_env()?.grab(req))),
                geometry_settled: Box::new(move || sync_input_region(&sw, &sh, &last)),
                diagnostics: Box::new(move || x11_diagnostics(&dw, &dh, &dpr)),
            }
        },
        |window, shell| {
            // Resize edges sit above everything, at the window's rim only.
            let overlay = gtk::Overlay::new();
            overlay.set_child(Some(shell));
            if !glimpse_ui::system_decorations() {
                install_resize_edges(window, &overlay);
            }
            window.set_child(Some(&overlay));
        },
    )
}

/// Recompute the hole and re-punch, skipping the server round trip when nothing
/// moved.
fn sync_input_region(
    win: &gtk::ApplicationWindow,
    hole: &gtk::Box,
    last: &Cell<(i32, i32, i32, i32)>,
) {
    let Some(b) = hole.compute_bounds(win) else {
        return;
    };
    let cur = (
        b.x() as i32,
        b.y() as i32,
        b.width() as i32,
        b.height() as i32,
    );
    if cur == last.get() || cur.2 <= 0 || cur.3 <= 0 {
        return;
    }
    last.set(cur);
    if let Err(e) = punch_input_hole(win.upcast_ref(), cur) {
        eprintln!("glimpse: input region not applied: {e:#}");
    }
}

/// Input region = the whole window minus the hole, so clicks in the middle reach
/// whatever is underneath.
fn punch_input_hole(window: &gtk::Window, hole: (i32, i32, i32, i32)) -> Result<()> {
    let surface = window.surface().ok_or_else(|| anyhow!("no surface"))?;
    if !surface.display().supports_input_shapes() {
        return Err(anyhow!("display does not support input shapes"));
    }

    // The input region is in SURFACE coordinates, and `hole` arrives in widget
    // coordinates. `capture_rect` applies this same transform; if only one of the
    // two applies it, the punched hole drifts from the visible one by the client-
    // side-decoration margin. Rounding matches `geometry.rs` for the same reason.
    let (tx, ty) = window.surface_transform();
    let hx = (hole.0 as f64 + tx).round() as i32;
    let hy = (hole.1 as f64 + ty).round() as i32;

    let (w, h) = (surface.width(), surface.height());
    if w <= 0 || h <= 0 {
        return Err(anyhow!("surface not sized yet"));
    }

    let region = cairo::Region::create_rectangle(&cairo::RectangleInt::new(0, 0, w, h));
    region.subtract_rectangle(&cairo::RectangleInt::new(hx, hy, hole.2, hole.3))?;
    surface.set_input_region(Some(&region));
    Ok(())
}

/// The X11 half of the self-test report: an X window id, an `xwininfo`
/// cross-check, and the input shape read back from the server.
///
/// None of this exists on macOS — there is no shape to read, because nothing over
/// the hole takes clicks in the first place (ADR 0015). Hence whole lines of text
/// rather than a structure: the two platforms have nothing to say to each other
/// here, and a shared vocabulary would be one neither fills honestly.
fn x11_diagnostics(window: &gtk::ApplicationWindow, hole: &gtk::Box, probe: &X11Probe) -> String {
    let xid = match x11probe::window_xid(window.upcast_ref()) {
        Ok(x) => x,
        Err(e) => return format!("xid          : FAILED: {e:#}\n"),
    };

    // Counting bands proves nothing: a window with no shape set can still report
    // a band covering everything, and a misplaced hole reports bands too. Check
    // the semantics — the hole must NOT take clicks, the border MUST.
    let shape = match (probe.input_shape(xid), hole.compute_bounds(window)) {
        (Ok(bands), Some(b)) => {
            let (hx, hy) = (b.x() as i32, b.y() as i32);
            let (hw, hh) = (b.width() as i32, b.height() as i32);
            let centre = shape_covers(&bands, hx + hw / 2, hy + hh / 2);
            let border = shape_covers(&bands, hx + hw / 2, hy - 2);
            let listed = bands
                .iter()
                .map(|(x, y, w, h)| format!("{w}x{h}+{x}+{y}"))
                .collect::<Vec<_>>()
                .join(" ");
            let verdict = match (centre, border) {
                (false, true) => "PASS — hole is click-through, border takes clicks",
                (true, _) => "FAIL — the hole still takes clicks; region misplaced or absent",
                (false, false) => "FAIL — the border takes no clicks either; region too large",
            };
            format!(
                "{verdict}\n                 {} band(s): {listed}",
                bands.len()
            )
        }
        (Err(e), _) => format!("shape query failed: {e}"),
        (_, None) => "could not compute hole bounds".to_string(),
    };

    let xwin = crate::geometry::verify_against_xwininfo(xid)
        .map(|(x, y)| format!("window origin {x},{y}"))
        .unwrap_or_else(|| "unavailable".into());

    // Trailing newline, and no leading one: run_selftest splices this between
    // two lines it owns, so the block is responsible for terminating itself.
    format!(
        "xid          : 0x{xid:x}\n\
         xwininfo     : {xwin}\n\
         input shape  : {shape}\n"
    )
}

/// Thickness of the invisible grab strips along each edge, in logical pixels.
/// Wide enough to hit without aiming, narrow enough not to eat the frame border.
const RESIZE_GRIP: i32 = 8;
/// Corner grips are square and take priority over the edges they overlap.
const RESIZE_CORNER: i32 = 16;

/// Give the window its own resize edges.
///
/// GTK provides these for decorated windows and — measured on gala — for neither
/// an undecorated window nor one with a replaced titlebar. Since the frame's size
/// *is* the capture region, resizing is not a nicety here, so Glimpse draws the
/// grips itself and asks the compositor to take over the drag through
/// `Toplevel::begin_resize`.
fn install_resize_edges(window: &gtk::ApplicationWindow, overlay: &gtk::Overlay) {
    use gtk::gdk::SurfaceEdge;
    use gtk::{Align, Box as GtkBox, Orientation};

    // (edge, halign, valign, width, height, cursor)
    let grips: [(SurfaceEdge, Align, Align, i32, i32, &str); 8] = [
        (
            SurfaceEdge::North,
            Align::Fill,
            Align::Start,
            -1,
            RESIZE_GRIP,
            "n-resize",
        ),
        (
            SurfaceEdge::South,
            Align::Fill,
            Align::End,
            -1,
            RESIZE_GRIP,
            "s-resize",
        ),
        (
            SurfaceEdge::West,
            Align::Start,
            Align::Fill,
            RESIZE_GRIP,
            -1,
            "w-resize",
        ),
        (
            SurfaceEdge::East,
            Align::End,
            Align::Fill,
            RESIZE_GRIP,
            -1,
            "e-resize",
        ),
        // Corners last so they stack above the edges they overlap.
        (
            SurfaceEdge::NorthWest,
            Align::Start,
            Align::Start,
            RESIZE_CORNER,
            RESIZE_CORNER,
            "nw-resize",
        ),
        (
            SurfaceEdge::NorthEast,
            Align::End,
            Align::Start,
            RESIZE_CORNER,
            RESIZE_CORNER,
            "ne-resize",
        ),
        (
            SurfaceEdge::SouthWest,
            Align::Start,
            Align::End,
            RESIZE_CORNER,
            RESIZE_CORNER,
            "sw-resize",
        ),
        (
            SurfaceEdge::SouthEast,
            Align::End,
            Align::End,
            RESIZE_CORNER,
            RESIZE_CORNER,
            "se-resize",
        ),
    ];

    for (edge, halign, valign, w, h, cursor) in grips {
        let grip = GtkBox::new(Orientation::Horizontal, 0);
        grip.set_halign(halign);
        grip.set_valign(valign);
        if w > 0 {
            grip.set_size_request(w, -1);
        }
        if h > 0 {
            grip.set_size_request(grip.width_request(), h);
        }
        grip.set_cursor_from_name(Some(cursor));
        if std::env::var("GLIMPSE_DEBUG_GRIPS").is_ok() {
            grip.add_css_class("glimpse-grip-debug");
        }

        let gesture = gtk::GestureClick::new();
        gesture.set_button(gtk::gdk::BUTTON_PRIMARY);
        let win = window.clone();
        let grip_ref = grip.clone();
        gesture.connect_pressed(move |g, _, x, y| {
            let Some(surface) = win.surface() else {
                return;
            };
            let Ok(toplevel) = surface.downcast::<gtk::gdk::Toplevel>() else {
                return;
            };
            // begin_resize wants surface coordinates; the gesture reports widget
            // ones. Same chain as `geometry.rs`, for the same reason.
            let point = gtk::graphene::Point::new(x as f32, y as f32);
            let Some(in_window) = grip_ref.compute_point(&win, &point) else {
                return;
            };
            let (tx, ty) = win.surface_transform();
            toplevel.begin_resize(
                edge,
                g.device().as_ref(),
                g.current_button() as i32,
                in_window.x() as f64 + tx,
                in_window.y() as f64 + ty,
                g.current_event_time(),
            );
            // Hand the sequence back: GTK's implicit grab would otherwise keep
            // holding the pointer and the compositor's resize grab never starts.
            g.set_state(gtk::EventSequenceState::Denied);
        });
        grip.add_controller(gesture);
        overlay.add_overlay(&grip);
    }
}
