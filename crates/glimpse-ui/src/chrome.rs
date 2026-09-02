//! The chrome: the header, the status bar, and the controller that drives them.
//!
//! Shared between frontends. **The window model is not** — that is
//! [ADR 0014](../../../docs/adr/0014-the-chrome-is-shared-the-window-model-is-not.md),
//! and it is why `new` takes two closures instead of assembling a window itself:
//!
//! ```text
//! X11                              macOS (ADR 0015)
//! one window                       two windows
//!   header                           chrome window: header, rule, status, sheet
//!   rule                             frame window:  border, click-through
//!   frame -> hole
//!   status
//! ```
//!
//! The pieces above are identical on both. Where they are *put* is not, so the
//! caller supplies `assemble`, and the platform facts the controller needs
//! arrive as [`PlatformHooks`].
//!
//! The frame border lives on `frame`, never on `hole`. `compute_bounds` returns a
//! widget's *border box*, so a border on the capture target would be recorded as
//! part of every GIF. The spike in ADR 0000 hit exactly that, and it survived an
//! `xwininfo` cross-check. Separating the two removes the bug class instead of
//! compensating for it with a magic inset.

use anyhow::{anyhow, Result};
use gtk::prelude::*;
use gtk::{gio, glib};
use gtk4 as gtk;
use std::cell::{Cell, RefCell};
use std::path::PathBuf;
use std::rc::Rc;

use crate::{display_path, human_size, stylesheet, PlatformHooks, DARK, LIGHT};
use glimpse_core::capture::{GrabRequest, Workspace};
use glimpse_core::config::{Config, Mode, Theme};
use glimpse_core::encode::{Canceller, OutputFormat, Progress};
use glimpse_core::geometry::ScreenPixelRect;
use glimpse_core::session::{transition, CaptureRequest, Effect, Event, State};
use glimpse_core::worker::{FileJob, JobEvent, RecordingWorker, WorkerEvent};

/// Where the capture region lives, relative to the chrome.
///
/// The one thing the two window models genuinely disagree about, so it is a
/// parameter rather than an assumption. See
/// [ADR 0015](../../../docs/adr/0015-the-frame-is-two-windows.md).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Hole {
    /// A hole in this same window, with clicks passing through it via an input
    /// region. X11.
    InChrome,
    /// A separate window that takes no clicks at all. macOS.
    Elsewhere,
}

/// The chrome's pieces, in the order X11 stacks them.
///
/// Handed to `assemble` so a platform can distribute them between windows
/// instead of only choosing whether to include the hole. macOS puts `header` and
/// `rule` above the frame and `status` and `sheet` below it
/// ([ADR 0016](../../../docs/adr/0016-the-chrome-is-above-and-below.md)); X11
/// leaves them where they are.
///
/// Borrowed, not owned: they are already parented to the shell, and a platform
/// that wants one elsewhere must `remove` it first — which GTK requires anyway,
/// since a widget has one parent.
pub struct ChromeParts<'a> {
    pub shell: &'a gtk::Box,
    pub header: &'a gtk::Widget,
    pub rule: &'a gtk::Widget,
    pub status: &'a gtk::Widget,
    pub sheet: &'a gtk::Widget,
}

pub struct Chrome {
    pub window: gtk::ApplicationWindow,
    /// Everything this controller needs from X11, as closures.
    ///
    /// The controller below is 99.4% platform-free — 12 lines of 1948 named X11
    /// before this field existed — so the coupling is collected here rather than
    /// scattered, which is what lets the chrome move to `glimpse-ui` unchanged.
    /// See [ADR 0014](../../../docs/adr/0014-the-chrome-is-shared-the-window-model-is-not.md).
    hooks: Rc<PlatformHooks>,
    /// The rect snapshotted when locking, per ADR 0002. `Some` means a session
    /// owns the geometry.
    frozen: Cell<Option<ScreenPixelRect>>,
    /// The lifecycle. Every transition goes through `glimpse_core::session::transition`,
    /// so the policies stay in the tested pure module rather than in callbacks.
    state: RefCell<State>,
    /// Owns the ffmpeg child while a recording is live. Dropping it reaps.
    worker: RefCell<Option<RecordingWorker>>,
    /// Owns the encode, or the snapshot, while one is running.
    encoder: RefCell<Option<FileJob>>,
    /// Stops the running encode. Replaced per job; cancelling a finished one is
    /// harmless.
    cancel_encode: RefCell<Canceller>,
    /// How far the running encode has got. Replaced per job.
    encode_progress: RefCell<Progress>,
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
    /// Persisted user settings. Written on every change rather than on exit,
    /// because a screen recorder is the kind of thing people close abruptly.
    config: RefCell<Config>,
    css: gtk::CssProvider,
    /// What the primary button does when clicked.
    mode: Cell<Mode>,
    bullet: gtk::Box,
    chip: gtk::Label,
    rec_label: gtk::Label,
    rule: gtk::Box,
    progress: gtk::ProgressBar,
    sheet: gtk::Box,
    sheet_title: gtk::Label,
    sheet_path: gtk::Label,
    reveal_sheet: gtk::Button,
    retry: gtk::Button,
    status_bar: gtk::Box,
}

