//! Does a status item reach the menu bar at all in this process?
//!
//! ```sh
//! cargo run -p glimpse-macos --example menubar_probe
//! ```
//!
//! **This is a control, and it exists because a negative result was about to be
//! attributed to the wrong thing.** Glimpse's menu bar item is created,
//! retained, reports `isVisible = true`, and never appears: its window comes
//! back at `(0, -33)`, off the bottom of the display. Deferring the install
//! until after the window was up did not change it. Neither did running the same
//! binary from inside a real `.app` bundle with a bundle identifier, which ruled
//! out the one explanation [ADR 0013](../../../docs/adr/0013-macos-ships-an-app-bundle.md)
//! would have made attractive.
//!
//! The remaining candidates were "the menu bar is full" and "placement takes
//! longer under GTK than anyone waited". Those want completely different
//! answers — one of them means the menu bar cannot be the primary stop path at
//! all — so this asks the question with no GTK in the process: a bare
//! `NSApplication`, the same `MenuBarItem::install`, and a run loop.
//!
//! **It answered after one turn**, placing the item at `(1030, 949)`. The menu
//! bar had room all along; the app was simply concluding failure too early. The
//! fix was `await_placement` in `app.rs`, which polls instead of guessing, and
//! under GTK the item lands after roughly a second.
//!
//! Kept rather than deleted, because it is the control that separates those two
//! causes. If the item ever stops appearing again, run this first: it appearing
//! here and not in the app means the app is asking too early, and it appearing
//! in neither means the machine has no room.

#[cfg(not(target_os = "macos"))]
fn main() {
    eprintln!("This example needs AppKit. macOS only.");
    std::process::exit(1);
}

#[cfg(target_os = "macos")]
fn main() {
    use glimpse_macos::menubar::MenuBarItem;
    use objc2_app_kit::{NSApplication, NSApplicationActivationPolicy};
    use objc2_foundation::{MainThreadMarker, NSDate, NSRunLoop};

    let mtm = MainThreadMarker::new().expect("the probe runs on the main thread");
    let app = NSApplication::sharedApplication(mtm);
    // Regular, not Accessory: the same policy a GTK application gets. An
    // Accessory app has no Dock icon and different menu bar behaviour, so
    // testing with one would answer a question nobody asked.
    app.setActivationPolicy(NSApplicationActivationPolicy::Regular);

    let item = MenuBarItem::install(mtm, || println!("probe: stop chosen"));
    app.finishLaunching();

    // The item is not placed in the turn it is created — measured, its window is
    // 28x0 at the origin immediately afterwards whether or not it will ever be
    // placed. So the loop runs before anything is concluded.
    let loop_ = NSRunLoop::currentRunLoop();
    for i in 0..12 {
        loop_.runUntilDate(&NSDate::dateWithTimeIntervalSinceNow(0.25));
        if item.placed() {
            println!("probe: placed after {} turns", i + 1);
            println!("{}", item.report());
            println!("\nRESULT: a bare NSApplication CAN place a status item here.");
            println!("        The menu bar has room, so the failure under GTK is GTK's.");
            return;
        }
    }

    println!("{}", item.report());
    println!("\nRESULT: even a bare NSApplication cannot place a status item here.");
    println!("        The menu bar has no room on this machine, so a menu bar item");
    println!("        cannot be relied on as the way to stop a recording.");
}
