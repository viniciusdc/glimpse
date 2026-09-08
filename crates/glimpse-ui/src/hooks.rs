//! What the chrome needs from a platform, and nothing more.
//!
//! The shape is decided in
//! [ADR 0014](../../../docs/adr/0014-the-chrome-is-shared-the-window-model-is-not.md):
//! a struct of closures rather than a trait. Both frontends are selected at
//! compile time, so a trait would buy no dispatch, and its only other effect
//! would be to make the chrome generic over a parameter with exactly one value
//! per binary. This is the same shape as `GrabCommand` — the platform hands over
//! something describing what to do, and the shared code does it.
//!
//! **Every hook here exists because a specific line in `ui.rs` needs it.**
//! Nothing was added for symmetry, or for a platform that does not exist yet.
//! The count is the argument: `ui.rs` is 1948 lines and 12 of them name X11, so
//! what follows is a seam, not a platform abstraction.

use anyhow::Result;
use glimpse_core::capture::{GrabCommand, GrabRequest};
use glimpse_core::geometry::ScreenPixelRect;

/// What a recording would capture, right now.
pub type CaptureRectFn = Box<dyn Fn() -> Result<ScreenPixelRect>>;
/// A request turned into the backend's ffmpeg invocation.
pub type GrabFn = Box<dyn Fn(&GrabRequest) -> Result<GrabCommand>>;
/// Called when the frame's geometry has settled.
pub type GeometrySettledFn = Box<dyn Fn()>;
/// Platform diagnostics for the self-test report, as whole lines of text.
pub type DiagnosticsFn = Box<dyn Fn() -> String>;
/// Turn click-through on or off for as long as the user must reach what is
/// behind the frame.
pub type SetPassthroughFn = Box<dyn Fn(bool)>;
/// What stops a recording while the chrome cannot be clicked, if anything does.
pub type StopHintFn = Box<dyn Fn() -> Option<String>>;

/// The platform half of the chrome.
///
/// Held by the controller and called from GTK callbacks, so every closure is
/// `'static` and takes `&self` by way of captured state rather than parameters.
pub struct PlatformHooks {
    /// What a recording would capture, right now, in global device pixels with a
    /// top-left origin.
    ///
    /// The two platforms answer this from entirely different places and that is
    /// the point of the hook. X11 walks widget → surface → root using the probe
    /// it holds as a field. macOS reads the frame window's `NSWindow` frame and
    /// applies the AppKit flip. Neither computation is expressible in the other's
    /// terms, but both produce the same type, which is what makes the chrome
    /// above them identical.
    pub capture_rect: CaptureRectFn,

    /// Turn a request into the backend's ffmpeg invocation.
    ///
    /// Returns `Result` because discovering the backend can fail and the failure
    /// is worth showing: X11 needs a display it can name, and macOS has to find
    /// the screen device behind the cameras in `-list_devices`.
    ///
    /// This is deliberately the *only* way the chrome obtains ffmpeg arguments.
    /// `grab_through_the_shipping_path` in `ui.rs` documents what happened the
    /// one time a second copy existed: it passed the region inside the input URL
    /// and named no codec, so ffmpeg inferred one from the file extension and
    /// wrote a JPEG into a `.png`. A check built on its own copy of the arguments
    /// can only ever vouch for that copy.
    pub grab: GrabFn,

    /// The frame's geometry has settled.
    ///
    /// X11 re-punches its input region here, because its hole is a region of a
    /// window that takes clicks everywhere else. macOS does nothing: its frame
    /// window sets `ignoresMouseEvents` once and takes no clicks anywhere, ever
    /// ([ADR 0015](../../../docs/adr/0015-the-frame-is-two-windows.md)).
    ///
    /// ADR 0014 described this hook as the place macOS "repositions its strips".
    /// There are no strips — that was the five-window composition ADR 0015
    /// superseded. The hook survives the change because it was defined by what
    /// X11 needs, not by the design that replaced it on the other side.
    pub geometry_settled: GeometrySettledFn,

    /// The user needs to reach whatever is behind the frame — or has stopped
    /// needing to.
    ///
    /// **Both entries below exist because this crate's own rule was applied, not
    /// waived.** The header of this file says every hook exists because a
    /// specific line needs it and nothing was added for symmetry, so two
    /// additions want naming: `refresh` has to make the window stop taking
    /// clicks when a recording starts, and it has to render something other than
    /// a dead button in the place the button would go. Neither is expressible in
    /// the four hooks that were already here.
    ///
    /// X11 does nothing: its hole is punched out of the input region and passes
    /// clicks at all times, recording or not. macOS turns the whole window
    /// click-through and dims it, because the middle of a GTK window cannot be
    /// made click-through there and the mode is the way around that
    /// ([ADR 0017](../../../docs/adr/0017-click-through-is-a-mode-not-a-window.md)).
    ///
    /// Called only on a change, never on every `refresh`. On macOS this reaches
    /// the window server, and the flag it sets is asynchronous.
    pub set_passthrough: SetPassthroughFn,

