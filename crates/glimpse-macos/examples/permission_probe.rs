//! What does macOS say about Screen Recording permission here?
//!
//! ```sh
//! cargo run -p glimpse-macos --example permission_probe
//! ```
//!
//! **Preflight only. This never prompts**, so it is safe in CI and safe to run
//! on a machine whose answer you do not want to change.
//!
//! WHY IT EXISTS. "A GitHub runner has no Screen Recording permission" was
//! asserted, then used to explain why the macOS journeys cannot run there, then
//! written into a CI assertion — which failed, because the runner reports the
//! permission as **granted**. What it does not have is a screen device:
//! `-list_devices` comes back with an I/O error and an empty list.
//!
//! Those are two different failures with two different fixes, and a capture that
//! refuses tells you which one only if you ask. This asks.

#[cfg(not(target_os = "macos"))]
fn main() {
    eprintln!("This example needs CoreGraphics. macOS only.");
    std::process::exit(1);
}

#[cfg(target_os = "macos")]
fn main() {
    let granted = glimpse_macos::permission::granted();
    println!(
        "SCREEN-RECORDING-PREFLIGHT: {}",
        if granted { "granted" } else { "denied" }
    );
    if !granted {
        println!("{}", glimpse_macos::permission::explain());
    }
}
