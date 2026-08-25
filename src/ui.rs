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
use gtk::prelude::*;
use gtk::{cairo, glib};
use gtk4 as gtk;
use std::cell::Cell;
use std::rc::Rc;

use crate::geometry::{capture_rect, RootPixelRect};
use crate::x11probe::{self, shape_covers, X11Probe};

const CSS: &str = "
window.glimpse { background: transparent; }
.glimpse-frame { border: 3px solid @accent_bg_color; border-radius: 2px; }
.glimpse-hole  { background: transparent; }
.glimpse-chrome { background: rgba(30,30,30,0.92); border-radius: 6px; }
";

pub struct FramingWindow {
    pub window: gtk::ApplicationWindow,
    hole: gtk::Box,
    probe: Rc<X11Probe>,
    /// The rect snapshotted when locking, per ADR 0002. `Some` means a session
    /// owns the geometry.
    frozen: Cell<Option<RootPixelRect>>,
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
            frozen: Cell::new(None),
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
        self.frozen.set(Some(rect));
        Ok(rect)
    }

    pub fn unlock(&self) {
        self.frozen.set(None);
        self.window.set_resizable(true);
    }

    pub fn frozen_rect(&self) -> Option<RootPixelRect> {
        self.frozen.get()
    }

    /// Has the frame moved since it was locked?
    ///
    /// `lock()` disables resizing, but **a window manager can still move the
    /// window** — alt-drag, a workspace change, a tiling rule. `x11grab` records a
    /// fixed root rectangle, so an undetected move means the visible frame and the
    /// recording diverge while the output still looks plausible. Enforcement is a
    /// checked invariant, not an assumption about what GTK can prevent.
    pub fn geometry_drifted(&self) -> Option<(RootPixelRect, RootPixelRect)> {
        let frozen = self.frozen.get()?;
        let now = capture_rect(&self.window, &self.hole, &self.probe).ok()?;
        (now != frozen).then_some((frozen, now))
    }

    /// Re-punch the input hole whenever the surface is laid out.
    ///
    /// Driven by the surface's `layout` signal rather than a tick callback. A tick
    /// callback fires at the monitor refresh rate for the life of the window — on
    /// a 165Hz display that is a permanent wakeup source for an app that is idle
    /// almost all of the time. `layout` fires when the geometry actually changes,
    /// which is the only moment the region needs recomputing.
    ///
    /// `connect_map` would be too early to *punch* — no allocation yet, which is
    /// how the spike first "failed" Q2 (ADR 0000) — so the first punch happens
    /// here on realize, after which `layout` keeps it current.
    fn install_input_region_updater(&self) {
        let hole = self.hole.clone();
        self.window.connect_realize(move |win| {
            let Some(surface) = win.surface() else {
                eprintln!("glimpse: realized with no surface; click-through disabled");
                return;
            };
            let last = Rc::new(Cell::new((0, 0, 0, 0)));
            sync_input_region(win, &hole, &last);

            let (hole, win, last) = (hole.clone(), win.clone(), last.clone());
            surface.connect_layout(move |_, _, _| sync_input_region(&win, &hole, &last));
        });
    }
}

/// Recompute the hole and re-punch, skipping the server round trip when nothing
/// moved.
fn sync_input_region(
    win: &gtk::ApplicationWindow,
    hole: &gtk::Box,
    last: &Cell<(i32, i32, i32, i32)>,
) {
    let Some(b) = hole.compute_bounds(win) else {
        return;
    };
    let cur = (
        b.x() as i32,
        b.y() as i32,
        b.width() as i32,
        b.height() as i32,
    );
    if cur == last.get() || cur.2 <= 0 || cur.3 <= 0 {
        return;
    }
    last.set(cur);
    if let Err(e) = punch_input_hole(win.upcast_ref(), cur) {
        eprintln!("glimpse: input region not applied: {e:#}");
    }
}

/// Input region = the whole window minus the hole, so clicks in the middle reach
/// whatever is underneath.
fn punch_input_hole(window: &gtk::Window, hole: (i32, i32, i32, i32)) -> Result<()> {
    let surface = window.surface().ok_or_else(|| anyhow!("no surface"))?;
    if !surface.display().supports_input_shapes() {
        return Err(anyhow!("display does not support input shapes"));
    }

    // The input region is in SURFACE coordinates, and `hole` arrives in widget
    // coordinates. `capture_rect` applies this same transform; if only one of the
    // two applies it, the punched hole drifts from the visible one by the client-
    // side-decoration margin. Rounding matches `geometry.rs` for the same reason.
    let (tx, ty) = window.surface_transform();
    let hx = (hole.0 as f64 + tx).round() as i32;
    let hy = (hole.1 as f64 + ty).round() as i32;

    let (w, h) = (surface.width(), surface.height());
    if w <= 0 || h <= 0 {
        return Err(anyhow!("surface not sized yet"));
    }

    let region = cairo::Region::create_rectangle(&cairo::RectangleInt::new(0, 0, w, h));
    region.subtract_rectangle(&cairo::RectangleInt::new(hx, hy, hole.2, hole.3))?;
    surface.set_input_region(Some(&region));
    Ok(())
}

impl FramingWindow {
    /// `GLIMPSE_SELFTEST=1` runs the two checks the spike (ADR 0000) proved are
    /// the only ones that catch real geometry bugs, then exits:
    ///
    ///   1. read the input shape back **from the X server** and check what it
    ///      means — pointer-position tests are blind to input shape, and merely
    ///      counting bands cannot tell a correct hole from a misplaced one;
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
            println!("{}", run_selftest(&window, &hole, &probe));
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
    let xid = match x11probe::window_xid(window.upcast_ref()) {
        Ok(x) => x,
        Err(e) => return format!("SELFTEST FAILED: xid: {e:#}"),
    };

    // Counting bands proves nothing: a window with no shape set can still report
    // a band covering everything, and a misplaced hole reports bands too. Check
    // the semantics — the hole must NOT take clicks, the border MUST.
    let shape = match (probe.input_shape(xid), hole.compute_bounds(window)) {
        (Ok(bands), Some(b)) => {
            let (hx, hy) = (b.x() as i32, b.y() as i32);
            let (hw, hh) = (b.width() as i32, b.height() as i32);
            let centre = shape_covers(&bands, hx + hw / 2, hy + hh / 2);
            let border = shape_covers(&bands, hx + hw / 2, hy - 2);
            let listed = bands
                .iter()
                .map(|(x, y, w, h)| format!("{w}x{h}+{x}+{y}"))
                .collect::<Vec<_>>()
                .join(" ");
            let verdict = match (centre, border) {
                (false, true) => "PASS — hole is click-through, border takes clicks",
                (true, _) => "FAIL — the hole still takes clicks; region misplaced or absent",
                (false, false) => "FAIL — the border takes no clicks either; region too large",
            };
            format!(
                "{verdict}\n                 {} band(s): {listed}",
                bands.len()
            )
        }
        (Err(e), _) => format!("shape query failed: {e}"),
        (_, None) => "could not compute hole bounds".to_string(),
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
            "wrote {out} — INSPECT IT: any Glimpse chrome in the image means the rect is wrong"
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