impl Chrome {
    /// Build the chrome, let the platform assemble it into whatever window model
    /// it has, and adopt the hooks it hands back.
    ///
    /// Two closures rather than two methods because both need widgets that only
    /// exist part-way through this function: `assemble` needs the shell, and
    /// `make_hooks` needs the window and the hole. Returning half-built widgets
    /// so a caller could finish the job would make it possible to forget.
    pub fn new(
        app: &gtk::Application,
        hole_is: Hole,
        make_hooks: impl FnOnce(&gtk::ApplicationWindow, &gtk::Box) -> PlatformHooks,
        assemble: impl FnOnce(&gtk::ApplicationWindow, ChromeParts),
    ) -> Rc<Self> {
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
        //
        // One definition, in `system_decorations`. This was two separate reads of
        // the same variable with the comparison spelled out twice, which agreed
        // only because both spellings happened to match — and the platform needs
        // the answer too, to decide about resize edges.
        let system_decorations = crate::system_decorations();
        window.set_decorated(system_decorations);

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

        // The third recording cue, alongside the tinted header and the promoted
        // timer. Three of them, because the window's middle is invisible.
        let rec_label = gtk::Label::new(Some("REC"));
        rec_label.add_css_class("glimpse-rec-label");
        rec_label.set_visible(false);

        let meta = gtk::Box::new(gtk::Orientation::Horizontal, 10);
        meta.set_hexpand(true);
        meta.set_halign(gtk::Align::Start);
        meta.append(&rec_dot);
        meta.append(&elapsed);
        meta.append(&size_label);

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

        // Read-only: it reports the active format rather than competing with the
        // split button for the same corner of the header. Changing it lives in
        // settings, where the other capture options already are.
        let chip = gtk::Label::new(Some(config.format.label()));
        chip.add_css_class("glimpse-chip");
        chip.set_valign(gtk::Align::Center);

        let settings = gtk::Popover::new();
        settings.set_has_arrow(true);
        let menu = gtk::MenuButton::builder()
            .icon_name("open-menu-symbolic")
            .popover(&settings)
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
        status_bar.append(&rec_label);
        status_bar.append(&status);
        status_bar.append(&reveal);

        // The hairline under the header. While encoding it is replaced by a
        // determinate bar of the same height, so progress costs no chrome.
        let rule = gtk::Box::new(gtk::Orientation::Horizontal, 0);
        rule.add_css_class("glimpse-rule");
        let progress = gtk::ProgressBar::new();
        progress.add_css_class("glimpse-progress");
        progress.set_visible(false);

        let rule_stack = gtk::Box::new(gtk::Orientation::Horizontal, 0);
        rule_stack.append(&rule);
        rule_stack.append(&progress);
        rule.set_hexpand(true);
        progress.set_hexpand(true);

        // Results leave the one-line strip: a taller sheet takes its place so a
        // path gets a full monospace line and real buttons, and it is simply not
        // there when idle.
        let sheet_title = gtk::Label::new(None);
        sheet_title.add_css_class("glimpse-sheet-title");
        sheet_title.set_xalign(0.0);
        let sheet_path = gtk::Label::new(None);
        sheet_path.add_css_class("glimpse-path");
        sheet_path.set_xalign(0.0);
        sheet_path.set_ellipsize(gtk::pango::EllipsizeMode::Start);
        sheet_path.set_selectable(true);

        let sheet_text = gtk::Box::new(gtk::Orientation::Vertical, 2);
        sheet_text.set_hexpand(true);
        sheet_text.set_valign(gtk::Align::Center);
        sheet_text.append(&sheet_title);
        sheet_text.append(&sheet_path);

        let copy_path = gtk::Button::with_label("Copy Path");
        copy_path.add_css_class("glimpse-sheet-button");
        copy_path.set_valign(gtk::Align::Center);
        let reveal_sheet = gtk::Button::with_label("Show in Files");
        reveal_sheet.add_css_class("glimpse-sheet-button");
        reveal_sheet.set_valign(gtk::Align::Center);

        let retry = gtk::Button::with_label("Encode Anyway");
        retry.add_css_class("glimpse-sheet-button");
        retry.set_valign(gtk::Align::Center);
        retry.set_visible(false);

        let sheet_actions = gtk::Box::new(gtk::Orientation::Horizontal, 6);
        sheet_actions.set_valign(gtk::Align::Center);
        sheet_actions.append(&copy_path);
        sheet_actions.append(&retry);
        sheet_actions.append(&reveal_sheet);

        let sheet = gtk::Box::new(gtk::Orientation::Horizontal, 12);
        sheet.add_css_class("glimpse-sheet");
        sheet.append(&sheet_text);
        sheet.append(&sheet_actions);
        sheet.set_visible(false);

        let shell = gtk::Box::new(gtk::Orientation::Vertical, 0);
        shell.add_css_class("glimpse-shell");
        shell.add_css_class("state-idle");
        shell.append(&header_handle);
        shell.append(&rule_stack);
        // The capture region is only part of THIS window on X11. On macOS it is
        // a second, click-through window, so the chrome must not reserve space
        // for a hole that is not in it — appending the frame there would push
        // the header and status bar apart around empty space.
        if hole_is == Hole::InChrome {
            shell.append(&frame);
        }
        shell.append(&status_bar);
        shell.append(&sheet);

        // How this becomes a window is the platform's business: X11 wraps it in
        // an Overlay carrying resize edges and makes it the single window's
        // child; macOS puts it in the chrome window and leaves the hole to a
        // second, click-through one.
        assemble(
            &window,
            ChromeParts {
                shell: &shell,
                header: header_handle.upcast_ref(),
                rule: rule_stack.upcast_ref(),
                status: status_bar.upcast_ref(),
                sheet: sheet.upcast_ref(),
            },
        );

        // Built here, where `window` and `hole` are still locals, so the closures
        // capture clones rather than borrowing the struct being built. GTK
        // objects are reference-counted, so a clone is a handle to the same
        // widget and not a copy of one.
        let hooks = Rc::new(make_hooks(&window, &hole));

        let me = Rc::new(Self {
            window: window.clone(),
            hooks,
            frozen: Cell::new(None),
            state: RefCell::new(State::Idle),
            worker: RefCell::new(None),
            encoder: RefCell::new(None),
            cancel_encode: RefCell::new(Canceller::new()),
            encode_progress: RefCell::new(Progress::new()),
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
            chip: chip.clone(),
            rec_label: rec_label.clone(),
            rule: rule.clone(),
            progress: progress.clone(),
            sheet: sheet.clone(),
            sheet_title: sheet_title.clone(),
            sheet_path: sheet_path.clone(),
            reveal_sheet: reveal_sheet.clone(),
            retry: retry.clone(),
            status_bar: status_bar.clone(),
            mode: Cell::new(config.mode),
            bullet: bullet.clone(),
            config: RefCell::new(config),
            css: css.clone(),
        });

        {
            // Esc stops a recording and Print Screen takes a snapshot. Both are
            // advertised in the status strip, so they have to exist.
            let me2 = me.clone();
            let keys = gtk::EventControllerKey::new();
            keys.connect_key_pressed(move |_, key, _, _| {
                use gtk::gdk::Key;
                match key {
                    Key::Escape if matches!(&*me2.state.borrow(), State::Recording { .. }) => {
                        me2.dispatch(Event::Stop);
                        glib::Propagation::Stop
                    }
                    Key::Print if !me2.state.borrow().is_active() => {
                        me2.take_snapshot();
                        glib::Propagation::Stop
                    }
                    _ => glib::Propagation::Proceed,
                }
            });
            me.window.add_controller(keys);
        }

        me.build_settings(&settings);
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
            let on = me.config.borrow().capture_mouse;
            let action = gio::SimpleAction::new_stateful("capture-mouse", None, &on.to_variant());
            action.connect_activate(move |action, _| {
                if me2.state.borrow().is_active() {
                    me2.status.set_text("finish the current recording first");
                    return;
                }
                let next = !action.state().and_then(|s| s.get::<bool>()).unwrap_or(true);
                action.set_state(&next.to_variant());
                me2.config.borrow_mut().capture_mouse = next;
                me2.persist();
                me2.status.set_text(if next {
                    "the pointer will be captured"
                } else {
                    "the pointer will not be captured"
                });
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
            copy_path.connect_clicked(move |_| {
                let Some(path) = me2.last_output.borrow().clone() else {
                    return;
                };
                if let Some(display) = gtk::gdk::Display::default() {
                    display.clipboard().set_text(&path.display().to_string());
                    me2.sheet_title.set_text("Path copied");
                }
            });
        }

        {
            let me2 = me.clone();
            reveal_sheet.connect_clicked(move |_| me2.open_containing_folder());
        }

        {
            let me2 = me.clone();
            retry.connect_clicked(move |_| {
                let destination = me2.config.borrow().destination();
                me2.dispatch(Event::Retry { destination });
            });
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
    pub fn lock(&self) -> Result<ScreenPixelRect> {
        let rect = (self.hooks.capture_rect)()?;
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

    pub fn frozen_rect(&self) -> Option<ScreenPixelRect> {
        self.frozen.get()
    }

    /// Has the frame moved since it was locked?
    ///
    /// `lock()` disables resizing, but **a window manager can still move the
    /// window** — alt-drag, a workspace change, a tiling rule. `x11grab` records a
    /// fixed root rectangle, so an undetected move means the visible frame and the
    /// recording diverge while the output still looks plausible. Enforcement is a
    /// checked invariant, not an assumption about what GTK can prevent.
    pub fn geometry_drifted(&self) -> Option<(ScreenPixelRect, ScreenPixelRect)> {
        let frozen = self.frozen.get()?;
        let now = (self.hooks.capture_rect)().ok()?;
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
        // Connecting to the surface is X11's business and stays here. What
        // happens when the geometry settles goes through the hook, so the
        // controller half of this is the same sentence on both platforms — on
        // macOS it does nothing, because its frame window took no clicks from the
        // moment it was created (ADR 0015).
        let hooks = self.hooks.clone();
        self.window.connect_realize(move |win| {
            let Some(surface) = win.surface() else {
                eprintln!("glimpse: realized with no surface; click-through disabled");
                return;
            };
            (hooks.geometry_settled)();

            let hooks = hooks.clone();
            surface.connect_layout(move |_, _, _| (hooks.geometry_settled)());
        });
    }
}

impl Chrome {
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
        // Records, then cancels *during* the encode — the one thing the
        // deterministic tests cannot show, because they cannot hit the window.
        if mode == "cancel-encode" {
            let me = self.clone();
            glib::timeout_add_seconds_local_once(2, move || {
                me.on_record_clicked();
                let me2 = me.clone();
                glib::timeout_add_seconds_local_once(3, move || {
                    me2.on_record_clicked(); // Stop
                    let me3 = me2.clone();
                    // Fire while the encoder is still working.
                    glib::timeout_add_local_once(
                        std::time::Duration::from_millis(400),
                        move || {
                            println!("[smoke] state before cancel: {:?}", me3.state());
                            println!("[smoke] ffmpeg alive: {}", ffmpeg_count());
                            me3.on_record_clicked(); // Cancel
                            let me4 = me3.clone();
                            glib::timeout_add_seconds_local_once(2, move || {
                                println!("[smoke] state after cancel:  {:?}", me4.state());
                                println!("[smoke] ffmpeg alive: {}", ffmpeg_count());
                                println!("[smoke] status: {}", me4.status.text());
                                finish_journey(&me4.window);
                            });
                        },
                    );
                });
            });
            return;
        }
        // Cancel an encode, then press Encode Anyway and check a file appears.
        //
        // Waits for each state rather than sleeping a guessed interval — the
        // first attempt at this cancelled during Stopping, because how long
        // ffmpeg takes to finalise a container is not something to hardcode.
        if mode == "retry" {
            let me = self.clone();
            glib::timeout_add_seconds_local_once(2, move || {
                me.on_record_clicked(); // Record
                let m = me.clone();
                glib::timeout_add_seconds_local_once(3, move || {
                    m.on_record_clicked(); // Stop
                    let m2 = m.clone();
                    // poll until the encode is actually running
                    let ticks = std::cell::Cell::new(0);
                    glib::timeout_add_local(std::time::Duration::from_millis(100), move || {
                        ticks.set(ticks.get() + 1);
                        let encoding = matches!(&*m2.state.borrow(), State::Encoding { .. });
                        if !encoding && ticks.get() < 100 {
                            return glib::ControlFlow::Continue;
                        }
                        println!(
                            "[smoke] reached {:?} after {} ticks",
                            m2.state(),
                            ticks.get()
                        );
                        m2.on_record_clicked(); // Cancel
                        println!("[smoke] after cancel: {:?}", m2.state());
                        println!("[smoke] retry visible: {}", m2.retry.is_visible());
                        let m3 = m2.clone();
                        glib::timeout_add_seconds_local_once(1, move || {
                            m3.retry.emit_clicked();
                            let m4 = m3.clone();
                            glib::timeout_add_seconds_local_once(5, move || {
                                println!("[smoke] after retry: {:?}", m4.state());
                                finish_journey(&m4.window);
                            });
                        });
                        glib::ControlFlow::Break
                    });
                });
            });
            return;
        }
        if mode == "snapshot" {
            let me = self.clone();
            glib::timeout_add_seconds_local_once(2, move || {
                println!("[smoke] pressing Snapshot");
                me.mode.set(Mode::Snapshot);
                me.on_record_clicked();
                let me2 = me.clone();
                glib::timeout_add_seconds_local_once(3, move || {
                    println!("[smoke] status: {}", me2.status.text());
                    finish_journey(&me2.window);
                });
            });
            return;
        }
        if mode == "record" || mode == "record-mp4" {
            if mode == "record-mp4" {
                // Through the config, because that is the only place the format
                // lives now.
                self.config.borrow_mut().format = OutputFormat::Mp4;
                self.chip.set_text(OutputFormat::Mp4.label());
            }
            let me = self.clone();
            glib::timeout_add_seconds_local_once(2, move || {
                // State the mode, exactly as the snapshot branch does. The split
                // button REMEMBERS what you last chose and that choice persists
                // to config.toml (ADR 0009), so without this the record smoke
                // test does whatever the developer last clicked in the UI. With
                // `mode = "snapshot"` persisted it pressed Snapshot, which does
                // not go through the session machine, so the state stayed Idle,
                // nothing was recorded, and the run still exited 0 — a smoke
                // test that had quietly stopped testing recording and could not
                // say so.
                me.mode.set(Mode::Record);
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
                        finish_journey(&me3.window);
                    });
                });
            });
            return;
        }

