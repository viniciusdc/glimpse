//! Starting the macOS frontend.
//!
//! The counterpart of `glimpse_x11::app`, and now almost the same length,
//! because the two window models finally agree: one window, the chrome and the
//! hole inside it, in the order [ADR 0006](../../docs/adr/0006-the-header-is-the-chrome.md)
//! designed ([ADR 0017](../../docs/adr/0017-click-through-is-a-mode-not-a-window.md)).
//!
//! What used to be here — three windows, `attach_strips`, `settle_status`, a
//! seam report and a layout to keep them all in step — is gone. The frame passes
//! clicks by becoming click-through while a recording runs, rather than by being
//! a separate window that never took any.

use gtk::glib;
use gtk::prelude::*;
use gtk4 as gtk;
use std::cell::RefCell;
use std::process::ExitCode;
use std::rc::Rc;

use glimpse_core::config::Config;
use glimpse_core::shutdown;
use glimpse_ui::Chrome;
use objc2_foundation::MainThreadMarker;

use crate::hotkey::HotKey;
use crate::menubar::MenuBarItem;
use crate::shortcut;
use crate::stop::StopPaths;
use crate::window::{set_floating, window_nswindow};

/// Wait for the system to give the menu bar item a slot, then declare it usable.
///
/// **Polled rather than assumed, and never assumed on a timer alone.** Placement
/// is asynchronous and does not happen in the turn the item is created — its
/// window reads `30x0` at the origin at that moment whether or not it will ever
/// be placed. Sleeping "long enough" and declaring success is how a stop path
/// that does not exist gets advertised in place of a Stop button.
///
/// Only on success is the path recorded, because the chrome renders whatever
/// `StopPaths` names.
fn await_placement(item: Rc<MenuBarItem>, stop: StopPaths, tries_left: u32) {
    if item.placed() {
        stop.add("the menu bar");
        return;
    }
    if tries_left == 0 {
        // The geometry, not a guess at the cause. "The menu bar is full" was the
        // first guess and it was wrong: a bare NSApplication places the identical
        // item on this machine after one run loop turn
        // (`examples/menubar_probe.rs`).
        eprintln!(
            "glimpse: the menu bar item was never placed, so a recording cannot be \
             stopped from there. {}",
            item.report()
        );
        return;
    }
    glib::timeout_add_local_once(std::time::Duration::from_millis(250), move || {
        await_placement(item, stop, tries_left - 1);
    });
}

