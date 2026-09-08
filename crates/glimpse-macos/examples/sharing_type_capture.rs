//! Does `NSWindow.sharingType = .none` hide a window from Glimpse's own capture?
//!
//! ```sh
//! cargo run -p glimpse-macos --example sharing_type_capture
//! ```
//!
//! Not a click-through question. This is the *other* problem the frame has: the
//! app's own chrome landing inside the recording. That has already cost one
//! shipped bug — the chrome's drop shadow was baked into the top 40 device
//! pixels of every macOS recording, with a correct capture rect and every
//! geometry check green, because a shadow is a gradient and the expanded-crop
//! test looks for a colour.
//!
//! If the window server can be told to keep a window out of a capture, then a
//! whole class of contamination stops being something the layout has to avoid by
//! hand. That would not change the window model — clicks are a separate matter,
//! settled in [ADR 0015] — but it would change what the frame is allowed to
//! overlap.
//!
//! ## Through the shipping path, not `screencapture`
//!
//! The recording is made with `AvfCapture` → `GrabCommand` → `Recorder`, the
//! same chain the application uses. A check built on its own copy of the ffmpeg
//! arguments can only ever vouch for that copy — `grab_through_the_shipping_path`
//! in the chrome records what happened the one time a second copy existed.
//!
//! ## Both directions
//!
//! Two identical opaque windows, side by side, inside the captured rectangle:
//!
//! * **LEFT is the control**, with the default sharing type. It MUST appear in
//!   the recording. Without it, a capture that failed for any unrelated reason —
//!   a black frame, a wrong rectangle, a denied permission — would read as
//!   "sharingType works" for every window on screen.
//! * **RIGHT has `sharingType = .none`.** Whether it appears is the question.
//!
//! **macOS grants Screen Recording permission to the program running this — your
//! terminal — not to the binary.** A refusal looks exactly like a bug in the
//! code, so the control above is what tells those apart.
//!
//! [ADR 0015]: ../../../docs/adr/0015-the-frame-is-two-windows.md

#[cfg(not(target_os = "macos"))]
fn main() {
    eprintln!("This example needs AppKit. macOS only.");
    std::process::exit(1);
}

#[cfg(target_os = "macos")]
fn main() -> anyhow::Result<()> {
    imp::run()
}

#[cfg(target_os = "macos")]
mod imp {
    use anyhow::{Context, Result};
    use glimpse_core::capture::{GrabRequest, Recorder, Workspace};
    use glimpse_core::geometry::ScreenPixelRect;
    use glimpse_macos::geometry::AppKitRect;
    use glimpse_macos::window::{place, set_floating, window_nswindow};
    use glimpse_macos::AvfCapture;
    use gtk::glib;
    use gtk::prelude::*;
    use gtk4 as gtk;
    use objc2_app_kit::{NSScreen, NSWindowSharingType};
    use objc2_foundation::MainThreadMarker;
    use std::cell::RefCell;
    use std::rc::Rc;
    use std::time::Duration;

    /// Two panels, side by side, both inside the recorded rectangle.
    const CONTROL: AppKitRect = AppKitRect {
        x: 320.0,
        y: 400.0,
        w: 300.0,
        h: 220.0,
    };
    const HIDDEN: AppKitRect = AppKitRect {
        x: 700.0,
        y: 400.0,
        w: 300.0,
        h: 220.0,
    };

