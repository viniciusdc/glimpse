//! Glimpse — animated screen recorder with a framing window.
//!
//! This crate is the binary and nothing else. The window model is a compile-time
//! choice with exactly one candidate per target, so the frontend is selected by a
//! dependency edge rather than by a trait — see
//! [ADR 0010](../docs/adr/0010-capture-providers-and-a-platform-free-core.md).
//!
//! Nothing toolkit-shaped is named here, which is what lets the crate resolve on
//! a platform that has no frontend yet.

use std::process::ExitCode;

/// The platforms a frontend exists for, for the `--help` text and the refusal
/// message. Kept as one list so the two cannot disagree.
///
/// It used to carry a qualifier — "macOS (frame only, no controls yet)" — which
/// was true when macOS put up a frame it could not record from. It is not any
/// more: macOS runs the same chrome X11 runs, in one window
/// ([ADR 0017](../docs/adr/0017-click-through-is-a-mode-not-a-window.md)).
///
/// Note what did *not* catch that. The macOS CI job greps `--help` for the
/// substring `macOS`, which a sentence saying macOS cannot record passes
/// happily. A check whose premise expired does not fail; it agrees with you.
const SUPPORTED: &str = "Linux/X11 and macOS";

/// Answer `--version` and `--help` before touching a toolkit.
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
                 Runs on {s}. Requires ffmpeg.\n\
                 {r}",
                v = env!("CARGO_PKG_VERSION"),
                s = SUPPORTED,
                r = env!("CARGO_PKG_REPOSITORY"),
            );
            true
        }
        _ => false,
    }
}

fn main() -> ExitCode {
    if handled_cli() {
        return ExitCode::SUCCESS;
    }
    if handled_reap() {
        return ExitCode::SUCCESS;
    }
    // The binary names itself as the guard to re-run, rather than the library
    // discovering it. `glimpse-core` explains why at `set_reaper_command`: the
    // obvious `current_exe()` inside the library would make `cargo test`
    // re-execute the test harness with `--reap`.
    if let Ok(exe) = std::env::current_exe() {
        glimpse_core::capture::set_reaper_command(exe);
    }
    run()
}

/// `glimpse --reap <parent-pid> <child-pid>` — the guard from
/// [ADR 0019](../docs/adr/0019-a-recording-outlives-a-killed-glimpse.md).
///
/// Answered here, before any toolkit is touched, because this copy of the binary
/// must never open a display, read a config or put up a window. It watches two
/// pids and exits.
///
/// Not in `--help`, deliberately. No user types it, and documenting an internal
/// argument invites someone to.
#[cfg(target_os = "macos")]
fn handled_reap() -> bool {
    match glimpse_macos::reap::args_from(std::env::args()) {
        Some((parent, child)) => {
            glimpse_macos::reap::watch(parent, child);
            true
        }
        None => false,
    }
}

/// Linux has `PR_SET_PDEATHSIG` and never spawns a guard, so nothing should ever
/// arrive here with `--reap`. Refusing to treat it as a normal launch anyway:
/// silently putting up a window because an argument was not understood is how a
/// misrouted invocation becomes a second Glimpse on someone's screen.
#[cfg(not(target_os = "macos"))]
fn handled_reap() -> bool {
    if std::env::args().nth(1).as_deref() == Some("--reap") {
        eprintln!("glimpse: --reap is a macOS-only internal argument");
        return true;
    }
    false
}

#[cfg(target_os = "linux")]
fn run() -> ExitCode {
    glimpse_x11::run()
}

/// One window with the hole inside it, the same shape X11 has. It stops taking
/// clicks for as long as a recording runs, so the user can work in whatever is
/// being recorded, and the menu bar item or the configured hotkey ends it —
/// [ADR 0017](../docs/adr/0017-click-through-is-a-mode-not-a-window.md), which
/// supersedes the multi-window composition of
/// [ADR 0011](../docs/adr/0011-why-the-macos-frame-is-more-than-one-window.md).
#[cfg(target_os = "macos")]
fn run() -> ExitCode {
    glimpse_macos::run()
}

/// Built, but with no window model to run.
///
/// The core compiles and is tested on every platform, which is deliberate — it is
/// how a frontend gets built against something already known to work, and how the
/// core is stopped from quietly re-acquiring a Linux assumption in the meantime.
/// Reaching this message means the core is fine and the frontend is the missing
/// piece.
#[cfg(not(any(target_os = "linux", target_os = "macos")))]
fn run() -> ExitCode {
    eprintln!(
        "glimpse: no framing window is implemented for this platform yet.\n\
         Supported: {SUPPORTED}.\n\
         macOS is tracked at {}/issues/1.",
        env!("CARGO_PKG_REPOSITORY"),
    );
    ExitCode::FAILURE
}