/// Run Glimpse on macOS.
///
/// Returns a process exit code rather than `glib::ExitCode` so the binary crate
/// never has to name GTK.
pub fn run() -> ExitCode {
    let app = gtk::Application::builder()
        .application_id("com.vinicius.glimpse")
        .build();

    // Held for the lifetime of the application rather than dropped at the end of
    // `activate`. Dropping the chrome drops its GTK window and the frame would
    // vanish the instant it appeared; dropping either stop path removes a way to
    // end a recording once the window has gone click-through, and dropping the
    // hotkey also unregisters it system-wide.
    #[allow(clippy::type_complexity)]
    let held: Rc<RefCell<Option<(Rc<Chrome>, Option<Rc<MenuBarItem>>, Option<HotKey>)>>> =
        Rc::new(RefCell::new(None));
    let held_c = held.clone();

    // Before anything can record. `Recorder` reaps its child on every exit path
    // it controls, but none of those run on a signal — and off Linux there is no
    // `PR_SET_PDEATHSIG` to take ffmpeg down with us, so `Ctrl-C` alone would
    // leave it recording forever and holding the screen capture device. Issue
    // #45 is what that costs the *next* recording.
    shutdown::install();

    app.connect_activate(move |app| {
        // Tidy up after Glimpse processes that were killed before they could.
        //
        // **X11 has done this since the sweep existed; macOS never called it.**
        // Nothing failed and nothing warned, so every abnormally-ended session
        // left a temp directory behind permanently — and, off Linux where
        // `die_with_parent` is a no-op, sometimes an ffmpeg still recording into
        // it. Seven had accumulated on one machine before anybody looked, and
        // each one held the screen capture device and broke the next recording
        // (issue #45). A start-up step added to one frontend and not the other
        // is invisible until it is expensive.
        match glimpse_core::capture::sweep_stale_workspaces() {
            0 => {}
            n => eprintln!("glimpse: removed {n} stale workspace(s) from a previous run"),
        }
        // And staging left in the output folder, which the workspace sweep does
        // not reach because it lives beside the user's finished files.
        match glimpse_core::encode::sweep_stale_staging(&Config::load().output_dir) {
            0 => {}
            n => eprintln!("glimpse: removed {n} stale staging file(s) from a previous run"),
        }

        let stop = StopPaths::default();
        let chrome = crate::ui::build(app, stop.clone());
        chrome.window.present();

        // GTK maps windows asynchronously, so there is no NSWindow to configure
        // until the main loop has turned.
        let c = chrome.clone();
        let held = held_c.clone();
        glib::timeout_add_local_once(std::time::Duration::from_millis(200), move || {
            match window_nswindow(c.window.upcast_ref()) {
                // Above ordinary windows, or the application being recorded
                // comes forward and covers the frame. That matters more here
                // than it did with three windows: while passthrough is on, every
                // click the user makes goes to the app behind and would raise it.
                //
                // The drop shadow is deliberately NOT turned off. It was, when
                // the chrome was a separate window directly above the capture
                // region and AppKit drew its shadow downward into every
                // recording (PR #40). One window casts no shadow into its own
                // hole — measured over a fixed backdrop, +0.00 on every sampled
                // row, while the shadow was provably being drawn (-70.08 just
                // outside the window's edge). So macOS gets its elevation back.
                Ok(ns) => set_floating(&ns),
                Err(e) => eprintln!(
                    "glimpse: the frame has no native window, so it will not float \
                     above what you are recording: {e:#}"
                ),
            }

            // The menu bar item, and only recorded as a stop path once the
            // system has actually placed it. The chrome replaces its Stop button
            // with whatever `StopPaths` names, so claiming an item that was
            // never placed would put a label on screen pointing at nothing —
            // ADR 0012's failure, applied to the one control that can end a
            // recording.
            if let Some(mtm) = MainThreadMarker::new() {
                let weak = Rc::downgrade(&c);
                let item = Rc::new(MenuBarItem::install(mtm, move || {
                    // Weak, so the item cannot keep the chrome alive.
                    if let Some(chrome) = weak.upgrade() {
                        chrome.stop_from_outside();
                    }
                }));
                if let Some(slot) = held.borrow_mut().as_mut() {
                    slot.1 = Some(item.clone());
                }
                await_placement(item, stop.clone(), 12);
            }

            // The global hotkey, from the user's own binding. Registration is
            // allowed to fail — another application may already own the
            // combination — and on failure nothing is added to `stop`, so the
            // chrome never shows a key that nothing is listening for.
            let spec = Config::load().stop_shortcut;
            match shortcut::parse(&spec) {
                Some(sc) => {
                    let weak = Rc::downgrade(&c);
                    let key = HotKey::register(&sc, move || {
                        if let Some(chrome) = weak.upgrade() {
                            chrome.stop_from_outside();
                        }
                    });
                    if let Some(key) = key {
                        stop.add(key.display.clone());
                        if let Some(slot) = held.borrow_mut().as_mut() {
                            slot.2 = Some(key);
                        }
                    }
                }
                // Refused rather than approximated. A shortcut the user typed
                // and Glimpse silently reinterpreted would be worse than none:
                // they would be pressing the wrong keys and blaming the app.
                None => eprintln!(
                    "glimpse: stop_shortcut = {spec:?} in config.toml is not a key \
                     combination Glimpse can register, so there is no stop hotkey. \
                     It needs at least one modifier, e.g. \"ctrl+opt+s\"."
                ),
            }

            // `frame up. capture rect` is a contract, not a log line: the macOS
            // CI job waits for it and fails the build if the binary comes up
            // without it. It is also the only thing that distinguishes "the
            // frontend started" from "the process is alive", which on a runner
            // with no window server are otherwise identical.
            match c.capture_rect() {
                Ok(r) => println!(
                    "glimpse: frame up. capture rect {}x{} at {},{} \
                     (device pixels, top-left origin)",
                    r.w, r.h, r.x, r.y
                ),
                Err(e) => eprintln!("glimpse: frame up but the capture rect is unavailable: {e:#}"),
            }
        });

        // Quit the way a user quitting from the UI would, so every destructor
        // runs and the recorder reaps its child. Polled rather than handled in
        // the signal itself, which may not touch GTK. 100ms is the same cadence
        // the chrome's own driver uses; a quarter second of lag on Ctrl-C is not
        // something anyone can feel.
        {
            let app = app.clone();
            let held = held_c.clone();
            glib::timeout_add_local(std::time::Duration::from_millis(100), move || {
                if !shutdown::requested() {
                    return glib::ControlFlow::Continue;
                }
                eprintln!("glimpse: shutting down");
                // Explicitly, and NOT by dropping the chrome. The chrome sits in
                // a reference cycle — window owns widgets, widgets own
                // callbacks, callbacks hold an `Rc<Chrome>` — so its refcount
                // never reaches zero and its fields are never dropped. Measured
                // twice while fixing issue #45: `app.quit()` alone left ffmpeg
                // recording, and so did dropping every `Rc` this function holds.
                if let Some((chrome, _, _)) = held.borrow().as_ref() {
                    chrome.shutdown();
                }
                held.borrow_mut().take();
                app.quit();
                glib::ControlFlow::Break
            });
        }

        // Both stop-path slots start empty and are filled by the timeout above,
        // each only if it actually installed.
        *held_c.borrow_mut() = Some((chrome, None, None));
    });

    ExitCode::from(glib::ExitCode::get(&app.run()))
}
