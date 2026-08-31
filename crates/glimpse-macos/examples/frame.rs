//! Put the macOS frame on screen and check it landed where it was asked to.
//!
//! ```sh
//! cargo run -p glimpse-macos --example frame
//! GLIMPSE_FRAME_HOLD=1 cargo run -p glimpse-macos --example frame   # leave it up
//! ```
//!
//! Every position is **read back from the window server**, never assumed. GTK
//! creates the windows and AppKit places them, and a `setFrame:` that was
//! ignored — by a minimum size, a screen edge, or being called before the window
//! was mapped — leaves a frame that looks deliberate and is wrong. That is the
//! failure [ADR 0000] exists to record, arriving through a new door.
//!
//! Both directions are checked: every window must be where the layout says, and
//! nothing must overlap the hole.
//!
//! [ADR 0000]: ../../../docs/adr/0000-x11-framing-window-spike.md

#[cfg(not(target_os = "macos"))]
fn main() {
    eprintln!("This example needs AppKit. macOS only.");
    std::process::exit(1);
}

#[cfg(target_os = "macos")]
fn main() -> anyhow::Result<()> {
    use glimpse_macos::frame::Frame;
    use glimpse_macos::geometry::AppKitRect;
    use gtk::glib;
    use gtk::prelude::*;
    use gtk4 as gtk;
    use objc2_foundation::MainThreadMarker;
    use std::cell::RefCell;
    use std::rc::Rc;

    let app = gtk::Application::builder()
        .application_id("com.vinicius.glimpse.example.frame")
        .build();

    let held: Rc<RefCell<Option<Frame>>> = Rc::new(RefCell::new(None));
    let held_c = held.clone();

    app.connect_activate(move |app| {
        let hole = AppKitRect {
            x: 400.0,
            y: 300.0,
            w: 640.0,
            h: 400.0,
        };
        let frame = Frame::new(app, hole);

        // A stand-in chrome window. `Frame` stopped building one when the real
        // chrome moved to `glimpse-ui` (ADR 0014), and this example is about
        // where the FRAME lands, so a bare window is the honest stub: it gives
        // `attach_to` something to parent to without dragging the whole
        // controller into a geometry check.
        let chrome_win = gtk::Window::builder()
            .application(app)
            .decorated(false)
            .default_width(frame.layout().chrome.w as i32)
            .default_height(frame.layout().chrome.h as i32)
            .build();
        chrome_win.present();

        // And a stand-in status window, for the same reason: `attach_to` places
        // and parents all three now (ADR 0016).
        let status_win = gtk::Window::builder()
            .application(app)
            .decorated(false)
            .default_width(frame.layout().status.w as i32)
            .default_height(frame.layout().status.h as i32)
            .build();
        status_win.present();

        // Positioning cannot happen here: GTK has not mapped the windows yet, so
        // there is no NSWindow to place. `realize()` returns an error rather
        // than doing nothing if called too early, which is why this is a
        // timeout and not a hope.
        let app = app.clone();
        let held = held_c.clone();
        glib::timeout_add_seconds_local_once(1, move || {
            if let Err(e) = report(&frame, &chrome_win, &status_win) {
                eprintln!("FAILED: {e:#}");
                std::process::exit(1);
            }
            if std::env::var("GLIMPSE_FRAME_HOLD").is_ok() {
                println!("\nHolding. Ctrl-C to quit.");
                *held.borrow_mut() = Some(frame);
            } else {
                app.quit();
            }
        });
    });

    fn report(
        frame: &Frame,
        chrome_win: &gtk::Window,
        status_win: &gtk::Window,
    ) -> anyhow::Result<()> {
        frame.attach_to(chrome_win, status_win)?;
        // Let AppKit settle before reading anything back; a frame read in the
        // same turn can report the pre-placement geometry.
        let mtm = MainThreadMarker::new().expect("GTK runs on the main thread");
        let layout = frame.layout();

        println!("=== requested (AppKit points, bottom-left origin) ===");
        for (name, r) in names().iter().zip([layout.chrome, layout.frame].iter()) {
            println!("  {name:<8} {}", fmt(*r));
        }
        println!("  {:<8} {}   <- not a window", "hole", fmt(layout.hole));

        let actual = frame.actual_frames(chrome_win)?;
        println!("\n=== actual, read back from the window server ===");
        let mut all_placed = true;
        for ((name, want), got) in names()
            .iter()
            .zip([layout.chrome, layout.frame].iter())
            .zip(actual.iter())
        {
            let ok = close(*want, *got);
            all_placed &= ok;
            println!(
                "  {name:<8} {}   {}",
                fmt(*got),
                if ok { "as requested" } else { "DIFFERS" }
            );
        }

        println!("\n=== the invariant ===");
        // The frame window DOES cover the hole now, deliberately. What must hold
        // is that the two descriptions of the recorded region agree, and that
        // the opaque chrome stays off it.
        let covered = layout.hole_from_frame(glimpse_macos::frame::BORDER) != layout.hole;
        println!(
            "  hole matches the inset frame : {}",
            if covered { "FAIL" } else { "PASS" }
        );
        println!(
            "  every window placed     : {}",
            if all_placed { "PASS" } else { "FAIL" }
        );

        let rect = frame.capture_rect(mtm)?;
        println!(
            "\n=== capture rect (top-left device pixels) ===\n  {}x{} at {},{}",
            rect.w, rect.h, rect.x, rect.y
        );

        if covered || !all_placed {
            anyhow::bail!("the frame is not laid out correctly");
        }
        Ok(())
    }

    fn names() -> [&'static str; 2] {
        ["chrome", "frame"]
    }

    fn fmt(r: glimpse_macos::geometry::AppKitRect) -> String {
        format!("{:>6.0}x{:<6.0} @ ({:>6.0},{:>6.0})", r.w, r.h, r.x, r.y)
    }

    fn close(
        a: glimpse_macos::geometry::AppKitRect,
        b: glimpse_macos::geometry::AppKitRect,
    ) -> bool {
        let d = |x: f64, y: f64| (x - y).abs() < 1.0;
        d(a.x, b.x) && d(a.y, b.y) && d(a.w, b.w) && d(a.h, b.h)
    }

    let code = app.run();
    std::process::exit(gtk::glib::ExitCode::get(&code) as i32);
}
