//! The smallest useful framing window.
//!
//! ```sh
//! cargo run --example framing_window
//! ```
//!
//! Position it over something and click "Show capture rect". The rectangle it
//! reports is exactly what `x11grab` would be handed — it excludes the frame
//! border, because the border is painted by the parent widget and the capture
//! target paints nothing (see ADR 0000).

use anyhow::Result;
use glimpse::{ui::FramingWindow, x11probe::X11Probe};
use gtk::prelude::*;
use gtk4 as gtk;
use std::rc::Rc;

fn main() -> Result<()> {
    let probe = Rc::new(X11Probe::new()?);
    let app = gtk::Application::builder()
        .application_id("com.vinicius.glimpse.example")
        .build();

    app.connect_activate(move |app| {
        let framing = FramingWindow::new(app, probe.clone());
        framing.window.present();
        std::mem::forget(framing); // example only: the app owns nothing else
    });

    app.run();
    Ok(())
}
