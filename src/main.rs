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
    run()
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
