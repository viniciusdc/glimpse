//! The framing window: you place it over what you want to record, and its hole
//! IS the capture region.
//!
//! Layout, and the reason for it:
//!
//! ```text
//! window            transparent
//!   toolbar         opaque controls
//!   frame           paints the 3px border  <-- the ONLY thing that draws
//!     hole          paints nothing         <-- the capture target
//!   status
//! ```
//!
//! The frame border lives on `frame`, never on `hole`. `compute_bounds` returns a
//! widget's *border box*, so a border on the capture target would be recorded as
//! part of every GIF. The T-0367 spike hit exactly that, and it survived an
//! `xwininfo` cross-check. Separating the two removes the bug class instead of
//! compensating for it with a magic inset.

use anyhow::{anyhow, Result};
use gdk4_x11::X11Surface;
use gtk::prelude::*;
use gtk::{cairo, glib};
use gtk4 as gtk;
use std::cell::{Cell, RefCell};
use std::rc::Rc;

use crate::geometry::{capture_rect, RootPixelRect};
use crate::x11probe::X11Probe;

const CSS: &str = "
window.glimpse { background: transparent; }
.glimpse-frame { border: 3px solid @accent_bg_color; border-radius: 2px; }
.glimpse-hole  { background: transparent; }
.glimpse-chrome { background: rgba(30,30,30,0.92); border-radius: 6px; }
";

/// Raw X11 window id for a GTK window.
///
/// Uses `GdkX11Surface::xid`, which is deprecated since GTK 4.18 with no
/// documented replacement (ADR 0001). It works on 4.14.5. This function is the
/// single choke point so that when it breaks, exactly one place changes.
pub fn window_xid(window: &gtk::Window) -> Result<u32> {
    let surface = window
        .surface()
        .ok_or_else(|| anyhow!("window has no surface"))?;
    let x11 = surface
        .downcast_ref::<X11Surface>()
        .ok_or_else(|| anyhow!("not an X11 surface — Glimpse v0.1 is X11-only (ADR 0002)"))?;
    Ok(x11.xid() as u32)
}

pub struct FramingWindow {
    pub window: gtk::ApplicationWindow,
    hole: gtk::Box,
    probe: Rc<X11Probe>,
    /// Set while arming/recording. The rect must not move under the recorder.
    locked: Rc<Cell<bool>>,
    /// The rect snapshotted when locking, per D-0058.
    frozen: Rc<RefCell<Option<RootPixelRect>>>,
}