        let me = self.clone();
        glib::timeout_add_seconds_local_once(3, move || {
            me.status.set_text("self-test running");
            println!("{}", run_selftest(&me.hooks));
            finish_journey(&me.window);
        });
    }
}

/// The report. Its exact line shape is a contract: `scripts/selftest.sh` greps
/// for `input shape  : PASS` and `grab         : wrote`, so the alignment below
/// is load-bearing rather than cosmetic. The platform-specific middle comes from
/// `diagnostics`, which supplies whole lines for that reason.
fn run_selftest(hooks: &PlatformHooks) -> String {
    let rect = match (hooks.capture_rect)() {
        Ok(r) => r,
        Err(e) => return format!("SELFTEST FAILED: geometry: {e:#}"),
    };

    let out = "/tmp/glimpse-selftest.png";
    let grab = match grab_through_the_shipping_path(hooks, rect, std::path::Path::new(out)) {
        Ok(()) => format!(
            "wrote {out} — INSPECT IT: any Glimpse chrome in the image means the rect is wrong"
        ),
        Err(e) => format!("FAILED: {e:#}"),
    };

    format!(
        "\n=== glimpse self-test ===\n\
         capture rect : {}x{} at {},{}\n\
         {}\
         grab         : {grab}\n",
        rect.w,
        rect.h,
        rect.x,
        rect.y,
        (hooks.diagnostics)(),
    )
}

