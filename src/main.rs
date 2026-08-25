//! Glimpse — animated GIF screen recorder with a framing window.

use glimpse::{ui, x11probe};
use gtk::prelude::*;
use gtk4 as gtk;
use std::rc::Rc;

fn main() -> gtk::glib::ExitCode {
    let app = gtk::Application::builder()
        .application_id("com.vinicius.glimpse")
        .build();

    app.connect_activate(|app| {
        let probe = match x11probe::X11Probe::new() {
            Ok(p) => Rc::new(p),
            Err(e) => {
                eprintln!("glimpse: cannot reach the X server: {e:#}");
                eprintln!(
                    "glimpse: v0.1 is X11-only — Wayland is not a missing backend (ADR 0002)."
                );
                app.quit();
                return;
            }
        };
        let framing = ui::FramingWindow::new(app, probe);
        framing.window.present();
        // Leaked deliberately: the window owns the application's lifetime.
        std::mem::forget(framing);
    });

    app.run()
}