impl FramingWindow {
    pub fn new(app: &gtk::Application, probe: Rc<X11Probe>) -> Self {
        let css = gtk::CssProvider::new();
        css.load_from_data(CSS);
        if let Some(display) = gtk::gdk::Display::default() {
            gtk::style_context_add_provider_for_display(
                &display,
                &css,
                gtk::STYLE_PROVIDER_PRIORITY_APPLICATION,
            );
        }

        let window = gtk::ApplicationWindow::builder()
            .application(app)
            .title("Glimpse")
            .default_width(760)
            .default_height(520)
            .build();
        window.add_css_class("glimpse");

        let hole = gtk::Box::new(gtk::Orientation::Vertical, 0);
        hole.add_css_class("glimpse-hole");
        hole.set_hexpand(true);
        hole.set_vexpand(true);

        let frame = gtk::Box::new(gtk::Orientation::Vertical, 0);
        frame.add_css_class("glimpse-frame");
        frame.set_hexpand(true);
        frame.set_vexpand(true);
        frame.append(&hole);

        let status = gtk::Label::new(Some("Position the frame, then Record."));
        status.add_css_class("dim-label");
        status.set_margin_top(4);
        status.set_margin_bottom(4);

        let record = gtk::Button::with_label("Record");
        record.add_css_class("suggested-action");
        let show_rect = gtk::Button::with_label("Show capture rect");

        let toolbar = gtk::Box::new(gtk::Orientation::Horizontal, 8);
        toolbar.add_css_class("glimpse-chrome");
        toolbar.set_halign(gtk::Align::Center);
        toolbar.set_margin_top(8);
        toolbar.set_margin_bottom(8);
        toolbar.set_margin_start(8);
        toolbar.set_margin_end(8);
        for w in [&record, &show_rect] {
            w.set_margin_top(6);
            w.set_margin_bottom(6);
            w.set_margin_start(6);
            w.set_margin_end(6);
            toolbar.append(w);
        }

        let root = gtk::Box::new(gtk::Orientation::Vertical, 0);
        root.append(&toolbar);
        root.append(&frame);
        root.append(&status);
        window.set_child(Some(&root));

        let me = Self {
            window: window.clone(),
            hole: hole.clone(),
            probe: probe.clone(),
            locked: Rc::new(Cell::new(false)),
            frozen: Rc::new(RefCell::new(None)),
        };

        me.install_input_region_updater();
        me.install_selftest(&status);

        {
            let me_probe = probe.clone();
            let window_c = window.clone();
            let hole_c = hole.clone();
            let status_c = status.clone();
            show_rect.connect_clicked(move |_| match capture_rect(&window_c, &hole_c, &me_probe) {
                Ok(r) if r.is_capturable() => {
                    status_c.set_text(&format!("capture rect: {}x{} at {},{}", r.w, r.h, r.x, r.y))
                }
                Ok(r) => status_c.set_text(&format!(
                    "frame is off-screen ({}x{}) — nothing to capture",
                    r.w, r.h
                )),
                Err(e) => status_c.set_text(&format!("geometry error: {e}")),
            });
        }

        {
            let status_c = status.clone();
            record.connect_clicked(move |_| {
                // Capture itself is not wired yet (ADR 0002); the lock contract is live.
                status_c.set_text("capture pipeline not wired yet — see docs/roadmap.md");
            });
        }

        me
    }

    /// Freeze the geometry for a recording session (ADR 0002: the frame must not
    /// move under the recorder, or the visible frame and the fixed x11grab
    /// rectangle diverge silently).
    pub fn lock(&self) -> Result<RootPixelRect> {
        let rect = capture_rect(&self.window, &self.hole, &self.probe)?;
        if !rect.is_capturable() {
            return Err(anyhow!("frame is off-screen: {}x{}", rect.w, rect.h));
        }
        self.window.set_resizable(false);
        self.locked.set(true);
        *self.frozen.borrow_mut() = Some(rect);
        Ok(rect)
    }

    pub fn unlock(&self) {
        self.locked.set(false);
        *self.frozen.borrow_mut() = None;
        self.window.set_resizable(true);
    }

    pub fn frozen_rect(&self) -> Option<RootPixelRect> {
        *self.frozen.borrow()
    }

    /// Re-punch the input hole whenever the layout settles.
    ///
    /// `connect_map` is too early — the window has no allocation yet, which is
    /// how the spike first "failed" Q2 (ADR 0000). A tick callback reacting to bounds
    /// changes is also what correctness demands, since the hole moves on resize.
    fn install_input_region_updater(&self) {
        let hole = self.hole.clone();
        let last: Cell<(i32, i32, i32, i32)> = Cell::new((0, 0, 0, 0));
        self.window.add_tick_callback(move |win, _| {
            if let Some(b) = hole.compute_bounds(win) {
                let cur = (
                    b.x() as i32,
                    b.y() as i32,
                    b.width() as i32,
                    b.height() as i32,
                );
                if cur != last.get() && cur.2 > 0 && cur.3 > 0 {
                    last.set(cur);
                    if let Err(e) = punch_input_hole(win.upcast_ref(), cur) {
                        eprintln!("glimpse: input region not applied: {e:#}");
                    }
                }
            }
            glib::ControlFlow::Continue
        });
    }
}

