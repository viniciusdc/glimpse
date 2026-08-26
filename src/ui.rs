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
//! part of every GIF. The spike in ADR 0000 hit exactly that, and it survived an
//! `xwininfo` cross-check. Separating the two removes the bug class instead of
//! compensating for it with a magic inset.

use anyhow::{anyhow, Result};
use gtk::prelude::*;
use gtk::{cairo, gio, glib};
use gtk4 as gtk;
use std::cell::{Cell, RefCell};
use std::path::PathBuf;
use std::rc::Rc;

use crate::capture::{RecorderConfig, Workspace};
use crate::config::{Config, Mode, Theme};
use crate::encode::OutputFormat;
use crate::geometry::{capture_rect, RootPixelRect};
use crate::session::{transition, CaptureRequest, Effect, Event, State};
use crate::worker::{FileJob, JobEvent, RecordingWorker, WorkerEvent};
use crate::x11probe::{self, shape_covers, X11Probe};

/// Colours that differ between the light and dark palettes.
///
/// The stylesheet itself is written once and the palette substituted in, so the
/// two themes cannot drift apart structurally — only in colour. The accent and
/// the recording red are deliberately absent: they are the same in both, because
/// they carry meaning rather than mood.
struct Palette {
    header_bg: &'static str,
    header_line: &'static str,
    meta: &'static str,
    emphasis: &'static str,
    chip_line: &'static str,
    hover: &'static str,
    status_bg: &'static str,
    link: &'static str,
    link_hover: &'static str,
    shadow: &'static str,
}

const DARK: Palette = Palette {
    header_bg: "#282c33",
    header_line: "rgba(0,0,0,0.4)",
    meta: "#8b939e",
    emphasis: "#c3c9d2",
    chip_line: "rgba(255,255,255,0.14)",
    hover: "rgba(255,255,255,0.08)",
    status_bg: "rgba(16,18,22,0.92)",
    link: "#8ab4f8",
    link_hover: "#b8d0fb",
    shadow: "rgba(0,0,0,0.6)",
};

const LIGHT: Palette = Palette {
    header_bg: "#e9ecf0",
    header_line: "rgba(0,0,0,0.14)",
    meta: "#5c6570",
    emphasis: "#2f3640",
    chip_line: "rgba(0,0,0,0.20)",
    hover: "rgba(0,0,0,0.07)",
    status_bg: "rgba(247,249,251,0.95)",
    link: "#0969da",
    link_hover: "#1a7fe8",
    shadow: "rgba(0,0,0,0.25)",
};

