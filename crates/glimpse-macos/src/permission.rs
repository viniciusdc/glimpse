//! Ask macOS whether Glimpse may record the screen, before trying to.
//!
//! Without this the first sign of a missing permission is ffmpeg failing to open
//! the avfoundation device, which reads exactly like a bug in the app — and the
//! error `AvfCapture::discover` produces says so only because it *guesses* from
//! an empty device list. The system will answer the question directly.
//!
//! ## The two calls, and why both
//!
//! `CGPreflightScreenCaptureAccess` reports the current state and **never
//! prompts**. `CGRequestScreenCaptureAccess` prompts if the answer is not
//! already recorded, and returns what it then knows. Preflight is what a
//! start-up report should use, because an application that throws a system
//! dialog at you for opening it has misjudged something. Request belongs at the
//! moment the user asks to record, where a prompt is the expected thing.
//!
//! ## Granting does not take effect in the running process
//!
//! TCC decides at process start. A user who grants at the prompt still has to
//! restart Glimpse before a recording works, and an app that does not say so
//! leaves them pressing Record and watching it fail with permission already
//! granted. The message says it.
//!
//! ## Who the grant attaches to
//!
//! The *responsible process*. Launched from Finder as `Glimpse.app` that is
//! Glimpse, and the grant persists against its bundle identifier
//! ([ADR 0013](../../docs/adr/0013-macos-ships-an-app-bundle.md)). Run as a bare
//! binary from a terminal it is the terminal — so granting works, applies to
//! every program that terminal ever launches, and has to be done again from a
//! different one. That is worth explaining rather than leaving as a surprise.

// SAFETY of the block below: both are plain CoreGraphics predicates taking no
// arguments and returning a Boolean. Neither has a failure mode expressible in
// Rust, and neither touches memory we own.
#[link(name = "CoreGraphics", kind = "framework")]
extern "C" {
    fn CGPreflightScreenCaptureAccess() -> bool;
    fn CGRequestScreenCaptureAccess() -> bool;
}

/// Is screen recording permitted right now? Never prompts.
pub fn granted() -> bool {
    // SAFETY: see the extern block.
    unsafe { CGPreflightScreenCaptureAccess() }
}

/// Ask for it, prompting if macOS has no answer recorded yet.
///
/// Returns whether it is granted *now*. A `true` here on a process that started
/// without the permission still does not mean a recording will work — see the
/// module note on TCC deciding at process start.
pub fn request() -> bool {
    // SAFETY: see the extern block.
    unsafe { CGRequestScreenCaptureAccess() }
}

/// What to tell someone who cannot record, in the place they will read it.
///
/// Names the setting, the path to it, and the restart — the three things that
/// are not guessable. It deliberately does not say "check your permissions",
/// which is the kind of message that sends people to the wrong panel.
pub fn explain() -> String {
    let bundled = std::env::current_exe()
        .ok()
        .and_then(|p| Some(p.parent()?.parent()?.file_name()? == "Contents"))
        .unwrap_or(false);

    let who = if bundled {
        "Glimpse"
    } else {
        // Being precise about this is the whole reason the bundle exists. A
        // user who grants "Terminal" and then runs Glimpse from a different one
        // is back where they started, with no indication why.
        "the terminal you started Glimpse from — macOS grants this to whatever \
         launched the program, not to the binary"
    };

    format!(
        "Screen Recording permission is not granted to {who}.\n\
         Grant it in System Settings > Privacy & Security > Screen & System Audio \
         Recording, then restart Glimpse — macOS only reads the permission when a \
         program starts, so granting it will not affect this one."
    )
}

#[cfg(test)]
mod tests {
    #[test]
    fn the_explanation_names_the_setting_and_the_restart() {
        // The three things that are not guessable. A message missing any of them
        // sends someone to the wrong panel or leaves them pressing Record after
        // granting, which is the failure this exists to prevent.
        let e = super::explain();
        assert!(e.contains("System Settings"), "{e}");
        assert!(e.contains("Screen & System Audio Recording"), "{e}");
        assert!(e.contains("restart"), "{e}");
    }

    #[test]
    fn preflight_does_not_panic_and_answers() {
        // Whatever the answer is on this machine, asking must be safe: it runs
        // on every start-up, including in CI where the answer is false.
        let _ = super::granted();
    }
}
