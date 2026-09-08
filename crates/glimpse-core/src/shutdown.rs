//! Noticing `Ctrl-C`, so the recorder gets a chance to reap its child.
//!
//! `Recorder` kills and waits for ffmpeg on every exit path it controls,
//! including a `Drop` backstop. None of those run on a signal: the default
//! disposition of `SIGINT` and `SIGTERM` is to end the process, and destructors
//! do not happen.
//!
//! On Linux that costs a temp directory, which the next start-up sweep collects,
//! because `PR_SET_PDEATHSIG` has already taken ffmpeg down with its parent. Off
//! Linux [`crate::capture`]'s `die_with_parent` is a no-op, so the same
//! `Ctrl-C` leaves an ffmpeg **recording forever**, holding the screen capture
//! device — and the next recording then gets no frames, never reaches the point
//! where ffmpeg reads `q`, and is killed at the graceful-stop timeout. That is
//! issue #45, and `Ctrl-C` was documented as the way to quit.
//!
//! ## A flag, not the work
//!
//! The handler does one thing: store `true`. It may not allocate, lock, or touch
//! GTK — a signal can arrive on any thread, between any two instructions, and
//! async-signal-safety is not advice. The frontend polls [`requested`] from its
//! main loop and quits normally there, which is what lets every destructor run.

use std::sync::atomic::{AtomicBool, Ordering};

static REQUESTED: AtomicBool = AtomicBool::new(false);

/// SAFETY: this is a signal handler. Storing to an `AtomicBool` is
/// async-signal-safe; nothing else here may be added without checking that it
/// is too.
#[cfg(unix)]
extern "C" fn on_signal(_sig: libc::c_int) {
    REQUESTED.store(true, Ordering::SeqCst);
}

/// Ask for `SIGINT` and `SIGTERM` to set the flag instead of ending the process.
///
/// Idempotent, and safe to call before anything else exists.
///
/// **The default disposition is restored for a second signal**, so a frontend
/// that hangs on the way out can still be killed with a second `Ctrl-C`. A
/// program that becomes unkillable because it wanted to tidy up is a worse
/// program than one that leaks a temp directory.
#[cfg(unix)]
pub fn install() {
    // SAFETY: `signal` with a plain extern "C" handler for two catchable
    // signals. `SA_RESETHAND` semantics are what `signal` gives on macOS via
    // `libc::signal`, so the second signal ends the process the usual way.
    unsafe {
        libc::signal(libc::SIGINT, on_signal as *const () as libc::sighandler_t);
        libc::signal(libc::SIGTERM, on_signal as *const () as libc::sighandler_t);
    }
}

#[cfg(not(unix))]
pub fn install() {}

/// Has a shutdown signal arrived since start-up?
///
/// Polled from the frontend's main loop. Latching rather than consuming: once
/// the user has asked to quit, every later read should agree.
pub fn requested() -> bool {
    REQUESTED.load(Ordering::SeqCst)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Both halves in one test, deliberately.
    ///
    /// The flag is process-global and cargo runs tests in parallel threads of
    /// one process, so a separate "starts false" test passes or fails depending
    /// on which test ran first. Written as two, it failed exactly that way on
    /// the first run.
    #[cfg(unix)]
    #[test]
    fn a_signal_sets_the_flag_without_ending_the_process() {
        // A frontend polling this on its first tick must not quit immediately.
        assert!(!requested(), "nothing should be requested before a signal");

        install();
        // SAFETY: signalling our own process with a signal we just installed a
        // handler for. If `install` were wrong this test would not fail — the
        // process would die and take the whole run with it.
        unsafe { libc::raise(libc::SIGTERM) };
        assert!(requested(), "SIGTERM should have set the flag");
    }
}
