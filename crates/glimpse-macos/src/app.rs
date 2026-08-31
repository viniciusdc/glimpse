//! Starting the macOS frontend.
//!
//! The counterpart of `glimpse_x11::app`, and nearly as small, because the
//! chrome it starts is the same one X11 runs — the header, the status bar and
//! the controller all live in `glimpse-ui`
//! ([ADR 0014](../../docs/adr/0014-the-chrome-is-shared-the-window-model-is-not.md)).
//!
//! What is assembled here is the window model, and only that: a chrome window
//! holding the shared widgets, and a second window that draws the border and
//! takes no clicks
//! ([ADR 0015](../../docs/adr/0015-the-frame-is-two-windows.md)).
//!
//! Order matters, and it is the reason `Frame` no longer builds the chrome
//! window. The chrome's `capture_rect` hook asks the frame what it would record,
//! so the frame must exist before the chrome is built; the frame must then be
//! attached to the chrome window, which does not exist until after that. Frame
//! first, chrome second, attach third.

use gtk::glib;
use gtk::prelude::*;
use gtk4 as gtk;
use objc2_foundation::MainThreadMarker;
use std::cell::RefCell;
use std::process::ExitCode;
use std::rc::Rc;

use glimpse_ui::{Chrome, Hole};

use crate::frame::Frame;
use crate::geometry::AppKitRect;

/// Where the frame appears before anything can move it.
///
/// AppKit coordinates, so y counts up from the bottom of the primary screen.
const INITIAL_HOLE: AppKitRect = AppKitRect {
    x: 400.0,
    y: 300.0,
    w: 640.0,
    h: 400.0,
};

/// Run Glimpse on macOS.
///
/// Returns a process exit code rather than `glib::ExitCode` so the binary crate
/// never has to name GTK.
pub fn run() -> ExitCode {
    let app = gtk::Application::builder()
        .application_id("com.vinicius.glimpse")
        .build();

    // Held for the lifetime of the application rather than dropped at the end of
    // `activate`. Dropping these drops their GTK windows, and the frame would
    // vanish the instant it appeared.
    #[allow(clippy::type_complexity)]
    let held: Rc<RefCell<Option<(Rc<Frame>, Rc<Chrome>)>>> = Rc::new(RefCell::new(None));
    let failed = Rc::new(RefCell::new(false));

    let held_c = held.clone();
    let failed_c = failed.clone();
    app.connect_activate(move |app| {
        // Frame first: the chrome's capture_rect hook needs something to ask.
        let frame = Rc::new(Frame::new(app, INITIAL_HOLE));

        // The window that sits BELOW the frame. Built here so `assemble` can put
        // the status bar and the sheet into it (ADR 0016). Undecorated and
        // unresizable like the frame: GTK4 has no positioning API, so AppKit
        // places it.
        let status_win = gtk::Window::builder()
            .application(app)
            .decorated(false)
            .resizable(false)
            .default_width(frame.layout().status.w as i32)
            .build();
        status_win.add_css_class("glimpse");
        status_win.add_css_class("glimpse-chrome");

        // Then the chrome — the same widgets and the same controller X11 builds.
        let chrome = Chrome::new(
            app,
            // The capture region is the OTHER window, so the shell must not
            // reserve space for a hole that is not in it.
            Hole::Elsewhere,
            {
                let frame = frame.clone();
                move |_window, _hole| crate::hooks::for_frame(frame)
            },
            {
                let status_win = status_win.clone();
                move |window, parts| {
                    // ADR 0016: header and rule above the frame, status and
                    // sheet below it, which is where X11 puts them. The pieces
                    // are the chrome's; only their distribution is ours.
                    //
                    // `remove` first, because a GTK widget has one parent and
                    // they arrive already packed into the shell.
                    parts.shell.remove(parts.status);
                    parts.shell.remove(parts.sheet);

                    let below = gtk::Box::new(gtk::Orientation::Vertical, 0);
                    below.add_css_class("glimpse-shell");
                    below.append(parts.status);
                    below.append(parts.sheet);
                    status_win.set_child(Some(&below));

                    // No Overlay and no resize edges: the frame takes no clicks,
                    // so there is nothing to grab at its rim. Resize has to come
                    // from the chrome and is not designed yet — issue #10.
                    window.set_child(Some(parts.shell));
                }
            },
        );
        // Width from the layout so the chrome and the frame line up; height -1
        // so GTK uses the widgets' natural height rather than the builder's
        // 760x520 default, which is sized for X11's single window and includes
        // room for a hole this window does not contain.
        // The chrome window has no hole in it, so it must be opaque. The shared
        // stylesheet makes `window.glimpse` transparent, which is correct for
        // X11 and wrong here.
        chrome.window.add_css_class("glimpse-chrome");
        chrome
            .window
            .set_default_size(frame.layout().chrome.w as i32, -1);
        chrome.window.present();
        status_win.present();

        let held = held_c.clone();
        let failed = failed_c.clone();
        let app = app.clone();

        // GTK maps windows asynchronously, so there is no NSWindow to position
        // until the main loop has turned. `attach_to` reports that rather than
        // silently doing nothing, which is why its result is checked.
        glib::timeout_add_local_once(std::time::Duration::from_millis(200), move || {
            if let Err(e) = attach_and_report(&frame, &chrome, &status_win) {
                eprintln!("glimpse: {e:#}");
                *failed.borrow_mut() = true;
                app.quit();
                return;
            }
            // Read the geometry back a SECOND time, a beat later. `place` sets
            // the NSWindow frame, but GTK lays out its content afterwards and
            // will resize the window to fit — so a readback taken in the same
            // turn reports what we asked for and not what we ended up with.
            // That is the same trap ADR 0015 records for ignoresMouseEvents.
            let (f2, c2, s2) = (frame.clone(), chrome.clone(), status_win.clone());
            glib::timeout_add_local_once(std::time::Duration::from_millis(400), move || {
                // Now that GTK has sized it, re-anchor the status window's top
                // edge to the frame's bottom. Its height is not ours to predict.
                if let Err(e) = f2.settle_status(&s2) {
                    eprintln!("glimpse: {e:#}");
                }
                // The seam that matters: the status window's TOP edge must sit
                // exactly on the frame's BOTTOM edge. A gap shows the desktop
                // between them; an overlap covers the recording area.
                if let Ok(st) = f2.status_frame(&s2) {
                    let l = f2.layout();
                    let top = st.y + st.h;
                    println!(
                        "glimpse: status {}x{} at {},{} — top {} vs frame bottom {} {}",
                        st.w as i64,
                        st.h as i64,
                        st.x as i64,
                        st.y as i64,
                        top as i64,
                        l.frame.y as i64,
                        if (top - l.frame.y).abs() < 0.5 {
                            "FLUSH"
                        } else {
                            "<-- SEAM"
                        },
                    );
                }
                if let Ok(a) = f2.actual_frames(c2.window.upcast_ref()) {
                    let l = f2.layout();
                    println!(
                        "glimpse: settled chrome {}x{} at {},{} (layout said {}x{})",
                        a[0].w as i64,
                        a[0].h as i64,
                        a[0].x as i64,
                        a[0].y as i64,
                        l.chrome.w as i64,
                        l.chrome.h as i64,
                    );
                }
            });

            *held.borrow_mut() = Some((frame, chrome));
        });
    });

    let code = app.run();
    if *failed.borrow() {
        return ExitCode::FAILURE;
    }
    ExitCode::from(glib::ExitCode::get(&code))
}

