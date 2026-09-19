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
//! permission as **granted**.
//!
//! The explanation that replaced it, "no screen device", was wrong as well: the
//! capture probe was reading an index that does not exist on a runner, and the
//! runner records (#56). Two confident explanations for a limitation that was
//! never there. This one at least asks the system rather than inferring.

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
