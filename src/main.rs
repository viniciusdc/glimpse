//! Glimpse — animated GIF screen recorder with a framing window.

use glimpse::{ui, x11probe};
use gtk::glib;
use gtk::prelude::*;
use gtk4 as gtk;
use std::cell::RefCell;
use std::rc::Rc;

/// Answer `--version` and `--help` before touching GTK.
///
/// A released binary that cannot say what it is leaves the user comparing file
/// dates. Returns true if the process should stop here.
fn handled_cli() -> bool {
    match std::env::args().nth(1).as_deref() {
        Some("--version" | "-V") => {
            println!("glimpse {}", env!("CARGO_PKG_VERSION"));
            true
        }
        Some("--help" | "-h") => {
            println!(
                "glimpse {v}\n\n\
                 A screen recorder with a framing window. Place the window over what you\n\
                 want to record; the hole in the middle is the capture region.\n\n\
                 Usage: glimpse\n\n\
                 Options:\n  \
                 -V, --version   Print the version\n  \
                 -h, --help      Print this help\n\n\
                 Settings live in ~/.config/glimpse/config.toml and in the header menu.\n\
                 Requires an X11 session and ffmpeg.\n\
                 {r}",
                v = env!("CARGO_PKG_VERSION"),
                r = env!("CARGO_PKG_REPOSITORY"),
            );
            true
        }
        _ => false,
    }
}

fn main() -> glib::ExitCode {
    if handled_cli() {
        return glib::ExitCode::SUCCESS;
    }

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

        // Tidy up after Glimpse processes that were killed before they could.
        match glimpse::capture::sweep_stale_workspaces() {
            0 => {}
            n => eprintln!("glimpse: removed {n} stale workspace(s) from a previous run"),
        }
        // And staging left in the output folder, which the workspace sweep does
        // not reach because it lives beside the user's finished files.
        match glimpse::encode::sweep_stale_staging(&glimpse::config::Config::load().output_dir) {
            0 => {}
            n => eprintln!("glimpse: removed {n} stale staging file(s) from a previous run"),
        }

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