fn attach_and_report(
    frame: &Frame,
    chrome: &Chrome,
    status_win: &gtk::Window,
) -> anyhow::Result<()> {
    frame.attach_to(chrome.window.upcast_ref(), status_win)?;

    let mtm = MainThreadMarker::new().expect("GTK runs on the main thread");
    let rect = frame.capture_rect(mtm)?;
    println!(
        "glimpse: frame up. capture rect {}x{} at {},{} (device pixels, top-left origin)",
        rect.w, rect.h, rect.x, rect.y
    );

    // The frame window covers the hole deliberately now (ADR 0015) and is
    // click-through because it takes no mouse events at all. What must still
    // hold is that the two descriptions of the recorded region agree: the hole
    // the caller asked for, and the frame window inset by its border. If those
    // drift, the user frames one rectangle and records another.
    // Asked-for versus actually-got, read back from the window server. The
    // layout is arithmetic; this is what the window manager did with it, and the
    // two are not the same claim (ADR 0000).
    if let Ok(actual) = frame.actual_frames(chrome.window.upcast_ref()) {
        let l = frame.layout();
        for (name, want, got) in [
            ("chrome", l.chrome, actual[0]),
            ("frame", l.frame, actual[1]),
        ] {
            // Width and origin only. Height is deliberately GTK's, so comparing
            // it against the layout's guess would report a disagreement on every
            // run and train whoever reads this to skip the line.
            let agree = want.w == got.w && want.x == got.x && want.y == got.y;
            println!(
                "glimpse: {name:6} {}x{} at {},{} (layout asked w={} at {},{}) {}",
                got.w as i64,
                got.h as i64,
                got.x as i64,
                got.y as i64,
                want.w as i64,
                want.x as i64,
                want.y as i64,
                if agree { "" } else { "<-- PLACED WRONG" },
            );
        }
    }

    let l = frame.layout();
    if l.hole_from_frame(crate::frame::BORDER) != l.hole {
        anyhow::bail!(
            "the recorded region and the drawn frame disagree: hole {:?} vs inset frame {:?}",
            l.hole,
            l.hole_from_frame(crate::frame::BORDER)
        );
    }

    Ok(())
}
