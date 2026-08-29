//! Starting the macOS frontend.
//!
//! The counterpart of `glimpse_x11::app`, and deliberately much smaller: there
//! is a frame and nothing else yet. No session, no recording, no header
//! controls. Running it puts the frame on screen and reports the rectangle a
//! recording would capture.
//!
//! That is worth shipping before the controls exist, because the frame is the
//! part that could not be built at all until [ADR 0011] settled how, and every
//! remaining piece of #9 is easier to judge against something visible.
//!
//! [ADR 0011]: ../../docs/adr/0011-why-the-macos-frame-is-more-than-one-window.md

use gtk::glib;
use gtk::prelude::*;
use gtk4 as gtk;
use objc2_foundation::MainThreadMarker;
use std::cell::RefCell;
use std::process::ExitCode;
use std::rc::Rc;

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
    // `activate`. Dropping the Frame drops both GTK windows, and the frame would
    // vanish the instant it appeared.
    let held: Rc<RefCell<Option<Frame>>> = Rc::new(RefCell::new(None));
    let failed = Rc::new(RefCell::new(false));

    let held_c = held.clone();
    let failed_c = failed.clone();
    app.connect_activate(move |app| {
        let frame = Frame::new(app, INITIAL_HOLE);
        let held = held_c.clone();
        let failed = failed_c.clone();
        let app = app.clone();

        // GTK maps windows asynchronously, so there is no NSWindow to position
        // until the main loop has turned. `realize` reports that rather than
        // silently doing nothing, which is why its result is checked.
        glib::timeout_add_local_once(std::time::Duration::from_millis(200), move || {
            if let Err(e) = realize_and_report(&frame) {
                eprintln!("glimpse: {e:#}");
                *failed.borrow_mut() = true;
                app.quit();
                return;
            }
            *held.borrow_mut() = Some(frame);
        });
    });

    let code = app.run();
    if *failed.borrow() {
        return ExitCode::FAILURE;
    }
    ExitCode::from(glib::ExitCode::get(&code))
}

fn realize_and_report(frame: &Frame) -> anyhow::Result<()> {
    frame.realize()?;

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
    let l = frame.layout();
    if l.hole_from_frame(crate::frame::BORDER) != l.hole {
        anyhow::bail!(
            "the recorded region and the drawn frame disagree: hole {:?} vs inset frame {:?}",
            l.hole,
            l.hole_from_frame(crate::frame::BORDER)
        );
    }

    println!("glimpse: no controls yet — this is the frame only. Ctrl-C to quit.");
    Ok(())
}
