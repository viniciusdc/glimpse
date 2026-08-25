//! Glimpse — animated GIF screen recorder with a framing window.

use glimpse::{ui, x11probe};
use gtk::glib;
use gtk::prelude::*;
use gtk4 as gtk;
use std::cell::RefCell;
use std::rc::Rc;

fn main() -> glib::ExitCode {
    let app = gtk::Application::builder()
        .application_id("com.vinicius.glimpse")
        .build();

    // `app.run()` reports success even after `quit()`, so a startup refusal would
    // otherwise exit 0 and any script chaining off Glimpse would march on.
    let refused = Rc::new(RefCell::new(false));
    // Held for the lifetime of the application rather than leaked. Once a session
    // owns an ffmpeg child and a temp workspace, "reap on every exit path" has to
    // be reachable through Drop, which a forgotten owner can never provide.
    let framing: Rc<RefCell<Option<Rc<ui::FramingWindow>>>> = Rc::new(RefCell::new(None));

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

        let framing_window = ui::FramingWindow::new(app, probe);
        framing_window.window.present();
        *framing_c.borrow_mut() = Some(framing_window);
    });

    let code = app.run();
    if *refused.borrow() {
        return glib::ExitCode::FAILURE;
    }
    code
}
