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
use std::cell::{Cell, RefCell};
use std::rc::Rc;

use crate::capture::{RecorderConfig, Workspace};
use crate::geometry::{capture_rect, RootPixelRect};
use crate::session::{transition, CaptureRequest, Effect, Event, State};
use crate::worker::{RecordingWorker, WorkerEvent};
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
    /// The lifecycle. Every transition goes through `crate::session::transition`,
    /// so the policies stay in the tested pure module rather than in callbacks.
    state: RefCell<State>,
    /// Owns the ffmpeg child while a recording is live. Dropping it reaps.
    worker: RefCell<Option<RecordingWorker>>,
    status: gtk::Label,
    record: gtk::Button,
}

impl FramingWindow {
    pub fn new(app: &gtk::Application, probe: Rc<X11Probe>) -> Rc<Self> {
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

        let me = Rc::new(Self {
            window: window.clone(),
            hole: hole.clone(),
            probe: probe.clone(),
            frozen: Cell::new(None),
            state: RefCell::new(State::Idle),
            worker: RefCell::new(None),
            status: status.clone(),
            record: record.clone(),
        });

        me.install_input_region_updater();
        me.install_selftest();
        me.install_driver();

        {
            let me2 = me.clone();
            show_rect.connect_clicked(move |_| {
                match capture_rect(&me2.window, &me2.hole, &me2.probe) {
                    Ok(r) if r.is_capturable() => me2
                        .status
                        .set_text(&format!("capture rect: {}x{} at {},{}", r.w, r.h, r.x, r.y)),
                    Ok(r) => me2.status.set_text(&format!(
                        "frame is off-screen ({}x{}) — nothing to capture",
                        r.w, r.h
                    )),
                    Err(e) => me2.status.set_text(&format!("geometry error: {e}")),
                }
            });
        }

        {
            let me2 = me.clone();
            record.connect_clicked(move |_| me2.on_record_clicked());
        }

        // Shutdown must reap. Dropping the worker kills and waits for the child,
        // and going through the state machine keeps that policy in one place.
        {
            let me2 = me.clone();
            window.connect_close_request(move |_| {
                me2.dispatch(Event::Shutdown);
                glib::Propagation::Proceed
            });
        }

        me.refresh();
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
    fn install_selftest(self: &Rc<Self>) {
        let mode = match std::env::var("GLIMPSE_SELFTEST") {
            Ok(m) => m,
            Err(_) => return,
        };
        // `GLIMPSE_SELFTEST=record` drives a real record/stop cycle through the
        // same code path the button uses, so the wiring is verified end to end
        // rather than only the geometry.
        if mode == "record" {
            let me = self.clone();
            glib::timeout_add_seconds_local_once(2, move || {
                println!("[smoke] pressing Record");
                me.on_record_clicked();
                println!("[smoke] state: {:?}", me.state());

                let me2 = me.clone();
                glib::timeout_add_seconds_local_once(3, move || {
                    println!("[smoke] pressing Stop");
                    me2.on_record_clicked();

                    // Give the worker time to finalise, then report and quit.
                    let me3 = me2.clone();
                    glib::timeout_add_seconds_local_once(3, move || {
                        println!("[smoke] final state: {:?}", me3.state());
                        if let Some(v) = me3.state.borrow().retryable() {
                            let bytes = std::fs::metadata(&v.path).map(|m| m.len()).unwrap_or(0);
                            println!("[smoke] recording: {} ({bytes} bytes)", v.path.display());
                        }
                        if let Some(a) = me3.window.application() {
                            a.quit();
                        }
                    });
                });
            });
            return;
        }

        let me = self.clone();
        glib::timeout_add_seconds_local_once(3, move || {
            me.status.set_text("self-test running");
            println!("{}", run_selftest(&me.window, &me.hole, &me.probe));
            if let Some(a) = me.window.application() {
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

// ---------------------------------------------------------------------------
// The controller: the impure half that drives the pure state machine.
// ---------------------------------------------------------------------------

/// How often the driver looks for a finished recording and for a frame that has
/// moved. Fast enough that a drifted recording is cut within a fraction of a
/// second; slow enough to be invisible next to a 60Hz frame clock.
const DRIVER_TICK_MS: u32 = 100;

impl FramingWindow {
    fn state(&self) -> State {
        self.state.borrow().clone()
    }

    fn on_record_clicked(self: &Rc<Self>) {
        match self.state() {
            State::Idle
            | State::Completed { .. }
            | State::Failed { .. }
            | State::Cancelled { .. } => {
                let rect = match self.lock() {
                    Ok(r) => r,
                    Err(e) => {
                        self.status.set_text(&format!("cannot record: {e}"));
                        return;
                    }
                };
                let request = CaptureRequest {
                    rect,
                    framerate: 15,
                    capture_mouse: true,
                    destination: default_destination(),
                };
                self.dispatch(Event::Arm(request));
                // Geometry is snapshotted and the window is fixed, so arming is
                // complete. A settling delay belongs here once resizing is
                // interactive rather than window-manager driven.
                self.dispatch(Event::Armed);
            }
            State::Recording { .. } => self.dispatch(Event::Stop),
            State::Arming { .. } => self.dispatch(Event::Cancel),
            State::Stopping { .. } | State::Encoding { .. } => {
                self.status.set_text("already finishing — one moment");
            }
        }
    }

    /// Feed an event to the pure machine and carry out whatever it asks for.
    ///
    /// Effects may produce a follow-up event, so this loops rather than
    /// recursing: recursion would re-enter `self.state.borrow_mut()` and panic.
    fn dispatch(self: &Rc<Self>, event: Event) {
        let mut pending = Some(event);
        while let Some(ev) = pending.take() {
            let current = self.state.borrow().clone();
            let (next, effect) = transition(current, ev);
            *self.state.borrow_mut() = next;
            pending = self.apply(effect);
        }
        self.refresh();
    }

    /// Perform one effect. Returns a follow-up event when the outcome is known
    /// immediately.
    fn apply(self: &Rc<Self>, effect: Effect) -> Option<Event> {
        match effect {
            Effect::None => None,

            Effect::StartRecorder(request) => {
                let display = match RecorderConfig::display_from_env() {
                    Ok(d) => d,
                    Err(e) => return Some(Event::RecorderFailed(format!("{e:#}"))),
                };
                let workspace = match Workspace::create() {
                    Ok(w) => w,
                    Err(e) => return Some(Event::RecorderFailed(format!("{e:#}"))),
                };
                let config = RecorderConfig {
                    display,
                    rect: request.rect,
                    framerate: request.framerate,
                    capture_mouse: request.capture_mouse,
                };
                *self.worker.borrow_mut() = Some(RecordingWorker::start(config, workspace));
                None
            }

            Effect::GracefulStop => {
                if let Some(w) = self.worker.borrow().as_ref() {
                    w.stop();
                }
                None
            }

            Effect::Terminate => {
                if let Some(w) = self.worker.borrow().as_ref() {
                    w.abort();
                }
                None
            }

            // Encoding is the next milestone. Rather than pretend, the session is
            // failed with the source preserved — which is the same path a real
            // encoder failure takes, so the retryable artifact is exercised for
            // real instead of only in tests.
            Effect::StartEncoder { source, .. } => {
                self.status.set_text(&format!(
                    "recorded to {} — GIF encoding is the next milestone",
                    source.path.display()
                ));
                Some(Event::EncoderFailed(
                    "GIF encoding is not implemented yet".into(),
                ))
            }

            Effect::Cleanup { preserve_source } => {
                // Dropping the worker joins its thread, which guarantees the
                // child is dead and reaped before anything else happens.
                self.worker.borrow_mut().take();
                if !preserve_source {
                    if let Some(v) = self.state.borrow().retryable() {
                        let _ = std::fs::remove_dir_all(&v.workspace);
                    }
                }
                self.unlock();
                None
            }

            Effect::Unlock => {
                self.worker.borrow_mut().take();
                self.unlock();
                None
            }
        }
    }

    /// Poll the worker, and watch for a frame that moved out from under a live
    /// recording.
    fn install_driver(self: &Rc<Self>) {
        let me = self.clone();
        glib::timeout_add_local(
            std::time::Duration::from_millis(DRIVER_TICK_MS as u64),
            move || {
                let event = me.worker.borrow().as_ref().and_then(|w| w.poll());
                if let Some(e) = event {
                    match e {
                        WorkerEvent::Finished(v) => me.dispatch(Event::RecorderFinished(v)),
                        WorkerEvent::Failed(msg) => me.dispatch(Event::RecorderFailed(msg)),
                        WorkerEvent::Aborted => me.refresh(),
                    }
                }

                // The checked invariant from ADR 0004: x11grab records a fixed
                // rectangle, so a moved frame means everything after the move is
                // the wrong region — and the file would still look plausible.
                if matches!(me.state(), State::Recording { .. }) {
                    if let Some((was, now)) = me.geometry_drifted() {
                        eprintln!(
                            "glimpse: frame moved during recording ({},{} -> {},{}); aborting",
                            was.x, was.y, now.x, now.y
                        );
                        me.dispatch(Event::GeometryDrifted);
                    }
                }

                glib::ControlFlow::Continue
            },
        );
    }

    /// Push the current state into the widgets. The single place that decides
    /// what the user sees, so the button label can never disagree with the state.
    fn refresh(&self) {
        let (label, status, sensitive) = match &*self.state.borrow() {
            State::Idle => (
                "Record",
                "Position the frame, then Record.".to_string(),
                true,
            ),
            State::Arming { .. } => ("Cancel", "arming…".to_string(), true),
            State::Recording { request } => (
                "Stop",
                format!("recording {}x{}", request.rect.w, request.rect.h),
                true,
            ),
            State::Stopping { .. } => ("Stop", "finishing the file…".to_string(), false),
            State::Encoding { .. } => ("Stop", "encoding…".to_string(), false),
            State::Completed { output } => ("Record", format!("saved {}", output.display()), true),
            State::Failed { error, retryable } => {
                let where_is_it = retryable
                    .as_ref()
                    .map(|v| format!(" — recording kept at {}", v.path.display()))
                    .unwrap_or_default();
                ("Record", format!("{error}{where_is_it}"), true)
            }
            State::Cancelled { preserved } => {
                let where_is_it = preserved
                    .as_ref()
                    .map(|v| format!(" — recording kept at {}", v.path.display()))
                    .unwrap_or_default();
                ("Record", format!("cancelled{where_is_it}"), true)
            }
        };
        self.record.set_label(label);
        self.record.set_sensitive(sensitive);
        self.status.set_text(&status);
    }
}

/// Where a finished GIF goes until output selection exists.
///
/// Collision policy is deliberately not decided here — it belongs with the
/// encoding milestone, because it governs the atomic commit.
fn default_destination() -> std::path::PathBuf {
    let home = std::env::var("HOME").unwrap_or_else(|_| "/tmp".into());
    std::path::PathBuf::from(home).join("glimpse.gif")
}