    /// Saturated and distinct, so "is it in the frame" is a colour test rather
    /// than a judgement call. Nothing else on a desktop is this green or this
    /// magenta by accident.
    const CSS: &str = "
        window.control { background: #00c853; }
        window.hidden  { background: #d500f9; }
    ";

    pub fn run() -> Result<()> {
        let app = gtk::Application::builder()
            .application_id("com.vinicius.glimpse.example.sharingtype")
            .build();

        let held: Rc<RefCell<Vec<gtk::Window>>> = Rc::new(RefCell::new(Vec::new()));
        let held_c = held.clone();

        app.connect_activate(move |app| {
            let provider = gtk::CssProvider::new();
            provider.load_from_data(CSS);
            if let Some(display) = gtk::gdk::Display::default() {
                gtk::style_context_add_provider_for_display(
                    &display,
                    &provider,
                    gtk::STYLE_PROVIDER_PRIORITY_APPLICATION,
                );
            }

            let control = panel(app, "control", CONTROL);
            let hidden = panel(app, "hidden", HIDDEN);
            held_c.borrow_mut().push(control.clone());
            held_c.borrow_mut().push(hidden.clone());

            let app = app.clone();
            glib::timeout_add_local_once(Duration::from_millis(500), move || {
                if let Err(e) = measure(&control, &hidden) {
                    eprintln!("sharing: {e:#}");
                }
                app.quit();
            });
        });

        app.run();
        Ok(())
    }

    fn panel(app: &gtk::Application, class: &str, r: AppKitRect) -> gtk::Window {
        let w = gtk::Window::builder()
            .application(app)
            .decorated(false)
            .resizable(false)
            .default_width(r.w as i32)
            .default_height(r.h as i32)
            .build();
        w.add_css_class(class);
        w.present();
        w
    }

    fn measure(control: &gtk::Window, hidden: &gtk::Window) -> Result<()> {
        let mtm = MainThreadMarker::new().expect("GTK runs on the main thread");

        let c_ns = window_nswindow(control)?;
        let h_ns = window_nswindow(hidden)?;
        place(&c_ns, CONTROL);
        place(&h_ns, HIDDEN);
        set_floating(&c_ns);
        set_floating(&h_ns);

        // The whole experiment, one line.
        h_ns.setSharingType(NSWindowSharingType::None);

        // Set, then let the server process it, then read it back — a macOS
        // window property is not in effect when the call returns.
        pump();
        println!(
            "sharing: control sharingType = {:?}, hidden sharingType = {:?}",
            c_ns.sharingType(),
            h_ns.sharingType()
        );

        // Only the primary screen's height is used for the flip, and only the
        // integer backing factor is ever trusted.
        let primary = NSScreen::screens(mtm).iter().next().context("no screens")?;
        let screen_h = primary.frame().size.height;
        let scale = primary.backingScaleFactor();

        // One rectangle covering both panels, in global device pixels with a
        // top-left origin, which is what ScreenPixelRect documents.
        let left = CONTROL.x - 40.0;
        let right = HIDDEN.x + HIDDEN.w + 40.0;
        let top = CONTROL.y + CONTROL.h + 40.0;
        let bottom = CONTROL.y - 40.0;
        let rect = ScreenPixelRect {
            x: (left * scale) as i32,
            y: ((screen_h - top) * scale) as i32,
            w: ((right - left) * scale) as i32,
            h: ((top - bottom) * scale) as i32,
        };

        let backend = AvfCapture::discover().context("finding the screen capture device")?;
        let grab = backend.grab(&GrabRequest {
            rect,
            framerate: Some(10),
            capture_mouse: false,
        });
        println!("sharing: device {} ", backend.device());
        println!(
            "sharing: recording {}x{} at {},{}",
            rect.w, rect.h, rect.x, rect.y
        );

        let workspace = Workspace::create()?;
        let recorder = Recorder::start(&grab, workspace).context("starting the recorder")?;
        std::thread::sleep(Duration::from_millis(1500));
        let video = recorder.stop().context("stopping the recorder")?;
        println!("sharing: captured {}", video.path.display());

        // A still from the recording, so the check is on what was actually
        // written rather than on a second, separate grab.
        let still = std::env::temp_dir().join("glimpse-sharing-probe.png");
        let out = std::process::Command::new("ffmpeg")
            .args(["-y", "-v", "error", "-i"])
            .arg(&video.path)
            .args(["-frames:v", "1"])
            .arg(&still)
            .output()
            .context("extracting a frame with ffmpeg")?;
        if !out.status.success() {
            anyhow::bail!(
                "ffmpeg could not extract a frame: {}",
                String::from_utf8_lossy(&out.stderr)
            );
        }
        println!("sharing: still at {}", still.display());
        println!();
        println!("Sample it — both panel centres, relative to the captured rect:");
        println!(
            "  control centre: {},{}",
            ((CONTROL.x + CONTROL.w / 2.0 - left) * scale) as i32,
            ((top - (CONTROL.y + CONTROL.h / 2.0)) * scale) as i32,
        );
        println!(
            "  hidden  centre: {},{}",
            ((HIDDEN.x + HIDDEN.w / 2.0 - left) * scale) as i32,
            ((top - (HIDDEN.y + HIDDEN.h / 2.0)) * scale) as i32,
        );
        println!();
        println!("  #00c853 green at the control centre  => the capture works and can see windows");
        println!("  #d500f9 magenta at the hidden centre  => sharingType does NOT exclude it");
        println!("  anything else at the hidden centre    => it was excluded");
        Ok(())
    }

    fn pump() {
        let ctx = glib::MainContext::default();
        for _ in 0..300 {
            if !ctx.iteration(false) {
                break;
            }
        }
        std::thread::sleep(Duration::from_millis(200));
        while ctx.iteration(false) {}
    }
}
