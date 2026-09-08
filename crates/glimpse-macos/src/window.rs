//! The macOS platform boundary: reaching through GTK to the `NSWindow`, and the
//! window-server operations the frame composition needs.
//!
//! GTK will not tell an application where it is on screen, and on macOS it will
//! not compose the frame either — see
//! [ADR 0011](../../docs/adr/0011-why-the-macos-frame-is-more-than-one-window.md).
//! So the frame is several windows and this module is where they are wired
//! together.

use anyhow::{anyhow, Result};
use gdk4_macos::prelude::*;
use gdk4_macos::MacosSurface;
use glimpse_core::geometry::ScreenPixelRect;
use gtk::prelude::*;
use gtk4 as gtk;
use objc2::rc::Retained;
use objc2_app_kit::{NSScreen, NSWindow, NSWindowOrderingMode};
use objc2_foundation::{MainThreadMarker, NSPoint, NSRect};

use crate::geometry::{to_screen_pixels, AppKitRect};

/// The `NSWindow` backing a GTK window.
///
/// **This is a deliberate choke point**, for the same reason
/// `glimpse_x11::x11probe::window_xid` is one: it is the single place where the
/// toolkit is reached past. Everything below builds on it, so the day GDK changes
/// how the native window is exposed, one function changes.
///
/// `GdkMacosSurface::get_native_window` is the documented accessor and is what
/// `gdk4-macos`'s `native()` calls under the `v4_8` feature. It hands back a raw
/// pointer with no ownership transfer, so the reference is retained here rather
/// than assumed.
pub fn window_nswindow(window: &gtk::Window) -> Result<Retained<NSWindow>> {
    let surface = window
        .surface()
        .ok_or_else(|| anyhow!("window has no surface — not realized yet"))?;
    let macos = surface.downcast_ref::<MacosSurface>().ok_or_else(|| {
        anyhow!(
            "not a macOS surface — GTK is using the {} backend",
            surface.display().type_().name()
        )
    })?;
    let ptr = macos.native();
    if ptr.is_null() {
        return Err(anyhow!("GDK reported a null native window"));
    }
    // SAFETY: `get_native_window` returns a borrowed NSWindow owned by GDK for
    // the lifetime of the surface. `retain` takes our own reference rather than
    // assuming GDK's outlives this one.
    unsafe { Retained::retain(ptr.cast::<NSWindow>()) }
        .ok_or_else(|| anyhow!("could not retain the native window"))
}

/// Make `strips` follow `header` as one object.
///
/// AppKit maintains the relative offset itself, measured: moving only the parent
/// moved every child by exactly the same delta, with a non-child control that the
/// same check correctly reported as not following. So the frame cannot shear on a
/// move, and there is no per-strip bookkeeping to get wrong.
///
/// What it does **not** do is propagate a resize — see [`move_frame_to`].
pub fn attach_strips(header: &NSWindow, strips: &[Retained<NSWindow>]) {
    for strip in strips {
        // SAFETY: both windows are live main-thread NSWindows, and the caller
        // holds a strong reference to each strip for at least as long as the
        // header.
        unsafe { header.addChildWindow_ordered(strip, NSWindowOrderingMode::Above) };
    }
}

/// Move the whole frame by moving its parent.
///
/// **Uses `setFrameOrigin:` and must keep doing so.** Propagation depends on
/// which call is made, measured:
///
/// | call on the parent | children follow? |
/// |---|---|
/// | `setFrameOrigin:` | yes, by exactly the delta |
/// | `setFrame:display:` carrying size **and** origin | no, not even the origin part |
///
/// So a "tidy" refactor that folds this into a single `setFrame:` alongside a
/// resize silently stops the strips tracking, and the frame shears. The
/// relationship is not broken by that call — a `setFrameOrigin:` immediately
/// afterwards still propagates — which is why the failure looks like a rendering
/// glitch rather than a wiring bug.
pub fn move_frame_to(header: &NSWindow, origin: NSPoint) {
    header.setFrameOrigin(origin);
}

/// The capture rectangle for a hole bounded by the frame's inner edges.
///
/// Takes the `NSWindow` frames rather than GTK widget bounds because on macOS the
/// hole is not inside any window — it is the gap between them, and no widget
/// describes it.
pub fn capture_rect(hole: AppKitRect, mtm: MainThreadMarker) -> Result<ScreenPixelRect> {
    // AppKit coordinates are always relative to the PRIMARY screen, whichever
    // display the window is actually on, so the primary screen's height is the
    // right one to flip against even for a frame dragged onto a second monitor.
    let primary = NSScreen::screens(mtm)
        .iter()
        .next()
        .ok_or_else(|| anyhow!("no screens"))?;
    let height = primary.frame().size.height;
    // Only the integer-ish backing factor is trusted, never a figure derived
    // from a monitor's reported physical size.
    let scale = primary.backingScaleFactor();
    Ok(to_screen_pixels(hole, height, scale))
}

/// Place and size a window directly.
///
/// Used for the strips, whose geometry is computed rather than inherited. GTK4
/// removed window positioning entirely — there is no `move` on `GtkWindow` — so
/// on macOS every strip's position comes through here.
///
/// Setting a **child's** own frame is fine and is not the hazard documented on
/// [`move_frame_to`]. That one is about the *parent* carrying an origin change
/// while children are attached to it, which stops the children tracking.
pub fn place(window: &NSWindow, r: AppKitRect) {
    window.setFrame_display(
        NSRect::new(
            NSPoint::new(r.x, r.y),
            objc2_foundation::NSSize::new(r.w, r.h),
        ),
        true,
    );
}

