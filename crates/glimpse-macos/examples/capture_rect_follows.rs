//! Move the frame; does the recorded rectangle move with it?
//!
//! ```sh
//! cargo run -p glimpse-macos --example capture_rect_follows
//! ```
//!
//! The three-window design answered `capture_rect` from a `Layout` computed once
//! in `Frame::new` and never recomputed. So after the frame was dragged, macOS
//! reported — and would have recorded — the rectangle the frame *started* at.
//! Nothing caught it: `geometry_drifted` compares the frozen rect against a
//! fresh `capture_rect`, and both sides read the same stale value, so the guard
//! [ADR 0002](../../../docs/adr/0002-ffmpeg-pipeline-and-session-model.md)
//! exists for could not fire.
//!
//! [ADR 0017](../../../docs/adr/0017-click-through-is-a-mode-not-a-window.md)
//! claims one window deletes that bug rather than patching it, because the rect
//! is derived from `compute_bounds` on every call. This is that claim, checked.
//!
//! **It runs against the real chrome**, through `ui::build` and the same
//! `PlatformHooks::capture_rect` the Record button uses. A probe that built its
//! own window and its own arithmetic could only vouch for its own arithmetic —
//! which is the failure `grab_through_the_shipping_path` records.
//!
//! Both directions: the rectangle must move by the delta the window moved, and
//! its size must not change. Checking only that it changed would pass for a
//! rectangle that moved by the wrong amount, which is the whole bug.

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
    use glimpse_macos::window::{appkit_frame, window_nswindow};
    use gtk::glib;
    use gtk::prelude::*;
    use gtk4 as gtk;
    use objc2_foundation::NSPoint;
    use std::cell::RefCell;
    use std::rc::Rc;
    use std::time::Duration;

    /// How far to move the window, in AppKit points. Chosen to be large enough
    /// that no rounding could account for it and asymmetric so a transposed x/y
    /// shows up as a failure rather than as a pass.
    const DX: f64 = 120.0;
    const DY: f64 = -70.0;

    pub fn run() -> Result<()> {
        let app = gtk::Application::builder()
            .application_id("com.vinicius.glimpse.example.followrect")
            .build();

        let held: Rc<RefCell<Option<Rc<glimpse_ui::Chrome>>>> = Rc::new(RefCell::new(None));
        let held_c = held.clone();
        let failed = Rc::new(RefCell::new(false));
        let failed_c = failed.clone();

        app.connect_activate(move |app| {
            // No stop paths: this probe never records, and an empty `StopPaths`
            // is what the chrome sees before a frontend installs any.
            let chrome = glimpse_macos::ui::build(app, Default::default());
            chrome.window.present();

            let (c, failed) = (chrome.clone(), failed_c.clone());
            let app = app.clone();
            glib::timeout_add_local_once(Duration::from_millis(600), move || {
                if let Err(e) = measure(&c) {
                    eprintln!("follows: {e:#}");
                    *failed.borrow_mut() = true;
                }
                app.quit();
            });
            *held_c.borrow_mut() = Some(chrome);
        });

        app.run();
        // A non-zero exit, so this is usable as a check rather than only as
        // something to read.
        if *failed.borrow() {
            std::process::exit(1);
        }
        Ok(())
    }

    fn measure(chrome: &Rc<glimpse_ui::Chrome>) -> Result<()> {
        let ns = window_nswindow(chrome.window.upcast_ref())
            .context("the window has no NSWindow yet")?;

        let before_frame = appkit_frame(&ns);
        let before = chrome
            .capture_rect()
            .context("capture_rect before the move")?;
        println!(
            "follows: window at {},{}   capture rect {}x{} at {},{}",
            before_frame.x as i64, before_frame.y as i64, before.w, before.h, before.x, before.y
        );

        // `setFrameOrigin:` specifically. It is the call that used to have to
        // propagate to child windows; with one window there are no children, but
        // moving by origin is still what a drag does.
        ns.setFrameOrigin(NSPoint::new(before_frame.x + DX, before_frame.y + DY));
        // The window server has not processed the move when the call returns.
        pump();

        let after_frame = appkit_frame(&ns);
        let after = chrome
            .capture_rect()
            .context("capture_rect after the move")?;
        println!(
            "follows: window at {},{}   capture rect {}x{} at {},{}",
            after_frame.x as i64, after_frame.y as i64, after.w, after.h, after.x, after.y
        );

        // Control: if the window did not actually move, the rectangle not moving
        // proves nothing at all.
        let moved_x = after_frame.x - before_frame.x;
        let moved_y = after_frame.y - before_frame.y;
        if (moved_x - DX).abs() > 0.5 || (moved_y - DY).abs() > 0.5 {
            anyhow::bail!(
                "CONTROL FAILED: asked the window to move by ({DX}, {DY}) and it moved by \
                 ({moved_x}, {moved_y}). Nothing below is evidence."
            );
        }

        // Two conversions, and the first draft of this check forgot one of them.
        //
        // 1. The window moves in POINTS; the capture rect is in DEVICE PIXELS.
        //    On a 2x display a 120pt move is a 240px move, and comparing the two
        //    directly failed this probe against a correct implementation.
        // 2. AppKit y counts up from the bottom of the screen; the capture rect
        //    counts down from the top, so +DY in AppKit is -DY here.
        //
        // Only the integer backing factor is trusted — never anything derived
        // from a monitor's reported physical size (AGENTS.md).
        let mtm = objc2_foundation::MainThreadMarker::new().context("not on the main thread")?;
        let scale = objc2_app_kit::NSScreen::screens(mtm)
            .iter()
            .next()
            .context("no screens")?
            .backingScaleFactor();
        let want_x = before.x + (DX * scale) as i32;
        let want_y = before.y - (DY * scale) as i32;

        let mut wrong = Vec::new();
        if after.x != want_x {
            wrong.push(format!("x: got {}, want {want_x}", after.x));
        }
        if after.y != want_y {
            wrong.push(format!("y: got {}, want {want_y}", after.y));
        }
        // The size must not change. A rect recomputed from a resized widget
        // would drift here, and a recorder that quietly changes resolution
        // mid-session is its own bug.
        if after.w != before.w || after.h != before.h {
            wrong.push(format!(
                "size changed: {}x{} → {}x{}",
                before.w, before.h, after.w, after.h
            ));
        }

        if !wrong.is_empty() {
            anyhow::bail!("FAIL (move): {}", wrong.join("; "));
        }
        println!(
            "\nmove: the capture rect followed by exactly ({}, {}) device pixels.",
            after.x - before.x,
            after.y - before.y
        );

        // ---- and the same question for a RESIZE -------------------------
        //
        // Resizing was issue #10's open question, and the answer arrived by
        // hand: dragging the window edge works, because GDK gives the window the
        // Resizable style mask and AppKit provides its own edges. What a manual
        // drag cannot check is the consequence — that the recorded region
        // follows the frame it is drawn inside. A frame you can resize into a
        // rectangle that records something else is worse than one you cannot
        // resize at all.
        let grow_w = 90.0;
        let grow_h = 60.0;
        let f = appkit_frame(&ns);
        // `setFrame:display:` and not `setFrameOrigin:`: this is a size change,
        // and it deliberately keeps the origin so the two effects do not have to
        // be told apart.
        ns.setFrame_display(
            objc2_foundation::NSRect::new(
                NSPoint::new(f.x, f.y),
                objc2_foundation::NSSize::new(f.w + grow_w, f.h + grow_h),
            ),
            true,
        );
        pump();

        let grown_frame = appkit_frame(&ns);
        let grown = chrome
            .capture_rect()
            .context("capture_rect after the resize")?;
        println!(
            "follows: window {}x{}          capture rect {}x{} at {},{}",
            grown_frame.w as i64, grown_frame.h as i64, grown.w, grown.h, grown.x, grown.y
        );

        // Control again: a window that refused to resize would make an unchanged
        // rectangle look like a pass.
        let dw = grown_frame.w - f.w;
        let dh = grown_frame.h - f.h;
        if (dw - grow_w).abs() > 0.5 || (dh - grow_h).abs() > 0.5 {
            anyhow::bail!(
                "CONTROL FAILED: asked the window to grow by ({grow_w}, {grow_h}) and it grew \
                 by ({dw}, {dh}). Nothing below is evidence."
            );
        }

        // The hole grows with the window, so the recorded region must grow by
        // the same amount in device pixels. The chrome above and the status bar
        // below keep their heights, so all of the extra height lands in the hole.
        let want_w = before.w + (grow_w * scale) as i32;
        let want_h = before.h + (grow_h * scale) as i32;
        let mut wrong = Vec::new();
        if grown.w != want_w {
            wrong.push(format!("width: got {}, want {want_w}", grown.w));
        }
        if grown.h != want_h {
            wrong.push(format!("height: got {}, want {want_h}", grown.h));
        }
        if !wrong.is_empty() {
            anyhow::bail!("FAIL (resize): {}", wrong.join("; "));
        }

        println!(
            "resize: the capture rect grew by exactly ({}, {}) device pixels.",
            grown.w - before.w,
            grown.h - before.h
        );
        println!("\nPASS: the rectangle is derived on every call, not remembered (ADR 0017).");
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