/// Ported from the `Glimpse Screen Recording UI` design document; the tokens that
/// carry meaning — the accent blue, the recording red, the abort amber — are kept
/// verbatim and are identical in both themes.
fn stylesheet(p: &Palette) -> String {
    format!(
        r#"
window.glimpse {{ background: transparent; }}

.glimpse-shell {{
  border-radius: 10px;
  box-shadow: 0 30px 80px {shadow};
}}

.glimpse-header {{
  background: {header_bg};
  border-bottom: 1px solid {header_line};
  border-radius: 10px 10px 0 0;
  min-height: 44px;
  padding: 0 12px;
}}
.glimpse-meta {{
  color: {meta};
  font-size: 12px;
  font-feature-settings: "tnum";
}}
.glimpse-elapsed {{ color: {emphasis}; }}
.glimpse-recdot {{
  background: #e04b4b;
  border-radius: 50%;
  min-width: 8px;
  min-height: 8px;
  animation: glimpse-pulse 1.4s ease-in-out infinite;
}}
@keyframes glimpse-pulse {{
  from {{ opacity: 1; }}
  50%  {{ opacity: 0.25; }}
  to   {{ opacity: 1; }}
}}

.glimpse-action {{
  background: #3689e6;
  color: #ffffff;
  font-size: 12.5px;
  font-weight: 500;
  border: 0;
  border-radius: 14px;
  min-height: 28px;
  padding: 0 16px;
  box-shadow: none;
  text-shadow: none;
}}
.glimpse-action-main {{ border-radius: 14px 0 0 14px; padding: 0 12px 0 16px; }}
.glimpse-action-arrow {{
  border-radius: 0 14px 14px 0;
  padding: 0 8px;
  min-width: 20px;
  border-left: 1px solid rgba(0,0,0,0.22);
}}
.glimpse-action:hover {{ background: #4a97ea; }}
.glimpse-action:disabled {{ opacity: 0.55; }}
.state-recording .glimpse-action,
.state-stopping  .glimpse-action {{ background: #c6262e; }}
.state-recording .glimpse-action:hover {{ background: #d2343c; }}

.glimpse-bullet {{ background: #ffffff; min-width: 9px; min-height: 9px; border-radius: 50%; }}
.state-recording .glimpse-bullet,
.state-stopping  .glimpse-bullet {{ min-width: 8px; min-height: 8px; border-radius: 0; }}

.glimpse-chip {{
  color: {meta};
  font-size: 10.5px;
  font-weight: 500;
  letter-spacing: 0.6px;
  border: 1px solid {chip_line};
  border-radius: 5px;
  padding: 2px 7px;
}}
.glimpse-menu {{
  color: {meta};
  background: none;
  border: 0;
  box-shadow: none;
  min-height: 24px;
  min-width: 24px;
  padding: 0;
}}
.glimpse-menu:hover {{ background: {hover}; border-radius: 5px; }}
.glimpse-menu > button {{ padding: 0 4px; min-height: 24px; }}

/* The capture region. The border lives on this widget, and the capture target
   inside it paints nothing — see ADR 0000. Never move this border onto
   .glimpse-hole. */
.glimpse-frame {{ border: 3px solid #3689e6; }}
.state-recording .glimpse-frame,
.state-stopping  .glimpse-frame {{ border-color: #e04b4b; }}
.state-aborted   .glimpse-frame {{ border-color: #e5a50a; }}
.glimpse-hole {{ background: transparent; }}

.glimpse-status {{
  background: {status_bg};
  border-radius: 0 0 10px 10px;
  min-height: 32px;
  padding: 0 14px;
  color: {meta};
  font-size: 11.5px;
}}
.glimpse-status label {{ color: {meta}; font-size: 11.5px; }}
.state-recording .glimpse-status label {{ color: #d78f8f; font-feature-settings: "tnum"; }}
.state-aborted   .glimpse-status label {{ color: #e0b45c; }}

.glimpse-statusdot {{ min-width: 7px; min-height: 7px; border-radius: 50%; background: #68b3f0; }}
.state-aborted .glimpse-statusdot {{ background: #e5a50a; }}

.glimpse-link {{
  background: none;
  border: 0;
  box-shadow: none;
  padding: 0;
  min-height: 0;
  color: {link};
  font-size: 11.5px;
}}
.glimpse-link:hover {{ color: {link_hover}; }}
.glimpse-grip-debug {{ background: rgba(255,0,255,0.6); }}
"#,
        shadow = p.shadow,
        header_bg = p.header_bg,
        header_line = p.header_line,
        meta = p.meta,
        emphasis = p.emphasis,
        chip_line = p.chip_line,
        hover = p.hover,
        status_bg = p.status_bg,
        link = p.link,
        link_hover = p.link_hover,
    )
}

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
    /// Owns the encode, or the snapshot, while one is running.
    encoder: RefCell<Option<FileJob>>,
    /// The current session's workspace.
    ///
    /// Tracked here rather than read back out of the state, because on the happy
    /// path the state is `Completed` by the time cleanup runs and `Completed`
    /// carries no `CapturedVideo` — so looking it up there silently leaked the
    /// recording directory on every successful encode.
    workspace: RefCell<Option<PathBuf>>,
    status: gtk::Label,
    record: gtk::Button,
    record_label: gtk::Label,
    shell: gtk::Box,
    size_label: gtk::Label,
    elapsed: gtk::Label,
    rec_dot: gtk::Box,
    status_dot: gtk::Box,
    reveal: gtk::Button,
    /// When the current recording started, for the elapsed readout.
    started: Cell<Option<std::time::Instant>>,
    /// The last output written, so "Show in folder" knows where to point.
    last_output: RefCell<Option<PathBuf>>,
    /// The chosen output format. Fixed for the duration of a session — it is
    /// copied into the `CaptureRequest` at arming time.
    format: Cell<OutputFormat>,
    chip: gtk::MenuButton,
    /// Persisted user settings. Written on every change rather than on exit,
    /// because a screen recorder is the kind of thing people close abruptly.
    config: RefCell<Config>,
    css: gtk::CssProvider,
    /// What the primary button does when clicked.
    mode: Cell<Mode>,
    bullet: gtk::Box,
}

impl FramingWindow {
    pub fn new(app: &gtk::Application, probe: Rc<X11Probe>) -> Rc<Self> {
        let config = Config::load();
        let css = gtk::CssProvider::new();
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

        // The design has no title bar: the header IS the chrome. Server-side
        // decorations remain available via GLIMPSE_DECORATIONS=server, because an
        // undecorated window that cannot be resized would break the product —
        // the frame's size *is* the capture region.
        let server_decorations = std::env::var("GLIMPSE_DECORATIONS")
            .map(|v| v == "server")
            .unwrap_or(false);
        window.set_decorated(server_decorations);

        // ---- header ------------------------------------------------------
        let rec_dot = gtk::Box::new(gtk::Orientation::Horizontal, 0);
        rec_dot.add_css_class("glimpse-recdot");
        rec_dot.set_valign(gtk::Align::Center);
        rec_dot.set_visible(false);

        let size_label = gtk::Label::new(Some("0 × 0"));
        size_label.add_css_class("glimpse-meta");

        let elapsed = gtk::Label::new(None);
        elapsed.add_css_class("glimpse-meta");
        elapsed.add_css_class("glimpse-elapsed");
        elapsed.set_visible(false);

        let meta = gtk::Box::new(gtk::Orientation::Horizontal, 10);
        meta.set_hexpand(true);
        meta.set_halign(gtk::Align::Start);
        meta.append(&rec_dot);
        meta.append(&size_label);
        meta.append(&elapsed);

        let bullet = gtk::Box::new(gtk::Orientation::Horizontal, 0);
        bullet.add_css_class("glimpse-bullet");
        bullet.set_valign(gtk::Align::Center);
        let record_label = gtk::Label::new(Some("Record"));
        let record_content = gtk::Box::new(gtk::Orientation::Horizontal, 8);
        record_content.append(&bullet);
        record_content.append(&record_label);

        let record = gtk::Button::builder().child(&record_content).build();
        record.add_css_class("glimpse-action");
        record.add_css_class("glimpse-action-main");
        record.set_valign(gtk::Align::Center);

        // A split button: one control, two actions. GTK has no SplitButton
        // outside libadwaita, so it is two widgets in a box that CSS joins into
        // one pill — the left half acts, the right half chooses what acting means.
        let mode_menu = gio::Menu::new();
        for m in Mode::all() {
            let item = gio::MenuItem::new(Some(m.label()), None);
            item.set_action_and_target_value(Some("win.mode"), Some(&m.id().to_variant()));
            mode_menu.append_item(&item);
        }
        let mode_button = gtk::MenuButton::builder().menu_model(&mode_menu).build();
        mode_button.add_css_class("glimpse-action");
        mode_button.add_css_class("glimpse-action-arrow");
        mode_button.set_valign(gtk::Align::Center);

        let action_group = gtk::Box::new(gtk::Orientation::Horizontal, 0);
        action_group.add_css_class("glimpse-split");
        action_group.set_valign(gtk::Align::Center);
        action_group.append(&record);
        action_group.append(&mode_button);

        let format_menu = gio::Menu::new();
        for f in OutputFormat::all() {
            let item = gio::MenuItem::new(Some(f.label()), None);
            item.set_action_and_target_value(Some("win.format"), Some(&f.extension().to_variant()));
            format_menu.append_item(&item);
        }
        let chip = gtk::MenuButton::builder()
            .label(config.format.label())
            .menu_model(&format_menu)
            .build();
        chip.add_css_class("glimpse-chip");
        chip.set_valign(gtk::Align::Center);

        let theme_menu = gio::Menu::new();
        for t in Theme::all() {
            let item = gio::MenuItem::new(Some(t.label()), None);
            item.set_action_and_target_value(Some("win.theme"), Some(&t.id().to_variant()));
            theme_menu.append_item(&item);
        }

        let menu_model = gio::Menu::new();
        let output_section = gio::Menu::new();
        output_section.append(Some("Save recordings to…"), Some("win.choose-folder"));
        menu_model.append_section(None, &output_section);
        menu_model.append_submenu(Some("Theme"), &theme_menu);
        let tail = gio::Menu::new();
        tail.append(Some("Show capture rect"), Some("win.show-rect"));
        tail.append(Some("Quit"), Some("window.close"));
        menu_model.append_section(None, &tail);
        let menu = gtk::MenuButton::builder()
            .icon_name("open-menu-symbolic")
            .menu_model(&menu_model)
            .build();
        menu.add_css_class("glimpse-menu");
        menu.set_valign(gtk::Align::Center);

        let trailing = gtk::Box::new(gtk::Orientation::Horizontal, 10);
        trailing.set_hexpand(true);
        trailing.set_halign(gtk::Align::End);
        trailing.append(&chip);
        trailing.append(&menu);

        let header = gtk::Box::new(gtk::Orientation::Horizontal, 0);
        header.add_css_class("glimpse-header");
        header.append(&meta);
        header.append(&action_group);
        header.append(&trailing);

        // Dragging the header moves the window; without this an undecorated
        // framing window could not be positioned, which is the whole interaction.
        let header_handle = gtk::WindowHandle::builder().child(&header).build();

        // GLIMPSE_DECORATIONS=server hands the frame back to the window manager.
        let system_decorations = std::env::var("GLIMPSE_DECORATIONS")
            .map(|v| v == "server")
            .unwrap_or(false);
        window.set_decorated(system_decorations);

        // ---- capture region ----------------------------------------------
        let hole = gtk::Box::new(gtk::Orientation::Vertical, 0);
        hole.add_css_class("glimpse-hole");
        hole.set_hexpand(true);
        hole.set_vexpand(true);

        let frame = gtk::Box::new(gtk::Orientation::Vertical, 0);
        frame.add_css_class("glimpse-frame");
        frame.set_hexpand(true);
        frame.set_vexpand(true);
        frame.append(&hole);

        // ---- status ------------------------------------------------------
        let status_dot = gtk::Box::new(gtk::Orientation::Horizontal, 0);
        status_dot.add_css_class("glimpse-statusdot");
        status_dot.set_valign(gtk::Align::Center);
        status_dot.set_visible(false);

        let status = gtk::Label::new(Some("Position the frame, then Record."));
        status.set_ellipsize(gtk::pango::EllipsizeMode::Middle);
        status.set_xalign(0.0);
        status.set_hexpand(true);

        let reveal = gtk::Button::with_label("Show in folder");
        reveal.add_css_class("glimpse-link");
        reveal.set_valign(gtk::Align::Center);
        reveal.set_visible(false);

        let status_bar = gtk::Box::new(gtk::Orientation::Horizontal, 8);
        status_bar.add_css_class("glimpse-status");
        status_bar.append(&status_dot);
        status_bar.append(&status);
        status_bar.append(&reveal);

        let shell = gtk::Box::new(gtk::Orientation::Vertical, 0);
        shell.add_css_class("glimpse-shell");
        shell.add_css_class("state-idle");
        shell.append(&header_handle);
        shell.append(&frame);
        shell.append(&status_bar);

        // Resize edges sit above everything, at the window's rim only.
        let overlay = gtk::Overlay::new();
        overlay.set_child(Some(&shell));
        if !system_decorations {
            install_resize_edges(&window, &overlay);
        }
        window.set_child(Some(&overlay));

        let me = Rc::new(Self {
            window: window.clone(),
            hole: hole.clone(),
            probe: probe.clone(),
            frozen: Cell::new(None),
            state: RefCell::new(State::Idle),
            worker: RefCell::new(None),
            encoder: RefCell::new(None),
            workspace: RefCell::new(None),
            status: status.clone(),
            record: record.clone(),
            record_label: record_label.clone(),
            shell: shell.clone(),
            size_label: size_label.clone(),
            elapsed: elapsed.clone(),
            rec_dot: rec_dot.clone(),
            status_dot: status_dot.clone(),
            reveal: reveal.clone(),
            started: Cell::new(None),
            last_output: RefCell::new(None),
            format: Cell::new(config.format),
            chip: chip.clone(),
            mode: Cell::new(config.mode),
            bullet: bullet.clone(),
            config: RefCell::new(config),
            css: css.clone(),
        });

        me.apply_theme();
        {
            // Keep following the desktop while the theme is "system". GTK
            // surfaces the desktop's preference here and updates it live.
            let me2 = me.clone();
            if let Some(settings) = gtk::Settings::default() {
                settings.connect_gtk_application_prefer_dark_theme_notify(move |_| {
                    if me2.config.borrow().theme == Theme::System {
                        me2.apply_theme();
                    }
                });
            }
        }

        me.install_input_region_updater();
        me.install_selftest();
        me.install_driver();

        // Reachable from the header menu; the design has no room for a second
        // button and this is a developer affordance, not a primary action.
        {
            let me2 = me.clone();
            let theme = me.config.borrow().theme;
            let action = gio::SimpleAction::new_stateful(
                "theme",
                Some(glib::VariantTy::STRING),
                &theme.id().to_variant(),
            );
            action.connect_activate(move |action, value| {
                let Some(chosen) = value.and_then(|v| v.str().map(str::to_owned)) else {
                    return;
                };
                let Some(theme) = Theme::from_id(&chosen) else {
                    return;
                };
                action.set_state(&chosen.to_variant());
                me2.config.borrow_mut().theme = theme;
                me2.apply_theme();
                me2.persist();
            });
            window.add_action(&action);
        }

        {
            let me2 = me.clone();
            let mode = me.mode.get();
            let action = gio::SimpleAction::new_stateful(
                "mode",
                Some(glib::VariantTy::STRING),
                &mode.id().to_variant(),
            );
            action.connect_activate(move |action, value| {
                let Some(chosen) = value.and_then(|v| v.str().map(str::to_owned)) else {
                    return;
                };
                let Some(mode) = Mode::from_id(&chosen) else {
                    return;
                };
                if me2.state.borrow().is_active() {
                    me2.status.set_text("finish the current recording first");
                    return;
                }
                action.set_state(&chosen.to_variant());
                me2.mode.set(mode);
                me2.config.borrow_mut().mode = mode;
                me2.persist();
                me2.refresh();
            });
            window.add_action(&action);
        }

        {
            let me2 = me.clone();
            let action = gio::SimpleAction::new("choose-folder", None);
            action.connect_activate(move |_, _| me2.choose_output_folder());
            window.add_action(&action);
        }

        {
            let me2 = me.clone();
            let action = gio::SimpleAction::new("show-rect", None);
            action.connect_activate(move |_, _| {
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
            window.add_action(&action);
        }

        {
            let me2 = me.clone();
            let action = gio::SimpleAction::new_stateful(
                "format",
                Some(glib::VariantTy::STRING),
                &OutputFormat::default().extension().to_variant(),
            );
            action.connect_activate(move |action, value| {
                let Some(chosen) = value.and_then(|v| v.str().map(str::to_owned)) else {
                    return;
                };
                let Some(format) = OutputFormat::all()
                    .into_iter()
                    .find(|f| f.extension() == chosen)
                else {
                    return;
                };
                // Changing format mid-session would leave the destination and the
                // encoder disagreeing, so it is refused rather than half-applied.
                if me2.state.borrow().is_active() {
                    me2.status
                        .set_text("finish the current recording before changing format");
                    return;
                }
                action.set_state(&chosen.to_variant());
                me2.format.set(format);
                me2.chip.set_label(format.label());
                me2.config.borrow_mut().format = format;
                me2.persist();
            });
            window.add_action(&action);
        }

        {
            let me2 = me.clone();
            reveal.connect_clicked(move |_| {
                let Some(path) = me2.last_output.borrow().clone() else {
                    return;
                };
                let dir = path.parent().unwrap_or(&path).to_path_buf();
                let uri = format!("file://{}", dir.display());
                if let Err(e) =
                    gio::AppInfo::launch_default_for_uri(&uri, gio::AppLaunchContext::NONE)
                {
                    eprintln!("glimpse: could not open {uri}: {e}");
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
        if mode == "snapshot" {
            let me = self.clone();
            glib::timeout_add_seconds_local_once(2, move || {
                println!("[smoke] pressing Snapshot");
                me.mode.set(Mode::Snapshot);
                me.on_record_clicked();
                let me2 = me.clone();
                glib::timeout_add_seconds_local_once(3, move || {
                    println!("[smoke] status: {}", me2.status.text());
                    if let Some(a) = me2.window.application() {
                        a.quit();
                    }
                });
            });
            return;
        }
        if mode == "record" || mode == "record-mp4" {
            if mode == "record-mp4" {
                self.format.set(OutputFormat::Mp4);
                self.chip.set_label(OutputFormat::Mp4.label());
            }
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
    /// Repaint with the palette the current setting resolves to.
    ///
    /// `System` asks GTK for the desktop's dark-mode preference; an explicit
    /// choice also sets that preference, so GTK's own surfaces — the menu, the
    /// folder chooser — match rather than fighting the window they came from.
    fn apply_theme(&self) {
        let theme = self.config.borrow().theme;
        let settings = gtk::Settings::default();

        let dark = match theme {
            Theme::Dark => true,
            Theme::Light => false,
            Theme::System => settings
                .as_ref()
                .map(|s| s.is_gtk_application_prefer_dark_theme())
                .unwrap_or(true),
        };

        if theme != Theme::System {
            if let Some(s) = &settings {
                s.set_gtk_application_prefer_dark_theme(dark);
            }
        }

        self.css
            .load_from_data(&stylesheet(if dark { &DARK } else { &LIGHT }));
    }

    /// Write settings out now rather than at exit: a screen recorder is the kind
    /// of application people close abruptly, and a preference that only survives
    /// a clean shutdown is a preference that does not survive.
    fn persist(&self) {
        if let Err(e) = self.config.borrow().save() {
            eprintln!("glimpse: could not save settings: {e}");
        }
    }

    /// Ask for a directory to write recordings into. Only the directory is the
    /// user's choice; Glimpse still names the file, and still disambiguates
    /// rather than overwriting.
    fn choose_output_folder(self: &Rc<Self>) {
        let dialog = gtk::FileDialog::builder()
            .title("Save recordings to")
            .accept_label("Select")
            .build();
        let current = self.config.borrow().output_dir.clone();
        if current.is_dir() {
            dialog.set_initial_folder(Some(&gio::File::for_path(&current)));
        }

        let me = self.clone();
        dialog.select_folder(
            Some(&self.window),
            None::<&gio::Cancellable>,
            move |result| match result {
                Ok(file) => {
                    let Some(path) = file.path() else { return };
                    me.config.borrow_mut().output_dir = path.clone();
                    me.persist();
                    me.status.set_text(&format!(
                        "recordings will be saved to {}",
                        display_path(&path)
                    ));
                }
                // Cancelling is not an error worth reporting.
                Err(e) if e.matches(gtk::DialogError::Dismissed) => {}
                Err(e) => me.status.set_text(&format!("could not set folder: {e}")),
            },
        );
    }

    /// Grab one frame of the current region, off the UI thread.
    fn take_snapshot(self: &Rc<Self>) {
        if self.encoder.borrow().is_some() {
            self.status.set_text("still finishing the last one");
            return;
        }
        let rect = match capture_rect(&self.window, &self.hole, &self.probe) {
            Ok(r) if r.is_capturable() => r,
            Ok(r) => {
                self.status
                    .set_text(&format!("frame is off-screen ({}x{})", r.w, r.h));
                return;
            }
            Err(e) => {
                self.status.set_text(&format!("geometry error: {e}"));
                return;
            }
        };
        let display = match RecorderConfig::display_from_env() {
            Ok(d) => d,
            Err(e) => {
                self.status.set_text(&format!("{e:#}"));
                return;
            }
        };

        let cfg = self.config.borrow();
        let recorder = RecorderConfig {
            display,
            rect,
            framerate: cfg.framerate,
            capture_mouse: cfg.capture_mouse,
        };
        let destination = cfg.snapshot_destination();
        drop(cfg);

        self.status.set_text("capturing…");
        self.record.set_sensitive(false);
        *self.encoder.borrow_mut() = Some(FileJob::spawn(move || {
            crate::capture::snapshot(&recorder, &destination)
        }));
    }

    fn finish_snapshot(&self, result: Result<PathBuf, String>) {
        self.encoder.borrow_mut().take();
        self.record.set_sensitive(true);
        match result {
            Ok(path) => {
                self.status
                    .set_text(&format!("saved {}", display_path(&path)));
                *self.last_output.borrow_mut() = Some(path);
                self.status_dot.set_visible(true);
                self.reveal.set_visible(true);
            }
            Err(e) => {
                self.status.set_text(&e);
                self.status_dot.set_visible(false);
                self.reveal.set_visible(false);
            }
        }
    }

    fn state(&self) -> State {
        self.state.borrow().clone()
    }

    fn on_record_clicked(self: &Rc<Self>) {
        // Snapshot is not a one-frame recording: no session, no lifecycle, no
        // stop. It only makes sense when nothing is in flight.
        if self.mode.get() == Mode::Snapshot && !self.state.borrow().is_active() {
            self.take_snapshot();
            return;
        }
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
                let format = self.format.get();
                let cfg = self.config.borrow();
                let request = CaptureRequest {
                    rect,
                    framerate: cfg.framerate,
                    capture_mouse: cfg.capture_mouse,
                    destination: cfg.destination(),
                    format,
                };
                drop(cfg);
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
                *self.workspace.borrow_mut() = Some(workspace.root().to_path_buf());
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

            Effect::StartEncoder {
                source,
                destination,
            } => {
                // The recording is finished and its child is gone; release the
                // recorder so its workspace is not held open during the encode.
                self.worker.borrow_mut().take();
                let (src, format) = (source.path.clone(), self.format.get());
                *self.encoder.borrow_mut() = Some(FileJob::spawn(move || {
                    crate::encode::encode(&src, &destination, format)
                }));
                None
            }

            Effect::Cleanup { preserve_source } => {
                // Dropping the worker joins its thread, which guarantees the
                // child is dead and reaped before anything else happens.
                self.worker.borrow_mut().take();
                self.encoder.borrow_mut().take();
                let workspace = self.workspace.borrow_mut().take();
                if !preserve_source {
                    if let Some(root) = workspace {
                        if let Err(e) = std::fs::remove_dir_all(&root) {
                            eprintln!("glimpse: could not remove {}: {e}", root.display());
                        }
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

                let job = me.encoder.borrow().as_ref().and_then(|w| w.poll());
                if let Some(e) = job {
                    // A snapshot has no session, so its result is reported
                    // directly rather than fed to the state machine.
                    let snapshotting = matches!(&*me.state.borrow(), State::Idle);
                    match (e, snapshotting) {
                        (JobEvent::Finished(path), true) => me.finish_snapshot(Ok(path)),
                        (JobEvent::Failed(msg), true) => me.finish_snapshot(Err(msg)),
                        (JobEvent::Finished(path), false) => {
                            me.dispatch(Event::EncoderFinished(path))
                        }
                        (JobEvent::Failed(msg), false) => me.dispatch(Event::EncoderFailed(msg)),
                    }
                }

                me.update_size_label();
                me.tick_elapsed();

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

    /// Push the current state into the widgets.
    ///
    /// The single writer of visual state, so the button label, the border colour
    /// and the status line cannot disagree with the machine. Visual states are
    /// the four in the design document: idle, recording, saved, aborted.
    fn refresh(&self) {
        for c in [
            "state-idle",
            "state-recording",
            "state-stopping",
            "state-aborted",
        ] {
            self.shell.remove_css_class(c);
        }

        let state = self.state.borrow().clone();

        self.update_size_label();

        let idle_label = self.mode.get().label();
        let idle_hint = match self.mode.get() {
            Mode::Record => "Position the frame, then Record.",
            Mode::Snapshot => "Position the frame, then Snapshot.",
        };
        let (class, action, status, sensitive) = match &state {
            State::Idle => ("state-idle", idle_label, idle_hint.to_string(), true),
            State::Arming { .. } => ("state-idle", "Cancel", "arming…".to_string(), true),
            State::Recording { request } => (
                "state-recording",
                "Stop",
                format!("recording {} × {}", request.rect.w, request.rect.h),
                true,
            ),
            State::Stopping { .. } => (
                "state-stopping",
                "Stop",
                "finishing the file…".to_string(),
                false,
            ),
            State::Encoding { .. } => ("state-idle", "Stop", "encoding…".to_string(), false),
            State::Completed { output } => (
                "state-idle",
                idle_label,
                format!("saved {}", display_path(output)),
                true,
            ),
            State::Failed { error, retryable } => {
                let kept = retryable
                    .as_ref()
                    .map(|v| format!(" — recording kept at {}", v.path.display()))
                    .unwrap_or_default();
                ("state-aborted", idle_label, format!("{error}{kept}"), true)
            }
            State::Cancelled { preserved } => {
                let kept = preserved
                    .as_ref()
                    .map(|v| format!(" — recording kept at {}", v.path.display()))
                    .unwrap_or_default();
                (
                    "state-aborted",
                    idle_label,
                    format!("cancelled{kept}"),
                    true,
                )
            }
        };

        self.shell.add_css_class(class);
        self.record_label.set_text(action);
        // The bullet is a circle for Record and a square for Stop; a snapshot is
        // neither, so it gets the camera-shutter ring.
        self.bullet
            .set_visible(!matches!(&state, State::Idle) || self.mode.get() == Mode::Record);
        self.record.set_sensitive(sensitive);
        self.status.set_text(&status);

        let recording = matches!(state, State::Recording { .. });
        self.rec_dot.set_visible(recording);
        self.elapsed.set_visible(recording);
        if !recording {
            self.started.set(None);
        }

        let completed = matches!(state, State::Completed { .. });
        self.status_dot
            .set_visible(completed || class == "state-aborted");
        self.reveal.set_visible(completed);
        if let State::Completed { output } = &state {
            *self.last_output.borrow_mut() = Some(output.clone());
        }
    }

    /// Update the dimensions readout.
    ///
    /// Driven from the tick rather than only from `refresh`, because the frame is
    /// resized by the window manager — the size changes with no state transition
    /// to hang a redraw off, and `refresh` at construction time runs before the
    /// window is realized, when `capture_rect` cannot answer at all.
    fn update_size_label(&self) {
        // While a session owns the geometry the readout must show what is being
        // recorded, not what the window happens to measure now.
        let rect = self
            .frozen
            .get()
            .or_else(|| capture_rect(&self.window, &self.hole, &self.probe).ok());
        if let Some(r) = rect {
            let text = format!("{} × {}", r.w, r.h);
            if self.size_label.text() != text {
                self.size_label.set_text(&text);
            }
        }
    }

    /// Update the elapsed readout. Called from the driver, not from `refresh`,
    /// because it changes without the state changing.
    fn tick_elapsed(&self) {
        if !matches!(&*self.state.borrow(), State::Recording { .. }) {
            return;
        }
        let started = match self.started.get() {
            Some(t) => t,
            None => {
                let now = std::time::Instant::now();
                self.started.set(Some(now));
                now
            }
        };
        let secs = started.elapsed().as_secs();
        self.elapsed
            .set_text(&format!("{}:{:02}", secs / 60, secs % 60));
    }
}

/// Shorten a path under `$HOME` to `~/…`, as the design shows it.
fn display_path(p: &std::path::Path) -> String {
    let text = p.display().to_string();
    match std::env::var("HOME") {
        Ok(home) if !home.is_empty() && text.starts_with(&home) => {
            format!("~{}", &text[home.len()..])
        }
        _ => text,
    }
}

/// Thickness of the invisible grab strips along each edge, in logical pixels.
/// Wide enough to hit without aiming, narrow enough not to eat the frame border.
const RESIZE_GRIP: i32 = 8;
/// Corner grips are square and take priority over the edges they overlap.
const RESIZE_CORNER: i32 = 16;

/// Give the window its own resize edges.
///
/// GTK provides these for decorated windows and — measured on gala — for neither
/// an undecorated window nor one with a replaced titlebar. Since the frame's size
/// *is* the capture region, resizing is not a nicety here, so Glimpse draws the
/// grips itself and asks the compositor to take over the drag through
/// `Toplevel::begin_resize`.
fn install_resize_edges(window: &gtk::ApplicationWindow, overlay: &gtk::Overlay) {
    use gtk::gdk::SurfaceEdge;
    use gtk::{Align, Box as GtkBox, Orientation};

    // (edge, halign, valign, width, height, cursor)
    let grips: [(SurfaceEdge, Align, Align, i32, i32, &str); 8] = [
        (
            SurfaceEdge::North,
            Align::Fill,
            Align::Start,
            -1,
            RESIZE_GRIP,
            "n-resize",
        ),
        (
            SurfaceEdge::South,
            Align::Fill,
            Align::End,
            -1,
            RESIZE_GRIP,
            "s-resize",
        ),
        (
            SurfaceEdge::West,
            Align::Start,
            Align::Fill,
            RESIZE_GRIP,
            -1,
            "w-resize",
        ),
        (
            SurfaceEdge::East,
            Align::End,
            Align::Fill,
            RESIZE_GRIP,
            -1,
            "e-resize",
        ),
        // Corners last so they stack above the edges they overlap.
        (
            SurfaceEdge::NorthWest,
            Align::Start,
            Align::Start,
            RESIZE_CORNER,
            RESIZE_CORNER,
            "nw-resize",
        ),
        (
            SurfaceEdge::NorthEast,
            Align::End,
            Align::Start,
            RESIZE_CORNER,
            RESIZE_CORNER,
            "ne-resize",
        ),
        (
            SurfaceEdge::SouthWest,
            Align::Start,
            Align::End,
            RESIZE_CORNER,
            RESIZE_CORNER,
            "sw-resize",
        ),
        (
            SurfaceEdge::SouthEast,
            Align::End,
            Align::End,
            RESIZE_CORNER,
            RESIZE_CORNER,
            "se-resize",
        ),
    ];

    for (edge, halign, valign, w, h, cursor) in grips {
        let grip = GtkBox::new(Orientation::Horizontal, 0);
        grip.set_halign(halign);
        grip.set_valign(valign);
        if w > 0 {
            grip.set_size_request(w, -1);
        }
        if h > 0 {
            grip.set_size_request(grip.width_request(), h);
        }
        grip.set_cursor_from_name(Some(cursor));
        if std::env::var("GLIMPSE_DEBUG_GRIPS").is_ok() {
            grip.add_css_class("glimpse-grip-debug");
        }

        let gesture = gtk::GestureClick::new();
        gesture.set_button(gtk::gdk::BUTTON_PRIMARY);
        let win = window.clone();
        let grip_ref = grip.clone();
        gesture.connect_pressed(move |g, _, x, y| {
            let Some(surface) = win.surface() else {
                return;
            };
            let Ok(toplevel) = surface.downcast::<gtk::gdk::Toplevel>() else {
                return;
            };
            // begin_resize wants surface coordinates; the gesture reports widget
            // ones. Same chain as `geometry.rs`, for the same reason.
            let point = gtk::graphene::Point::new(x as f32, y as f32);
            let Some(in_window) = grip_ref.compute_point(&win, &point) else {
                return;
            };
            let (tx, ty) = win.surface_transform();
            toplevel.begin_resize(
                edge,
                g.device().as_ref(),
                g.current_button() as i32,
                in_window.x() as f64 + tx,
                in_window.y() as f64 + ty,
                g.current_event_time(),
            );
            // Hand the sequence back: GTK's implicit grab would otherwise keep
            // holding the pointer and the compositor's resize grab never starts.
            g.set_state(gtk::EventSequenceState::Denied);
        });
        grip.add_controller(gesture);
        overlay.add_overlay(&grip);
    }
}
