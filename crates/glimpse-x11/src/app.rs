//! Starting the X11 frontend.
//!
//! Everything GTK-shaped that used to live in `main.rs` is here, so the binary
//! crate can pick a frontend by target without depending on a toolkit itself.

use gtk::glib;
use gtk::prelude::*;
use gtk4 as gtk;
use std::cell::RefCell;
use std::process::ExitCode;
use std::rc::Rc;

use crate::{ui, x11probe};

/// Run Glimpse on X11.
///
/// Returns a process exit code rather than `glib::ExitCode` so the binary crate
/// never has to name GTK.
pub fn run() -> ExitCode {
    let app = gtk::Application::builder()
        .application_id("com.vinicius.glimpse")
        .build();

    // `app.run()` reports success even after `quit()`, so a startup refusal would
    // otherwise exit 0 and any script chaining off Glimpse would march on.
    let refused = Rc::new(RefCell::new(false));
    // Held for the lifetime of the application rather than leaked. Once a session
    // owns an ffmpeg child and a temp workspace, "reap on every exit path" has to
    // be reachable through Drop, which a forgotten owner can never provide.
    let framing: Rc<RefCell<Option<Rc<glimpse_ui::Chrome>>>> = Rc::new(RefCell::new(None));

    let refused_c = refused.clone();
    let framing_c = framing.clone();
    app.connect_activate(move |app| {
        // Connecting to an X server is not proof GTK is *using* X11 — under
        // Wayland, XWayland answers on $DISPLAY while GTK picks its own backend.
        if let Err(e) = x11probe::require_x11_display() {
            eprintln!("glimpse: {e:#}");
            *refused_c.borrow_mut() = true;
            app.quit();
            return;
        }

        let probe = match x11probe::X11Probe::new() {
            Ok(p) => Rc::new(p),
            Err(e) => {
                eprintln!("glimpse: cannot reach the X server: {e:#}");
                *refused_c.borrow_mut() = true;
                app.quit();
                return;
            }
        };

        // Tidy up after Glimpse processes that were killed before they could.
        match glimpse_core::capture::sweep_stale_workspaces() {
            0 => {}
            n => eprintln!("glimpse: removed {n} stale workspace(s) from a previous run"),
        }
        // And staging left in the output folder, which the workspace sweep does
        // not reach because it lives beside the user's finished files.
        match glimpse_core::encode::sweep_stale_staging(
            &glimpse_core::config::Config::load().output_dir,
        ) {
            0 => {}
            n => eprintln!("glimpse: removed {n} stale staging file(s) from a previous run"),
        }

        let framing_window = ui::build(app, probe);
        framing_window.window.present();
        *framing_c.borrow_mut() = Some(framing_window);
    });

    let code = app.run();
    if *refused.borrow() {
        return ExitCode::FAILURE;
    }
    ExitCode::from(glib::ExitCode::get(&code))
}