impl FramingWindow {
    /// `GLIMPSE_SELFTEST=1` runs the two checks the spike (ADR 0000) proved are the
    /// only ones that catch real geometry bugs, then exits:
    ///
    ///   1. read the input shape back **from the X server** — pointer-position
    ///      tests are blind to it;
    ///   2. grab the computed rect and write a PNG — arithmetic that agrees with
    ///      `xwininfo` can still be wrong, and only the image shows it.
    fn install_selftest(&self, status: &gtk::Label) {
        if std::env::var("GLIMPSE_SELFTEST").is_err() {
            return;
        }
        let window = self.window.clone();
        let hole = self.hole.clone();
        let probe = self.probe.clone();
        let status = status.clone();
        glib::timeout_add_seconds_local_once(3, move || {
            status.set_text("self-test running");
            let report = run_selftest(&window, &hole, &probe);
            println!("{report}");
            if let Some(a) = window.application() {
                a.quit();
            }
        });
    }
}

fn run_selftest(window: &gtk::ApplicationWindow, hole: &gtk::Box, probe: &X11Probe) -> String {
    let rect = match capture_rect(window, hole, probe) {
        Ok(r) => r,
        Err(e) => return format!("SELFTEST FAILED: geometry: {e:#}"),
    };
    let xid = match window_xid(window.upcast_ref()) {
        Ok(x) => x,
        Err(e) => return format!("SELFTEST FAILED: xid: {e:#}"),
    };

    let shape = match probe.input_shape(xid) {
        Ok(rs) if rs.is_empty() => "no input shape set — click-through is NOT active".to_string(),
        Ok(rs) => format!(
            "{} band(s): {}",
            rs.len(),
            rs.iter()
                .map(|(x, y, w, h)| format!("{w}x{h}+{x}+{y}"))
                .collect::<Vec<_>>()
                .join(" ")
        ),
        Err(e) => format!("shape query failed: {e}"),
    };

    let xwin = crate::geometry::verify_against_xwininfo(xid)
        .map(|(x, y)| format!("window origin {x},{y}"))
        .unwrap_or_else(|| "unavailable".into());

    let out = "/tmp/glimpse-selftest.png";
    let display = std::env::var("DISPLAY").unwrap_or_else(|_| ":0".into());
    let grab = std::process::Command::new("ffmpeg")
        .args([
            "-y",
            "-f",
            "x11grab",
            "-video_size",
            &rect.video_size(),
            "-i",
            &format!("{display}+{},{}", rect.x, rect.y),
            "-frames:v",
            "1",
            out,
        ])
        .output();
    let grab = match grab {
        Ok(o) if o.status.success() => format!(
            "wrote {out} — INSPECT IT: any Glimpse chrome \
             (frame border, toolbar) in the image means the rect is wrong"
        ),
        Ok(o) => format!(
            "FAILED: {}",
            String::from_utf8_lossy(&o.stderr)
                .lines()
                .last()
                .unwrap_or("?")
        ),
        Err(e) => format!("ffmpeg not spawnable: {e}"),
    };

    format!(
        "\n=== glimpse self-test ===\n\
         capture rect : {}x{} at {},{}\n\
         xid          : 0x{xid:x}\n\
         xwininfo     : {xwin}\n\
         input shape  : {shape}\n\
         grab         : {grab}\n",
        rect.w, rect.h, rect.x, rect.y
    )
}

/// Input region = the whole window minus the hole, so clicks in the middle reach
/// whatever is underneath.
fn punch_input_hole(window: &gtk::Window, hole: (i32, i32, i32, i32)) -> Result<()> {
    let surface = window.surface().ok_or_else(|| anyhow!("no surface"))?;
    if !surface.display().supports_input_shapes() {
        return Err(anyhow!("display does not support input shapes"));
    }
    let (w, h) = (window.width(), window.height());
    if w <= 0 || h <= 0 {
        return Err(anyhow!("window not sized yet"));
    }

    let region = cairo::Region::create_rectangle(&cairo::RectangleInt::new(0, 0, w, h));
    region.subtract_rectangle(&cairo::RectangleInt::new(hole.0, hole.1, hole.2, hole.3))?;
    surface.set_input_region(Some(&region));
    Ok(())
}
