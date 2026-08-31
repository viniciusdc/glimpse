//! The macOS framing window: two windows, one of which takes no clicks.
//!
//! Per [ADR 0015](../../docs/adr/0015-the-frame-is-two-windows.md). GTK does not
//! inherit the window server's per-pixel alpha hit test on macOS, so a normal
//! window covering the hole would swallow every click. A window with
//! `ignoresMouseEvents` set takes no clicks *anywhere*, which makes it safe to
//! put over the hole — and means all interaction has to come from the chrome.
//!
//! ## Why placement goes through AppKit
//!
//! GTK4 removed window positioning — there is no `move` on `GtkWindow`. So GTK
//! creates and draws the windows and every position comes from
//! [`crate::window::place`] via the `NSWindow` underneath. That is the whole
//! reason [`crate::window::window_nswindow`] is a choke point.
//!
//! ## Why realizing is deferred
//!
//! A `GdkSurface` does not exist until the window is mapped, and there is no
//! `NSWindow` before there is a surface. [`Frame::realize`] returns an error
//! when called too early rather than quietly achieving nothing, because a frame
//! sitting unplaced at the origin reads as a layout bug rather than a timing one.
//!
//! `ignoresMouseEvents` is asynchronous too: it does not take effect within the
//! turn it is set. Anything reading window state back after setting it must pump
//! the run loop first, which is how it was once measured as non-functional.

use anyhow::{Context, Result};
use glimpse_core::geometry::ScreenPixelRect;
use gtk::prelude::*;
use gtk4 as gtk;
use objc2_foundation::MainThreadMarker;

use crate::geometry::AppKitRect;
use crate::layout::{lay_out, Layout};
use crate::window::{
    attach_strips, capture_rect, ignore_mouse_events, move_frame_to, place, set_floating,
    window_nswindow,
};

/// Frame thickness in points. Matches the X11 frontend's border.
pub const BORDER: f64 = 3.0;
/// The HEADER's height in points, from the design document via ADR 0006.
///
/// An initial guess only: GTK sizes the window from its widgets, and `attach_to`
/// anchors the header's BOTTOM edge and lets it grow upward from there.
pub const CHROME_HEIGHT: f64 = 44.0;
/// Initial height of the status window, below the frame.
///
/// Also a guess, and one that changes at runtime: the sheet is hidden until a
/// file is written. `settle_status` re-anchors the window's TOP edge afterwards.
/// See [ADR 0016](../../docs/adr/0016-the-chrome-is-above-and-below.md).
pub const STATUS_HEIGHT: f64 = 34.0;

