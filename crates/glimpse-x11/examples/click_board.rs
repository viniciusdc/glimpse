//! A board that says where it was clicked, for testing click-through.
//!
//! ```sh
//! cargo run -p glimpse-x11 --example click_board -- 1920 1080
//! ```
//!
//! Put this behind the framing window, aim a click at the hole, and see whether
//! the board reacts. That is the only check that tests the *consequence* of the
//! input region — whether a person can touch the application they are
//! recording — rather than the region itself.
//!
//! `scripts/clickthrough.sh` drives it. There is a matching one for macOS in
//! `glimpse-macos/examples/click_through_blackboard.rs`, which cannot be
//! automated: synthesising a click there needs Accessibility permission. Under
//! Xvfb on X11 it needs nothing, which is why the automated half of this idea
//! lives on this side.
//!
//! ## What it prints
//!
//! One line per click, and the format is a contract the script greps:
//!
//! ```text
//! BOARD-CLICK 960 540
//! ```
//!
//! Coordinates are the board's own, and the board is placed at the origin with
//! no window manager to move it, so they are screen coordinates too. It also
//! prints `BOARD-READY` once mapped, so the driver waits for a fact rather than
//! for a duration.

use gtk::prelude::*;
use gtk4 as gtk;

fn main() {
    let (w, h) = {
        let mut a = std::env::args().skip(1);
        let w: i32 = a.next().and_then(|v| v.parse().ok()).unwrap_or(1920);
        let h: i32 = a.next().and_then(|v| v.parse().ok()).unwrap_or(1080);
        (w, h)
    };

    let app = gtk::Application::builder()
        .application_id("com.vinicius.glimpse.example.clickboard")
        .build();

    app.connect_activate(move |app| {
        let area = gtk::DrawingArea::new();
        area.set_hexpand(true);
        area.set_vexpand(true);
        // Opaque and distinct, so a screenshot of a failing run shows what was
        // where. Nothing depends on the colour.
        area.set_draw_func(|_, cr, w, h| {
            cr.set_source_rgb(0.08, 0.09, 0.11);
            cr.rectangle(0.0, 0.0, w as f64, h as f64);
            let _ = cr.fill();
        });

        let click = gtk::GestureClick::new();
        click.connect_pressed(|_, _, x, y| {
            // The contract. `scripts/clickthrough.sh` greps for this exact
            // prefix, and flushes matter: a buffered line that arrives after the
            // driver has already decided is a click that did not count.
            println!("BOARD-CLICK {} {}", x as i32, y as i32);
            use std::io::Write;
            let _ = std::io::stdout().flush();
        });
        area.add_controller(click);

        let window = gtk::ApplicationWindow::builder()
            .application(app)
            .decorated(false)
            .default_width(w)
            .default_height(h)
            .build();
        window.set_child(Some(&area));

        // BEFORE `present`, not after. Connected afterwards the map has already
        // happened, the handler never runs, and the driver waits ten seconds for
        // a line that will never come — which is what it did on its first CI
        // run.
        //
        // Announced rather than timed, so the driver waits for the board to
        // exist instead of sleeping and hoping. A board that is not up yet takes
        // no clicks, and "the click did not land" would read as a finding.
        window.connect_map(|_| {
            println!("BOARD-READY");
            use std::io::Write;
            let _ = std::io::stdout().flush();
        });

        window.present();
    });

    // **No argv.** A GApplication given positional arguments treats them as
    // files to open, and one without `HANDLES_OPEN` answers "This application
    // can not open files" and never emits `activate` at all. The board then
    // never mapped and the driver reported that as the finding. The size is read
    // from `std::env::args` above, before GTK sees anything.
    app.run_with_args::<&str>(&[]);
}