    /// What to show where the action button goes, while passthrough is on.
    ///
    /// `None` means the button still works and should be shown — X11's answer,
    /// always, because its chrome keeps taking clicks while it records.
    ///
    /// `Some(hint)` means the button is unreachable and this is what stops the
    /// recording instead. Showing the button anyway would be
    /// [ADR 0012](../../../docs/adr/0012-a-setting-a-backend-cannot-honour.md)'s
    /// failure exactly: a control that is visible, looks live, and does nothing.
    ///
    /// **A closure rather than a string, because the answer is not fixed.** The
    /// shortcut is configurable, so a literal would disagree with the binding as
    /// soon as anyone changed it; and registration can fail outright when
    /// another application already owns the combination, in which case the
    /// honest answer names something else that does work. A hint is only worth
    /// showing if it is rendered from state known to be live.
    pub stop_hint: StopHintFn,

    /// Platform diagnostics for the self-test, as free text.
    ///
    /// Text, and not anything structured, because the two platforms have nothing
    /// to say to each other here. X11 prints an X window id, the shape bands read
    /// back from the server, and an `xwininfo` cross-check. macOS has no shape to
    /// read, because nothing over its hole takes clicks in the first place.
    /// Inventing a common vocabulary for two unrelated facts would produce a
    /// structure that neither platform fills honestly.
    pub diagnostics: DiagnosticsFn,

    /// Whether this backend actually honours the Capture-pointer setting.
    ///
    /// A plain fact rather than a closure: it is fixed per platform and does not
    /// depend on anything that changes at runtime.
    ///
    /// False on macOS. avfoundation ignores `-capture_cursor`, measured, and in
    /// the OFF direction — the pointer is never drawn. So the switch would flip,
    /// persist across restarts and change nothing, which
    /// [ADR 0012](../../../docs/adr/0012-a-setting-a-backend-cannot-honour.md)
    /// calls the same failure as a file that lies about its contents, and
    /// decides such a setting is **not offered** on that platform.
    ///
    /// That record noted the timing was free because "there is no macOS frontend
    /// yet, so this is a line in a UI that has not been written". The frontend
    /// now exists and runs this very chrome, so the line has to be written.
    pub honours_pointer_capture: bool,
}

impl PlatformHooks {
    /// Hooks that answer nothing, for tests that exercise the chrome's logic
    /// without a platform under it.
    ///
    /// `capture_rect` and `grab` fail rather than returning a plausible default.
    /// A stub rectangle would let a test pass while the chrome recorded the wrong
    /// region, which is the failure ADR 0000 exists to record and the one this
    /// project has already shipped once.
    pub fn unavailable() -> Self {
        Self {
            capture_rect: Box::new(|| anyhow::bail!("no platform: capture_rect is unavailable")),
            grab: Box::new(|_| anyhow::bail!("no platform: grab is unavailable")),
            geometry_settled: Box::new(|| {}),
            set_passthrough: Box::new(|_| {}),
            // Not `Some("...")`: a stub that named a stop path would put a
            // shortcut in front of a user that nothing is listening for.
            stop_hint: Box::new(|| None),
            diagnostics: Box::new(|| "no platform".to_string()),
            honours_pointer_capture: false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_stub_refuses_rather_than_inventing_a_rectangle() {
        // A stub that returned Ok(some rect) would let a test of the recording
        // path pass while the rectangle was fictional. Refusing is the only
        // answer that cannot be mistaken for a working platform.
        let h = PlatformHooks::unavailable();
        assert!((h.capture_rect)().is_err());
        assert!((h.grab)(&GrabRequest {
            rect: ScreenPixelRect {
                x: 0,
                y: 0,
                w: 1,
                h: 1
            },
            framerate: None,
            capture_mouse: false,
        })
        .is_err());
    }

    #[test]
    fn a_backend_that_cannot_honour_pointer_capture_says_so() {
        // The chrome hides the switch on this, so the default matters: a stub
        // claiming it works would put a dead control in front of a user.
        assert!(!PlatformHooks::unavailable().honours_pointer_capture);
    }

    #[test]
    fn geometry_settled_is_allowed_to_do_nothing() {
        // macOS's implementation is genuinely empty, so the stub being a no-op is
        // the real shape rather than a placeholder.
        (PlatformHooks::unavailable().geometry_settled)();
    }

    #[test]
    fn hooks_are_callable_through_a_shared_reference() {
        // The controller holds these behind Rc and calls them from GTK callbacks,
        // so anything requiring &mut self would not compile there. Proving it
        // here keeps that constraint visible in this crate rather than surfacing
        // as a borrow error 1900 lines away.
        let h = std::rc::Rc::new(PlatformHooks::unavailable());
        let h2 = h.clone();
        let call = move || (h2.diagnostics)();
        assert_eq!(call(), "no platform");
        assert_eq!((h.diagnostics)(), "no platform");
    }
}
