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
    attach_strips, capture_rect, ignore_mouse_events, place, set_floating, window_nswindow,
};

/// Frame thickness in points. Matches the X11 frontend's border.
pub const BORDER: f64 = 3.0;
/// Chrome height in points, from the design document via ADR 0006.
pub const CHROME_HEIGHT: f64 = 44.0;

/// The frame window paints a border and nothing else: the middle must stay
/// genuinely transparent or it lands in the recording. That is a failure mode
/// the five-window composition could not have, because nothing was over the hole.
const CSS: &str = "
    window.glimpse-frame  { background: transparent; border: 3px solid #4080f5; }
    window.glimpse-chrome { background: #282c33; }
";

/// The two windows, and the hole between them.
pub struct Frame {
    chrome: gtk::Window,
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

        let layout = lay_out(hole, BORDER, CHROME_HEIGHT);
        let chrome = bare_window(app, "glimpse-chrome", layout.chrome);
        let frame = bare_window(app, "glimpse-frame", layout.frame);
        chrome.present();
        frame.present();

        Self {
            chrome,
            frame,
            layout,
        }
    }

    /// Position both windows, make the frame click-through, and bind it to the
    /// chrome so a move carries.
    pub fn realize(&self) -> Result<()> {
        let chrome = window_nswindow(&self.chrome)
            .context("the chrome has no NSWindow yet — realize() ran before GTK mapped it")?;
        place(&chrome, self.layout.chrome);
        set_floating(&chrome);

        let frame = window_nswindow(&self.frame).context("the frame has no NSWindow yet")?;
        place(&frame, self.layout.frame);
        set_floating(&frame);
        // The whole design. Without this the frame swallows every click in the
        // hole, and the user cannot touch the application they are recording.
        ignore_mouse_events(&frame);

        // After placement, not before: `addChildWindow` records the offset that
        // exists at the moment it is called.
        attach_strips(&chrome, std::slice::from_ref(&frame));
        Ok(())
    }

    /// What a recording of this frame would capture.
    pub fn capture_rect(&self, mtm: MainThreadMarker) -> Result<ScreenPixelRect> {
        capture_rect(self.layout.hole, mtm)
    }

    pub fn layout(&self) -> &Layout {
        &self.layout
    }

    pub fn chrome(&self) -> &gtk::Window {
        &self.chrome
    }

    /// Read both windows' positions back from the window server.
    ///
    /// For checking the frame is where it was asked to be rather than where it
    /// was told to go — the two differ whenever something else has an opinion.
    pub fn actual_frames(&self) -> Result<Vec<AppKitRect>> {
        let mut out = Vec::with_capacity(2);
        for w in [&self.chrome, &self.frame] {
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