/// The frame window paints a border and nothing else: the middle must stay
/// genuinely transparent or it lands in the recording. That is a failure mode
/// the five-window composition could not have, because nothing was over the hole.
const CSS: &str = "
    window.glimpse-frame  { background: transparent; border: 3px solid #4080f5; }
    /* Two classes, not one, so this beats `window.glimpse { transparent }` in
       the shared stylesheet on specificity rather than on load order. The shared
       rule is right for X11, where the window must be see-through so the hole
       is; macOS's chrome window contains no hole, so anything the shell does not
       paint would show the desktop through the status bar. */
    window.glimpse.glimpse-chrome { background: #f2f3f5; }
";

/// The frame window, and the layout it shares with the chrome.
///
/// The chrome window is NOT built here any more. It comes from
/// `glimpse_ui::Chrome`, which builds the header, the status bar and the
/// controller that drives them — the same ones X11 uses (ADR 0014).
///
/// This split also fixes an ordering problem. The chrome's `capture_rect` hook
/// has to ask the frame what it would record, so the frame must exist first;
/// but the frame has to be attached to the chrome window, which does not exist
/// until the chrome is built. Building the frame window here and attaching it in
/// [`Frame::attach_to`] breaks the cycle instead of working around it.
pub struct Frame {
    frame: gtk::Window,
    layout: Layout,
}

impl Frame {
    /// Build the frame around `hole`, in AppKit coordinates.
    ///
    /// Presented but not positioned: there is no `NSWindow` until GTK has
    /// mapped them. Call [`Frame::realize`] after a turn of the main loop.
    pub fn new(app: &gtk::Application, hole: AppKitRect) -> Self {
        let provider = gtk::CssProvider::new();
        provider.load_from_data(CSS);
        if let Some(display) = gtk::gdk::Display::default() {
            gtk::style_context_add_provider_for_display(
                &display,
                &provider,
                gtk::STYLE_PROVIDER_PRIORITY_APPLICATION,
            );
        }

        let layout = lay_out(hole, BORDER, CHROME_HEIGHT, STATUS_HEIGHT);
        let frame = bare_window(app, "glimpse-frame", layout.frame);
        frame.present();

        Self { frame, layout }
    }

    /// Position both windows, make the frame click-through, and bind it to the
    /// chrome so a move carries.
    ///
    /// Takes the chrome window rather than owning it: it belongs to
    /// `glimpse_ui::Chrome`, which knows nothing about `NSWindow`.
    pub fn attach_to(
        &self,
        chrome_window: &gtk::Window,
        status_window: &gtk::Window,
    ) -> Result<()> {
        let chrome = window_nswindow(chrome_window)
            .context("the chrome has no NSWindow yet — attach_to ran before GTK mapped it")?;

        // ORIGIN ONLY, deliberately. The chrome's height is GTK's business: it is
        // whatever the shared widgets need, and those are not ours to predict
        // from here. Setting a size would be a guess that GTK immediately
        // overrides — measured at 78pt against a CHROME_HEIGHT of 44, which is
        // ADR 0006's HEADER height and stopped describing the whole chrome the
        // moment the status bar arrived with it.
        //
        // Positioning the bottom-left corner is what actually matters: AppKit
        // grows a window upward from its origin, so gluing that corner to the
        // frame's top edge keeps them flush at any height.
        move_frame_to(
            &chrome,
            objc2_foundation::NSPoint::new(self.layout.chrome.x, self.layout.chrome.y),
        );
        set_floating(&chrome);

        let frame = window_nswindow(&self.frame).context("the frame has no NSWindow yet")?;
        place(&frame, self.layout.frame);
        set_floating(&frame);
        // The whole design. Without this the frame swallows every click in the
        // hole, and the user cannot touch the application they are recording.
        ignore_mouse_events(&frame);

        // Below the frame, anchored by its TOP edge so it grows downward when
        // the sheet appears rather than climbing over the recording area
        // (ADR 0016). Re-applied by `settle_status` once GTK has sized it.
        let status =
            window_nswindow(status_window).context("the status bar has no NSWindow yet")?;
        move_frame_to(
            &status,
            objc2_foundation::NSPoint::new(self.layout.status.x, self.layout.status.y),
        );
        set_floating(&status);

        // After placement, not before: `addChildWindow` records the offset that
        // exists at the moment it is called. Both children attach to the header,
        // so one drag carries all three.
        attach_strips(&chrome, &[frame, status]);
        Ok(())
    }

    /// What a recording of this frame would capture.
    pub fn capture_rect(&self, mtm: MainThreadMarker) -> Result<ScreenPixelRect> {
        capture_rect(self.layout.hole, mtm)
    }

    pub fn layout(&self) -> &Layout {
        &self.layout
    }

    /// The frame window, for callers that need to read it back.
    pub fn window(&self) -> &gtk::Window {
        &self.frame
    }

    /// Read both windows' positions back from the window server.
    ///
    /// For checking the frame is where it was asked to be rather than where it
    /// was told to go — the two differ whenever something else has an opinion.
    /// Re-anchor the status window's TOP edge to the frame's bottom.
    ///
    /// GTK sizes the window from its widgets, and the sheet changes that at
    /// runtime. AppKit keeps the origin and grows upward, so a taller status
    /// window climbs over the recording area unless its origin moves down by the
    /// difference. Called once GTK has settled, and again when the sheet appears.
    pub fn settle_status(&self, status_window: &gtk::Window) -> Result<()> {
        let status = window_nswindow(status_window).context("the status bar has no NSWindow")?;
        let got = crate::window::appkit_frame(&status);
        let want_y = self.layout.frame.y - got.h;
        if (got.y - want_y).abs() >= 0.5 {
            // Only when it actually moved, so the log shows the sheet appearing
            // rather than every layout pass.
            println!(
                "glimpse: status re-anchored {}x{} y {} -> {} (frame bottom {})",
                got.w as i64, got.h as i64, got.y as i64, want_y as i64, self.layout.frame.y as i64,
            );
        }
        move_frame_to(
            &status,
            objc2_foundation::NSPoint::new(self.layout.status.x, want_y),
        );
        Ok(())
    }

    /// The status window's frame, read back from the window server.
    pub fn status_frame(&self, status_window: &gtk::Window) -> Result<AppKitRect> {
        let ns = window_nswindow(status_window)?;
        Ok(crate::window::appkit_frame(&ns))
    }

    pub fn actual_frames(&self, chrome_window: &gtk::Window) -> Result<Vec<AppKitRect>> {
        let mut out = Vec::with_capacity(2);
        for w in [chrome_window, &self.frame] {
            let ns = window_nswindow(w)?;
            out.push(crate::window::appkit_frame(&ns));
        }
        Ok(out)
    }
}

/// An undecorated, unresizable window with a CSS class and a size.
///
/// Only the size is set here. GTK4 has no positioning API, so the position is
/// applied later through AppKit.
fn bare_window(app: &gtk::Application, class: &str, r: AppKitRect) -> gtk::Window {
    let w = gtk::Window::builder()
        .application(app)
        .decorated(false)
        .resizable(false)
        .default_width(r.w as i32)
        .default_height(r.h as i32)
        .build();
    w.add_css_class(class);
    w
}
