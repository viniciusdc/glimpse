//! The macOS framing window: five GTK windows around a hole that is not one.
//!
//! See [ADR 0011](../../docs/adr/0011-why-the-macos-frame-is-more-than-one-window.md).
//! GTK cannot make a covered region click-through on macOS, so nothing is placed
//! over the hole and the click-through comes from there being no window there at
//! all.
//!
//! ## Why placement goes through AppKit
//!
//! GTK4 removed window positioning — there is no `move` on `GtkWindow`. So GTK
//! creates and draws the windows, and every position comes from
//! [`crate::window::place`] via the `NSWindow` underneath. That is the whole
//! reason [`crate::window::window_nswindow`] is a choke point.
//!
//! ## Why positioning is deferred
//!
//! A `GdkSurface` does not exist until the window is mapped, and there is no
//! `NSWindow` before there is a surface. Positioning at construction time
//! silently does nothing: a frame laid out on top of itself at the origin looks
//! like a layout bug rather than a timing one. So [`Frame::realize`] returns an
//! error when called too early rather than quietly achieving nothing.

use anyhow::{Context, Result};
use glimpse_core::geometry::ScreenPixelRect;
use gtk::prelude::*;
use gtk4 as gtk;
use objc2::rc::Retained;
use objc2_app_kit::NSWindow;
use objc2_foundation::MainThreadMarker;

use crate::geometry::AppKitRect;
use crate::layout::{lay_out, Layout};
use crate::window::{attach_strips, capture_rect, place, set_floating, window_nswindow};

/// Frame thickness in points. Matches the X11 frontend's border.
pub const BORDER: f64 = 3.0;
/// Header height in points, from the design document via ADR 0006.
pub const HEADER_HEIGHT: f64 = 44.0;

const CSS: &str = "
    window.glimpse-strip  { background: #4080f5; }
    window.glimpse-header { background: #282c33; }
";

/// The five windows, and the hole they surround.
pub struct Frame {
    header: gtk::Window,
    strips: [gtk::Window; 4],
    layout: Layout,
}

impl Frame {
    /// Build the frame around `hole`, in AppKit coordinates.
    ///
    /// The windows are created and presented here; they are **not** positioned
    /// until [`Frame::realize`] runs, because there is no `NSWindow` to position
    /// until GTK has mapped them.
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

        let layout = lay_out(hole, BORDER, HEADER_HEIGHT);
        let header = bare_window(app, "glimpse-header", layout.header);
        let strips = [
            bare_window(app, "glimpse-strip", layout.strips[0]),
            bare_window(app, "glimpse-strip", layout.strips[1]),
            bare_window(app, "glimpse-strip", layout.strips[2]),
            bare_window(app, "glimpse-strip", layout.strips[3]),
        ];

        for w in std::iter::once(&header).chain(strips.iter()) {
            w.present();
        }

        Self {
            header,
            strips,
            layout,
        }
    }

    /// Position the windows and bind the strips to the header.
    ///
    /// Must run after GTK has mapped them. Returns an error rather than doing
    /// nothing if called too early, because a frame that quietly failed to lay
    /// itself out is indistinguishable from one laid out wrongly.
    pub fn realize(&self) -> Result<()> {
        let header = window_nswindow(&self.header)
            .context("the header has no NSWindow yet — realize() ran before GTK mapped it")?;
        place(&header, self.layout.header);
        set_floating(&header);

        let mut strip_windows: Vec<Retained<NSWindow>> = Vec::with_capacity(4);
        for (i, w) in self.strips.iter().enumerate() {
            let ns =
                window_nswindow(w).with_context(|| format!("strip {i} has no NSWindow yet"))?;
            place(&ns, self.layout.strips[i]);
            set_floating(&ns);
            strip_windows.push(ns);
        }

        // After placement, not before: `addChildWindow` records the offset that
        // exists at the moment it is called, so attaching first would lock in
        // whatever positions GTK happened to choose.
        attach_strips(&header, &strip_windows);
        Ok(())
    }

    /// What a recording of this frame would capture.
    pub fn capture_rect(&self, mtm: MainThreadMarker) -> Result<ScreenPixelRect> {
        capture_rect(self.layout.hole, mtm)
    }

    pub fn layout(&self) -> &Layout {
        &self.layout
    }

    pub fn header(&self) -> &gtk::Window {
        &self.header
    }

    /// Read every window's position back from the window server.
    ///
    /// For checking that the frame is where it was asked to be, rather than
    /// where it was told to go. The two differ whenever something else has an
    /// opinion — a screen edge, a minimum size, a call that arrived too early.
    pub fn actual_frames(&self) -> Result<Vec<AppKitRect>> {
        let mut out = Vec::with_capacity(5);
        for w in std::iter::once(&self.header).chain(self.strips.iter()) {
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