/// Grab `rect` to `out`, through the same argument construction the app ships.
///
/// The point of routing this through the platform's `grab` hook rather than
/// assembling flags
/// here is that the self-test is the check of last resort — the one thing that
/// can see a misplaced rectangle, which the suite provably cannot (ADR 0000). A
/// check built on its own copy of the arguments can only ever vouch for that
/// copy.
///
/// It was not a hypothetical divergence. The inline version passed the region as
/// `DISPLAY+x,y` inside the input URL — precisely the form
/// `grab::tests::the_region_is_passed_as_documented_options_not_baked_into_the_url`
/// exists to forbid — and named no codec, so the PNG came out of ffmpeg
/// inferring one from the file extension. That inference is what wrote a JPEG
/// into a file called `.png` (ADR 0009) and an MP4 into one called `.gif`.
///
/// So this function deliberately contains **no ffmpeg flag strings at all**.
/// There is now exactly one place in the crate that knows how to spell an
/// `x11grab` invocation, which makes the divergence unpronounceable rather than
/// merely fixed — the same move as putting the border on the parent widget.
fn grab_through_the_shipping_path(
    hooks: &PlatformHooks,
    rect: ScreenPixelRect,
    out: &std::path::Path,
) -> Result<()> {
    // Through the hook for the same reason it went through the backend before:
    // there must be exactly one place that knows how to spell an invocation. The
    // hook is now that place, and it is also what the app itself records with.
    let command = (hooks.grab)(&GrabRequest {
        rect,
        // A single frame, so no capture rate — the same shape a snapshot takes.
        framerate: None,
        // Off, so the image is about geometry and nothing else. Left unstated by
        // the old inline version, which meant x11grab's default applied and the
        // pointer could land in the middle of a rectangle-alignment check.
        capture_mouse: false,
    })?;

    let output = std::process::Command::new("ffmpeg")
        .args(command.snapshot_args(out))
        .output()
        .map_err(|e| anyhow!("ffmpeg not spawnable: {e}"))?;
    if !output.status.success() {
        // Report the status when there is nothing on stderr. `-loglevel error`
        // means a silent failure is entirely possible, and "FAILED: ?" tells
        // whoever is looking at it nothing at all.
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(match stderr.lines().last() {
            Some(last) => anyhow!("{last}"),
            None => anyhow!("ffmpeg exited {} with no diagnostics", output.status),
        });
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// The controller: the impure half that drives the pure state machine.
// ---------------------------------------------------------------------------

/// Whether a finished self-test journey should leave the app on screen.
///
/// `GLIMPSE_SELFTEST_HOLD=1`. The journeys quit as soon as they finish, which is
/// right for CI — a smoke test that leaves a window behind hangs the job. It is
/// wrong for looking at the result: a screen grab of a macOS window takes a
/// couple of seconds through avfoundation, by which time the app has gone.
///
/// This exists because the macOS status bar sat on the wrong side of the frame
/// through three merged pull requests. Every automated check passed, because
/// every one of them asserted on state and output rather than on where things
/// were. The layout was only wrong to look at.
///
/// Anything but `0` holds. Matching `"1"` exactly meant `=true` silently did not
/// hold — a quiet failure in the same direction as the bug this flag was added
/// for, which is the one direction it must not fail in.
fn hold_after_selftest() -> bool {
    match std::env::var("GLIMPSE_SELFTEST_HOLD") {
        Ok(v) => !matches!(v.trim(), "" | "0"),
        Err(_) => false,
    }
}

/// End a self-test journey: quit, unless asked to hold.
///
/// Every journey ends here rather than deciding for itself. The decision was
/// copied at five journey endings, which is five chances for the sixth to forget
/// it — and forgetting is invisible, because the app simply vanishes before it
/// can be looked at, which is the failure the hold exists to prevent.
/// `check-journeys.sh` asserts this stays the only place a journey quits.
fn finish_journey(window: &gtk::ApplicationWindow) {
    if hold_after_selftest() {
        println!("[smoke] holding: GLIMPSE_SELFTEST_HOLD is set");
        return;
    }
    if let Some(app) = window.application() {
        app.quit();
    }
}

/// How often the driver looks for a finished recording and for a frame that has
/// moved. Fast enough that a drifted recording is cut within a fraction of a
/// second; slow enough to be invisible next to a 60Hz frame clock.
const DRIVER_TICK_MS: u32 = 100;

impl Chrome {
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
        let rect = match (self.hooks.capture_rect)() {
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
        let cfg = self.config.borrow();
        // `None` framerate: a snapshot is a single frame, not a one-frame
        // recording, so no capture rate is expressed at all.
        let grab = match (self.hooks.grab)(&GrabRequest {
            rect,
            framerate: None,
            capture_mouse: cfg.capture_mouse,
        }) {
            Ok(g) => g,
            Err(e) => {
                drop(cfg);
                self.status.set_text(&format!("{e:#}"));
                return;
            }
        };
        let destination = cfg.snapshot_destination();
        drop(cfg);

        self.status.set_text("capturing…");
        self.record.set_sensitive(false);
        *self.encoder.borrow_mut() = Some(FileJob::spawn(move || {
            glimpse_core::capture::snapshot(&grab, &destination)
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
        trace(|| format!("click in {}", short_state(&self.state.borrow())));
        // A job may have finished between the last driver tick and this click.
        // Without this, cancelling in that window reports "cancelled" while the
        // encode has already committed its file — the user is told one thing and
        // the filesystem says another.
        self.drain_jobs();

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
                let format = self.config.borrow().format;
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
            State::Encoding { .. } => self.cancel_encoding(),
            State::Arming { .. } => self.dispatch(Event::Cancel),
            // Stopping is the one uninterruptible window: ffmpeg has been asked
            // to finalise the container and interrupting that is how you get a
            // truncated file.
            State::Stopping { .. } => {
                self.status.set_text("finishing the file — one moment");
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
            let (next, effect) = transition(current.clone(), ev.clone());
            trace(|| {
                format!(
                    "{} + {:?} -> {} [{:?}]",
                    short_state(&current),
                    ev,
                    short_state(&next),
                    effect
                )
            });
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
                let grab = match (self.hooks.grab)(&GrabRequest {
                    rect: request.rect,
                    framerate: Some(request.framerate),
                    capture_mouse: request.capture_mouse,
                }) {
                    Ok(g) => g,
                    Err(e) => return Some(Event::RecorderFailed(format!("{e:#}"))),
                };
                let workspace = match Workspace::create() {
                    Ok(w) => w,
                    Err(e) => return Some(Event::RecorderFailed(format!("{e:#}"))),
                };
                *self.workspace.borrow_mut() = Some(workspace.root().to_path_buf());
                *self.worker.borrow_mut() = Some(RecordingWorker::start(grab, workspace));
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
                self.cancel_encode.borrow().cancel();
                None
            }

            Effect::StartEncoder {
                source,
                destination,
            } => {
                // The recording is finished and its child is gone; release the
                // recorder so its workspace is not held open during the encode.
                self.worker.borrow_mut().take();
                let (src, format) = (source.path.clone(), self.config.borrow().format);
                // The canceller must be the one `cancel_encoding` holds. Calling
                // the plain `encode` here silently makes the Cancel button
                // decorative: it fires a canceller wired to nothing, the encode
                // runs to completion, and the file is committed while the session
                // reports Cancelled.
                let canceller = Canceller::new();
                let progress = Progress::new();
                *self.cancel_encode.borrow_mut() = canceller.clone();
                *self.encode_progress.borrow_mut() = progress.clone();
                *self.encoder.borrow_mut() = Some(FileJob::spawn(move || {
                    glimpse_core::encode::encode_reporting(
                        &src,
                        &destination,
                        format,
                        &canceller,
                        &progress,
                    )
                }));
                None
            }

            Effect::Cleanup { preserve_source } => {
                self.cancel_encode.borrow().cancel();
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

    /// Cancel a running encode and settle the outcome before returning.
    ///
    /// The commit is atomic, so an encode either wrote its file or did not — and
    /// a cancel can arrive in the gap between the rename and the driver noticing.
    /// Polling for the result here rather than dispatching `Cancel` blind means
    /// the user is never told "cancelled" about a file that exists.
    ///
    /// This does block the UI thread, deliberately and briefly: the job returns
    /// as soon as it sees the flag, which is within one 25ms poll of `encode`.
    /// The bound exists so a wedged job degrades to the old behaviour instead of
    /// freezing the window.
    fn cancel_encoding(self: &Rc<Self>) {
        const SETTLE: std::time::Duration = std::time::Duration::from_millis(250);
        self.cancel_encode.borrow().cancel();

        let deadline = std::time::Instant::now() + SETTLE;
        while std::time::Instant::now() < deadline {
            let event = self.encoder.borrow().as_ref().and_then(|w| w.poll());
            match event {
                // It had already committed. Cancelling something that finished is
                // not a cancellation, whatever the button said.
                Some(JobEvent::Finished(path)) => {
                    trace(|| format!("settle: finished {}", path.display()));
                    self.dispatch(Event::EncoderFinished(path));
                    return;
                }
                Some(JobEvent::Failed(m)) => {
                    trace(|| format!("settle: failed {m}"));
                    break;
                }
                None => std::thread::sleep(std::time::Duration::from_millis(5)),
            }
        }
        self.dispatch(Event::Cancel);
    }

    /// Deliver any finished background result immediately, rather than waiting
    /// for the next driver tick.
    fn drain_jobs(self: &Rc<Self>) {
        let job = self.encoder.borrow().as_ref().and_then(|w| w.poll());
        let Some(event) = job else { return };
        trace(|| format!("drain: {event:?}"));
        let snapshotting = matches!(&*self.state.borrow(), State::Idle);
        match (event, snapshotting) {
            (JobEvent::Finished(path), true) => self.finish_snapshot(Ok(path)),
            (JobEvent::Failed(msg), true) => self.finish_snapshot(Err(msg)),
            (JobEvent::Finished(path), false) => self.dispatch(Event::EncoderFinished(path)),
            (JobEvent::Failed(msg), false) => self.dispatch(Event::EncoderFailed(msg)),
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

                // A snapshot has no session, so its result is reported directly
                // rather than fed to the state machine — `drain_jobs` knows the
                // difference.
                me.drain_jobs();

                me.update_size_label();
                me.tick_elapsed();
                if matches!(&*me.state.borrow(), State::Encoding { .. }) {
                    match me.encode_progress.borrow().fraction() {
                        // ffmpeg has reported; show how far.
                        Some(f) => {
                            me.progress.set_fraction(f);
                            // Below the handover the GIF encoder is still
                            // building its palette, which is worth naming: it is
                            // the slow half and it looks like nothing happening.
                            let gif = me.config.borrow().format == OutputFormat::Gif;
                            me.status.set_text(&if gif && f < 0.35 {
                                "Quantising palette · 256 colours".to_string()
                            } else {
                                format!("Encoding {}%", (f * 100.0).round() as u32)
                            });
                        }
                        // Nothing reported yet — pulse rather than sit at zero,
                        // which would claim no progress rather than no answer.
                        None => me.progress.pulse(),
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
            Mode::Snapshot => "One still frame, saved as PNG. · Print Screen",
        };
        let (class, action, status, sensitive) = match &state {
            State::Idle => ("state-idle", idle_label, idle_hint.to_string(), true),
            State::Arming { .. } => ("state-idle", "Cancel", "arming…".to_string(), true),
            State::Recording { request } => (
                "state-recording",
                "Stop",
                format!(
                    "{} fps · pointer {} · Esc to stop",
                    request.framerate,
                    if request.capture_mouse {
                        "captured"
                    } else {
                        "hidden"
                    }
                ),
                true,
            ),
            State::Stopping { .. } => (
                "state-stopping",
                "Stop",
                "finishing the file…".to_string(),
                false,
            ),
            State::Encoding { .. } => ("state-idle", "Cancel", "encoding…".to_string(), true),
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
        self.record.set_sensitive(sensitive);
        self.status.set_text(&status);
        self.chip.set_text(match self.mode.get() {
            // A still is always PNG, so reporting the recording format here would
            // be reporting something that does not apply.
            Mode::Snapshot => "PNG",
            Mode::Record => self.config.borrow().format.label(),
        });

        let recording = matches!(state, State::Recording { .. });
        self.rec_dot.set_visible(recording);
        self.elapsed.set_visible(recording);
        self.rec_label.set_visible(recording);
        if !recording {
            self.started.set(None);
        }

        // The bullet is a circle for Record and a square for Stop; a snapshot is
        // neither.
        self.bullet
            .set_visible(!matches!(&state, State::Idle) || self.mode.get() == Mode::Record);

        // Progress replaces the hairline while encoding, driven by ffmpeg's own
        // `-progress` output. It pulses only until the first report arrives: a
        // determinate bar at zero claims no progress, where the truth is no
        // answer yet.
        let encoding = matches!(state, State::Encoding { .. });
        self.progress.set_visible(encoding);
        self.rule.set_visible(!encoding);
        if !encoding {
            self.progress.set_fraction(0.0);
        }

        // A result gets the sheet; everything else gets the one-line strip.
        let (sheet_title, sheet_path, offer_reveal) = match &state {
            State::Completed { output } => (
                Some(match std::fs::metadata(output).map(|m| m.len()) {
                    Ok(bytes) => format!("Saved · {}", human_size(bytes)),
                    Err(_) => "Saved".to_string(),
                }),
                Some(display_path(output)),
                true,
            ),
            State::Failed { error, retryable } => (
                Some(error.clone()),
                retryable
                    .as_ref()
                    .map(|v| format!("Raw capture kept: {}", v.path.display())),
                false,
            ),
            State::Cancelled { preserved: Some(v) } => (
                Some("Cancelled".to_string()),
                Some(format!("Raw capture kept: {}", v.path.display())),
                false,
            ),
            _ => (None, None, false),
        };

        match sheet_title {
            Some(title) => {
                self.sheet_title.set_text(&title);
                self.sheet_path
                    .set_text(sheet_path.as_deref().unwrap_or(""));
                self.sheet_path.set_visible(sheet_path.is_some());
                self.reveal_sheet.set_visible(offer_reveal);
                self.retry
                    .set_visible(self.state.borrow().retryable().is_some());
                self.sheet.set_visible(true);
                self.status_bar.set_visible(false);
            }
            None => {
                self.sheet.set_visible(false);
                self.status_bar.set_visible(true);
            }
        }

        self.status_dot.set_visible(false);
        self.reveal.set_visible(false);
        if let State::Completed { output } = &state {
            *self.last_output.borrow_mut() = Some(output.clone());
        }
        if let State::Failed {
            retryable: Some(v), ..
        }
        | State::Cancelled { preserved: Some(v) } = &state
        {
            *self.last_output.borrow_mut() = Some(v.path.clone());
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
            .or_else(|| (self.hooks.capture_rect)().ok());
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

/// Count of live ffmpeg processes, for the cancellation smoke test. `-x` matches
/// the process name exactly; `-f` would match the harness's own command line.
fn ffmpeg_count() -> usize {
    std::process::Command::new("pgrep")
        .args(["-x", "ffmpeg"])
        .output()
        .map(|o| String::from_utf8_lossy(&o.stdout).lines().count())
        .unwrap_or(0)
}

/// `GLIMPSE_TRACE=1` prints every state transition and background result.
///
/// Exists because an earlier attempt to debug this by hand added a print that
/// silently failed to apply, and the missing output was read as evidence about
/// the program. A trace that is switched on in one place cannot go half-missing.
fn trace(msg: impl FnOnce() -> String) {
    use std::sync::OnceLock;
    static ON: OnceLock<bool> = OnceLock::new();
    if *ON.get_or_init(|| std::env::var("GLIMPSE_TRACE").is_ok()) {
        eprintln!("[trace] {}", msg());
    }
}

fn short_state(s: &State) -> &'static str {
    match s {
        State::Idle => "Idle",
        State::Arming { .. } => "Arming",
        State::Recording { .. } => "Recording",
        State::Stopping { .. } => "Stopping",
        State::Encoding { .. } => "Encoding",
        State::Completed { .. } => "Completed",
        State::Failed { .. } => "Failed",
        State::Cancelled { .. } => "Cancelled",
    }
}

impl Chrome {
    /// Fill the settings popover.
    ///
    /// Inline controls in labelled groups: no navigation, no modal, and it floats
    /// outside the window so it never covers the region being framed. The menu it
    /// replaces had settings, actions and Quit in one undifferentiated list.
    fn build_settings(self: &Rc<Self>, popover: &gtk::Popover) {
        let root = gtk::Box::new(gtk::Orientation::Vertical, 2);
        root.set_size_request(268, -1);

        let group = |text: &str| {
            let l = gtk::Label::new(Some(text));
            l.add_css_class("glimpse-group");
            l.set_xalign(0.0);
            l
        };
        let row = |label: &str, control: &gtk::Widget| {
            let b = gtk::Box::new(gtk::Orientation::Horizontal, 12);
            b.add_css_class("glimpse-row");
            let l = gtk::Label::new(Some(label));
            l.set_xalign(0.0);
            l.set_hexpand(true);
            b.append(&l);
            b.append(control);
            b
        };

        // ---- capture ----------------------------------------------------
        root.append(&group("CAPTURE"));

        let rates = [10u32, 15, 24, 30];
        let seg = gtk::Box::new(gtk::Orientation::Horizontal, 0);
        seg.add_css_class("glimpse-seg");
        seg.set_valign(gtk::Align::Center);
        let mut first: Option<gtk::ToggleButton> = None;
        for fps in rates {
            let b = gtk::ToggleButton::with_label(&fps.to_string());
            match &first {
                None => first = Some(b.clone()),
                Some(f) => b.set_group(Some(f)),
            }
            b.set_active(self.config.borrow().framerate == fps);
            let me = self.clone();
            b.connect_toggled(move |b| {
                if !b.is_active() {
                    return;
                }
                if me.state.borrow().is_active() {
                    me.status.set_text("finish the current recording first");
                    return;
                }
                me.config.borrow_mut().framerate = fps;
                me.persist();
            });
            seg.append(&b);
        }
        root.append(&row("Frame rate", seg.upcast_ref()));

        // Only where the backend actually honours it. avfoundation ignores
        // `-capture_cursor`, measured, so on macOS this switch would flip,
        // persist and change nothing — the same failure as a file that lies
        // about its contents
        // ([ADR 0012](../../../docs/adr/0012-a-setting-a-backend-cannot-honour.md)).
        //
        // Hidden rather than insensitive: a greyed-out switch invites the
        // question "what do I have to do to enable this?", and the answer is
        // nothing, ever, on this platform.
        if self.hooks.honours_pointer_capture {
            let pointer = gtk::Switch::new();
            pointer.set_valign(gtk::Align::Center);
            pointer.set_active(self.config.borrow().capture_mouse);
            {
                let me = self.clone();
                pointer.connect_state_set(move |_, on| {
                    me.config.borrow_mut().capture_mouse = on;
                    me.persist();
                    glib::Propagation::Proceed
                });
            }
            root.append(&row("Capture pointer", pointer.upcast_ref()));
        }

        let show_rect = gtk::Button::with_label("Show");
        show_rect.add_css_class("glimpse-sheet-button");
        show_rect.set_valign(gtk::Align::Center);
        {
            let me = self.clone();
            let pop = popover.clone();
            show_rect.connect_clicked(move |_| {
                pop.popdown();
                me.report_capture_rect();
            });
        }
        root.append(&row("Show capture rect", show_rect.upcast_ref()));

        // ---- output -----------------------------------------------------
        root.append(&group("OUTPUT"));

        let fmt_seg = gtk::Box::new(gtk::Orientation::Horizontal, 0);
        fmt_seg.add_css_class("glimpse-seg");
        fmt_seg.set_valign(gtk::Align::Center);
        let mut fmt_first: Option<gtk::ToggleButton> = None;
        for f in OutputFormat::all() {
            let b = gtk::ToggleButton::with_label(f.label());
            match &fmt_first {
                None => fmt_first = Some(b.clone()),
                Some(g) => b.set_group(Some(g)),
            }
            b.set_active(self.config.borrow().format == f);
            let me = self.clone();
            b.connect_toggled(move |b| {
                if !b.is_active() {
                    return;
                }
                if me.state.borrow().is_active() {
                    me.status.set_text("finish the current recording first");
                    return;
                }
                me.config.borrow_mut().format = f;
                me.persist();
                me.refresh();
            });
            fmt_seg.append(&b);
        }
        root.append(&row("Format", fmt_seg.upcast_ref()));

        let change = gtk::Button::with_label("Change…");
        change.add_css_class("glimpse-sheet-button");
        change.set_valign(gtk::Align::Center);
        {
            let me = self.clone();
            let pop = popover.clone();
            change.connect_clicked(move |_| {
                pop.popdown();
                me.choose_output_folder();
            });
        }
        root.append(&row("Save to", change.upcast_ref()));

        let folder = gtk::Label::new(Some(&display_path(&self.config.borrow().output_dir)));
        folder.add_css_class("glimpse-path");
        folder.set_xalign(0.0);
        folder.set_ellipsize(gtk::pango::EllipsizeMode::Start);
        folder.set_margin_start(4);
        folder.set_margin_bottom(4);
        root.append(&folder);

        // ---- theme ------------------------------------------------------
        root.append(&group("APPEARANCE"));
        let theme_seg = gtk::Box::new(gtk::Orientation::Horizontal, 0);
        theme_seg.add_css_class("glimpse-seg");
        theme_seg.set_valign(gtk::Align::Center);
        let mut t_first: Option<gtk::ToggleButton> = None;
        for t in Theme::all() {
            let b = gtk::ToggleButton::with_label(match t {
                Theme::System => "Auto",
                Theme::Light => "Light",
                Theme::Dark => "Dark",
            });
            match &t_first {
                None => t_first = Some(b.clone()),
                Some(g) => b.set_group(Some(g)),
            }
            b.set_active(self.config.borrow().theme == t);
            let me = self.clone();
            b.connect_toggled(move |b| {
                if !b.is_active() {
                    return;
                }
                me.config.borrow_mut().theme = t;
                me.apply_theme();
                me.persist();
            });
            theme_seg.append(&b);
        }
        root.append(&row("Theme", theme_seg.upcast_ref()));

        // Quit sits below a divider, apart from the settings.
        let sep = gtk::Separator::new(gtk::Orientation::Horizontal);
        sep.set_margin_top(6);
        sep.set_margin_bottom(4);
        root.append(&sep);

        let quit = gtk::Button::with_label("Quit Glimpse");
        quit.add_css_class("flat");
        {
            let win = self.window.clone();
            quit.connect_clicked(move |_| win.close());
        }
        root.append(&quit);

        popover.set_child(Some(&root));
    }

    fn open_containing_folder(&self) {
        let Some(path) = self.last_output.borrow().clone() else {
            return;
        };
        let dir = path.parent().unwrap_or(&path).to_path_buf();
        let uri = format!("file://{}", dir.display());
        if let Err(e) = gio::AppInfo::launch_default_for_uri(&uri, gio::AppLaunchContext::NONE) {
            eprintln!("glimpse: could not open {uri}: {e}");
        }
    }

    fn report_capture_rect(self: &Rc<Self>) {
        match (self.hooks.capture_rect)() {
            Ok(r) if r.is_capturable() => self
                .status
                .set_text(&format!("capture rect: {}x{} at {},{}", r.w, r.h, r.x, r.y)),
            Ok(r) => self
                .status
                .set_text(&format!("frame is off-screen ({}x{})", r.w, r.h)),
            Err(e) => self.status.set_text(&format!("geometry error: {e}")),
        }
    }
}