/// Stop a window casting a drop shadow.
///
/// The chrome sits directly above the recording area, and AppKit draws its
/// shadow *downward* — into the region being captured. Measured against the same
/// screen with the app closed, the top 40 device pixels came out up to 4.4
/// luminance darker, tapering to exactly zero by row 40:
///
/// ```text
///    y   with-app  without   delta
///     0     13.5     17.9    -4.4
///    20     15.1     17.1    -2.0
///    40     17.1     17.1    +0.0
/// ```
///
/// The capture rect is correct, so no geometry check can see this. It is only
/// visible in the image, which is the failure ADR 0000 exists to record — and it
/// would have shipped in every macOS recording.
///
/// LICEcap does the same thing for the same reason: `SWELL_SetWindowShadow(hwnd,
/// false)` in `capturewindow.mm`.
pub fn drop_shadow_off(window: &NSWindow) {
    window.setHasShadow(false);
}

/// Lift a window above ordinary windows.
///
/// `kCGFloatingWindowLevel`. Not decoration: a framing window that sits at the
/// normal level cannot be placed over what the user wants to record, because
/// whatever they are recording is in front of it. The frame is then still on
/// screen and still correct, and a capture of its hole shows the wrong
/// application — which reads as a geometry bug rather than a stacking one.
///
/// Found exactly that way. The frame reported the right rectangle, every window
/// was where the layout asked, and a grab of the hole came back showing the
/// terminal that had focus.
pub fn set_floating(window: &NSWindow) {
    const FLOATING: isize = 3;
    window.setLevel(FLOATING);
}

/// How translucent the window goes while it is passing clicks through.
///
/// A window that silently stopped accepting clicks reads as a frozen
/// application. The dim is what makes "not interactive" visible.
///
/// Measured not to reach the hole: dimming moved a header band by −10.97 against
/// a fixed backdrop while the hole stayed at exactly the backdrop's value
/// ([ADR 0017](../../docs/adr/0017-click-through-is-a-mode-not-a-window.md)). The
/// arithmetic said as much — the hole is already at alpha 0 — but PR #40 is why
/// compositing arguments get measured here rather than reasoned about.
const PASSTHROUGH_ALPHA: f64 = 0.8;

/// Turn whole-window click-through on or off.
///
/// `ignoresMouseEvents` is whole-window, which is why issue #1 and
/// [ADR 0011](../../docs/adr/0011-why-the-macos-frame-is-more-than-one-window.md)
/// both dismissed it: it cannot describe a hole. That was the wrong conclusion
/// twice over. [ADR 0015](../../docs/adr/0015-the-frame-is-two-windows.md) found
/// that a purely decorative window wants exactly whole-window click-through, and
/// [ADR 0017](../../docs/adr/0017-click-through-is-a-mode-not-a-window.md) found
/// that so does an *application*, for as long as the user is working in whatever
/// sits behind the frame.
///
/// **It does not take effect within the turn it is set.** The window server
/// processes it asynchronously, so anything reading window state back must pump
/// the run loop first. Measuring without that pump once produced a confident
/// "the flag does nothing".
///
/// That asynchrony is survivable here only because this is called on a state
/// transition and not on every pointer crossing — once, at a moment when there
/// is no click in flight to lose.
pub fn set_passthrough(window: &NSWindow, on: bool) {
    window.setIgnoresMouseEvents(on);
    window.setAlphaValue(if on { PASSTHROUGH_ALPHA } else { 1.0 });
}

/// What a recording would capture: where the hole widget is, on screen, now.
///
/// **Computed from the widget on every call, never cached.** The three-window
/// design answered this from a `Layout` built once at startup, so after the
/// frame was dragged it reported — and recorded — the rectangle the frame
/// started at, and `geometry_drifted` could not see the error because both sides
/// read the same stale value. Deriving it from `compute_bounds` gives the
/// staleness nowhere to live, and is what X11 has always done.
///
/// The hole's bounds are its *border box*, and the frame is painted by the
/// parent, so no inset is applied here. Adding one would reintroduce a fixed bug
/// (AGENTS.md, [ADR 0000](../../docs/adr/0000-x11-framing-window-spike.md)).
pub fn capture_rect_of(
    window: &gtk::Window,
    hole: &gtk::Box,
    mtm: MainThreadMarker,
) -> Result<ScreenPixelRect> {
    let b = hole
        .compute_bounds(window)
        .ok_or_else(|| anyhow!("the hole has no bounds yet — not laid out"))?;
    // Widget coordinates are relative to the window widget; the NSWindow frame
    // describes the surface. The two differ by the client-side-decoration
    // margin, and applying the transform to only one of them is how a punched
    // hole drifts from a visible one.
    let (tx, ty) = window.surface_transform();
    let ns = window_nswindow(window)?;
    let f = appkit_frame(&ns);
    capture_rect(
        AppKitRect {
            x: f.x + b.x() as f64 + tx,
            // Widget y counts down from the window's top; AppKit counts up from
            // the screen's bottom. One flip, here.
            y: f.y + f.h - (b.y() as f64 + ty) - b.height() as f64,
            w: b.width() as f64,
            h: b.height() as f64,
        },
        mtm,
    )
}

/// The AppKit frame of a window, as the flip expects it.
pub fn appkit_frame(window: &NSWindow) -> AppKitRect {
    let f = window.frame();
    AppKitRect {
        x: f.origin.x,
        y: f.origin.y,
        w: f.size.width,
        h: f.size.height,
    }
}
