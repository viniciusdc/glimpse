//! The proposed macOS design, measured before it is written down.
//!
//! ```sh
//! cargo run -p glimpse-macos --example single_window_frame
//! GLIMPSE_PROBE_HOLD=1 cargo run -p glimpse-macos --example single_window_frame
//! ```
//!
//! **The design.** One window with Linux's exact structure — header, rule,
//! bordered frame around a transparent hole, status bar — and
//! `ignoresMouseEvents` toggled as an application *mode* rather than a property
//! of a decorative window. Idle: the window takes clicks, so the chrome works.
//! Recording: the whole window takes none, so the user can touch the application
//! being recorded. Stopping moves to a menu bar item and a global hotkey,
//! because the Stop button is unreachable by construction while that mode is on.
//!
//! [ADR 0015] already measured that the flag makes a GTK window pass every
//! click, including on its painted parts. What that record has NOT measured is
//! the flag used this way, on a window that also contains the chrome, and two
//! consequences that only appear in this shape:
//!
//! 1. **Does the window shadow fall into an interior hole?** With three windows
//!    the chrome's shadow fell downward onto the capture region and was baked
//!    into the top 40 device pixels of every recording — correct capture rect,
//!    every geometry check green, visible only in the image (PR #40). A single
//!    non-opaque window has its shadow derived from its alpha shape, so an
//!    interior hole plausibly gets shadowed from its own edges. That is the same
//!    bug arriving through a new door, and it is the reason LICEcap calls
//!    `SWELL_SetWindowShadow(hwnd, false)`.
//!
//! 2. **What style mask does GDK give a resizable window?** Resize has nowhere
//!    to be grabbed today (issue #10). One window is the precondition for GTK's
//!    own resize edges, and a titled AppKit window carrying `Resizable` would
//!    get native ones for free. GDK builds a titled window even when GTK asks
//!    for `decorated(false)` — measured, style mask `0x8007` — so this is worth
//!    reading back rather than assuming.
//!
//! The shadow half is measured the way PR #40 measured it: against an identical
//! backdrop, with and without the app's shadow, comparing the same pixels. A
//! single capture cannot show a gradient; two can.
//!
//! [ADR 0015]: ../../../docs/adr/0015-the-frame-is-two-windows.md

#[cfg(not(target_os = "macos"))]
fn main() {
    eprintln!("This example needs AppKit. macOS only.");
    std::process::exit(1);
}

#[cfg(target_os = "macos")]
fn main() -> anyhow::Result<()> {
    imp::run()
}

#[cfg(target_os = "macos")]
mod imp {
    use anyhow::{Context, Result};
    use glimpse_core::capture::{GrabRequest, Recorder, Workspace};
    use glimpse_core::geometry::ScreenPixelRect;
    use glimpse_macos::geometry::AppKitRect;
    use glimpse_macos::window::{appkit_frame, place, set_floating, window_nswindow};
    use glimpse_macos::AvfCapture;
    use gtk::glib;
    use gtk::prelude::*;
    use gtk4 as gtk;
    use objc2_app_kit::NSWindow;
    use objc2_foundation::{MainThreadMarker, NSPoint};
    use std::cell::RefCell;
    use std::rc::Rc;
    use std::time::Duration;

    /// A uniform surface to record, so a shadow shows up as a delta rather than
    /// as "the desktop looks a bit dark there".
    const BACKDROP: AppKitRect = AppKitRect {
        x: 200.0,
        y: 150.0,
        w: 1100.0,
        h: 700.0,
    };
    /// The frame, sitting on the backdrop, well inside it on every side.
    const FRAME: AppKitRect = AppKitRect {
        x: 400.0,
        y: 300.0,
        w: 620.0,
        h: 460.0,
    };

    /// Linux's structure, in one window. The hole is a widget inside the shell,
    /// not a gap between windows, which is the whole point.
    const CSS: &str = "
        window.backdrop { background: #b9bec4; }
        window.single   { background: transparent; }
        /* NO background on the shell. The first version of this probe put one
           here and it filled the hole: every sampled row came back at the shell
           colour, both captures agreed exactly, and the shadow test returned a
           confident false negative. The shared stylesheet gets this right —
           `.glimpse-shell` carries only radius and shadow, and the colour lives
           on the header and the status bar, which is precisely what keeps the
           hole transparent. A single-window macOS design inherits that rule. */
        .s-shell  { border-radius: 10px; }
        .s-header { min-height: 44px; background: #e9ecf0; }
        .s-rule   { min-height: 2px; background: rgba(0,0,0,0.14); }
        .s-frame  { border: 3px solid #3689e6; }
        .s-hole   { background: transparent; }
        .s-status { min-height: 30px; background: #e9ecf0; }
    ";

    /// What the backdrop reads as in a grayscale capture, and what the shell
    /// reads as. The hole must show the first.
    const BACKDROP_GRAY: f64 = 189.0;
    const SHELL_GRAY: f64 = 235.0;

    struct Single {
        window: gtk::Window,
        hole: gtk::Box,
    }

    pub fn run() -> Result<()> {
        let app = gtk::Application::builder()
            .application_id("com.vinicius.glimpse.example.singlewindow")
            .build();

        let held: Rc<RefCell<Vec<gtk::Window>>> = Rc::new(RefCell::new(Vec::new()));
        let held_c = held.clone();

        app.connect_activate(move |app| {
            let provider = gtk::CssProvider::new();
            provider.load_from_data(CSS);
            if let Some(display) = gtk::gdk::Display::default() {
                gtk::style_context_add_provider_for_display(
                    &display,
                    &provider,
                    gtk::STYLE_PROVIDER_PRIORITY_APPLICATION,
                );
            }

            let backdrop = gtk::Window::builder()
                .application(app)
                .decorated(false)
                .resizable(false)
                .default_width(BACKDROP.w as i32)
                .default_height(BACKDROP.h as i32)
                .build();
            backdrop.add_css_class("backdrop");
            backdrop.present();

            let single = build_single(app);
            held_c.borrow_mut().push(backdrop.clone());
            held_c.borrow_mut().push(single.window.clone());

            let app = app.clone();
            glib::timeout_add_local_once(Duration::from_millis(500), move || {
                if let Err(e) = measure(&backdrop, &single) {
                    eprintln!("single: {e:#}");
                }
                if hold() {
                    println!("\nHOLD: the window is up with click-through OFF.");
                    println!("  Drag the bottom-right grip to test begin_resize by hand —");
                    println!("  synthesised drags need Accessibility, so this half is yours.");
                    println!("  Ctrl-C to quit.");
                } else {
                    app.quit();
                }
            });
        });

        app.run();
        Ok(())
    }

    fn hold() -> bool {
        std::env::var("GLIMPSE_PROBE_HOLD").is_ok_and(|v| v != "0")
    }

    /// Header, rule, frame around a hole, status bar — one shell, one window.
    ///
    /// **Resizable on purpose.** The style mask GDK produces for this is one of
    /// the two things being measured, and `resizable(false)` would answer a
    /// question nobody asked.
    fn build_single(app: &gtk::Application) -> Single {
        let header = gtk::Box::new(gtk::Orientation::Vertical, 0);
        header.add_css_class("s-header");
        let label = gtk::Label::new(Some("single window — Linux structure"));
        label.set_valign(gtk::Align::Center);
        header.append(&label);

        let rule = gtk::Box::new(gtk::Orientation::Vertical, 0);
        rule.add_css_class("s-rule");

        let hole = gtk::Box::new(gtk::Orientation::Vertical, 0);
        hole.add_css_class("s-hole");
        hole.set_hexpand(true);
        hole.set_vexpand(true);

        let frame = gtk::Box::new(gtk::Orientation::Vertical, 0);
        frame.add_css_class("s-frame");
        frame.set_hexpand(true);
        frame.set_vexpand(true);
        frame.append(&hole);

        let status = gtk::Box::new(gtk::Orientation::Vertical, 0);
        status.add_css_class("s-status");
        let slabel = gtk::Label::new(Some("Position the frame, then Record."));
        slabel.set_valign(gtk::Align::Center);
        status.append(&slabel);

        let shell = gtk::Box::new(gtk::Orientation::Vertical, 0);
        shell.add_css_class("s-shell");
        shell.append(&header);
        shell.append(&rule);
        shell.append(&frame);
        shell.append(&status);

        // One grip, bottom-right, doing exactly what the X11 frontend's eight
        // edges do. Enough to answer whether begin_resize is implemented at all.
        let grip = gtk::Box::new(gtk::Orientation::Vertical, 0);
        grip.set_size_request(18, 18);
        grip.set_halign(gtk::Align::End);
        grip.set_valign(gtk::Align::End);
        grip.set_cursor(gtk::gdk::Cursor::from_name("se-resize", None).as_ref());

        let overlay = gtk::Overlay::new();
        overlay.set_child(Some(&shell));
        overlay.add_overlay(&grip);

        let window = gtk::Window::builder()
            .application(app)
            .decorated(false)
            .resizable(true)
            .default_width(FRAME.w as i32)
            .default_height(FRAME.h as i32)
            .build();
        window.add_css_class("single");
        window.set_child(Some(&overlay));

        let drag = gtk::GestureDrag::new();
        {
            let window = window.clone();
            drag.connect_drag_begin(move |g, x, y| {
                let Some(surface) = window.surface() else {
                    return;
                };
                let Ok(toplevel) = surface.downcast::<gtk::gdk::Toplevel>() else {
                    println!("single: surface is not a toplevel");
                    return;
                };
                let (tx, ty) = window.surface_transform();
                let Some(w) = g.widget() else { return };
                let Some(b) = w.compute_bounds(&window) else {
                    return;
                };
                println!("single: begin_resize(BottomRight) called");
                toplevel.begin_resize(
                    gtk::gdk::SurfaceEdge::SouthEast,
                    g.device().as_ref(),
                    g.current_button() as i32,
                    (b.x() + x as f32) as f64 + tx,
                    (b.y() + y as f32) as f64 + ty,
                    g.current_event_time(),
                );
            });
        }
        grip.add_controller(drag);

        window.present();
        Single { window, hole }
    }

    fn measure(backdrop: &gtk::Window, single: &Single) -> Result<()> {
        let mtm = MainThreadMarker::new().expect("GTK runs on the main thread");

        let b_ns = window_nswindow(backdrop)?;
        let s_ns = window_nswindow(&single.window)?;
        place(&b_ns, BACKDROP);
        place(&s_ns, FRAME);
        set_floating(&s_ns);
        pump();

        // ---- Question 2, the cheap one ----------------------------------
        println!(
            "single: GTK asked for decorated(false) resizable(true); \
             GDK's style mask is {:#x}",
            s_ns.styleMask().0
        );
        println!(
            "single: (Titled 0x1, Closable 0x2, Miniaturizable 0x4, \
             Resizable 0x8, FullSizeContentView 0x8000)"
        );

        // ---- The mode switch, in this shape ------------------------------
        let hole_rect = hole_in_appkit(&single.window, &single.hole, &s_ns)
            .context("locating the hole on screen")?;
        println!(
            "single: hole at {},{} {}x{} (AppKit, from compute_bounds — not a cached layout)",
            hole_rect.x as i64, hole_rect.y as i64, hole_rect.w as i64, hole_rect.h as i64
        );

        let points = [
            (
                "hole centre",
                NSPoint::new(
                    hole_rect.x + hole_rect.w / 2.0,
                    hole_rect.y + hole_rect.h / 2.0,
                ),
            ),
            (
                "hole top-left",
                NSPoint::new(hole_rect.x + 30.0, hole_rect.y + hole_rect.h - 30.0),
            ),
            (
                "header",
                NSPoint::new(FRAME.x + FRAME.w / 2.0, FRAME.y + FRAME.h - 20.0),
            ),
            (
                "status bar",
                NSPoint::new(FRAME.x + FRAME.w / 2.0, FRAME.y + 12.0),
            ),
        ];
        let ours = s_ns.windowNumber();

        for (mode, flag) in [("IDLE  (flag off)", false), ("RECORDING (flag on)", true)] {
            s_ns.setIgnoresMouseEvents(flag);
            // The flag is not in effect when the call returns. Reading back in
            // the same turn is how it was once measured as doing nothing.
            pump();
            print!("single: {mode:<20} ");
            for (name, at) in &points {
                let n = NSWindow::windowNumberAtPoint_belowWindowWithWindowNumber(*at, 0, mtm);
                print!("{name}={} ", if n == ours { "ours" } else { "through" });
            }
            println!();
        }
        s_ns.setIgnoresMouseEvents(false);
        pump();

        // ---- Question 1, the one that matters ----------------------------
        let primary = objc2_app_kit::NSScreen::screens(mtm)
            .iter()
            .next()
            .context("no screens")?;
        let screen_h = primary.frame().size.height;
        let scale = primary.backingScaleFactor();
        let rect = ScreenPixelRect {
            x: (hole_rect.x * scale) as i32,
            y: ((screen_h - (hole_rect.y + hole_rect.h)) * scale) as i32,
            w: (hole_rect.w * scale) as i32,
            h: (hole_rect.h * scale) as i32,
        };

        // A strip of bare backdrop just OUTSIDE the window's left edge. This is
        // where a window shadow certainly does fall, so it is how the probe
        // proves a shadow exists at all. Without it, "+0.00 in the hole" is
        // equally consistent with GDK having disabled shadows before we started
        // — a true statement about contamination, but not the one the result
        // line would be claiming.
        let outside = ScreenPixelRect {
            x: ((FRAME.x - 40.0) * scale) as i32,
            y: ((screen_h - (hole_rect.y + hole_rect.h)) * scale) as i32,
            w: (40.0 * scale) as i32,
            h: (hole_rect.h * scale) as i32,
        };

        println!("\nsingle: shadow test — recording twice over the same backdrop");
        s_ns.setHasShadow(true);
        s_ns.invalidateShadow();
        pump();
        let with = capture_rows(rect).context("capture with shadow")?;
        let with_out = capture_cols(outside).context("capture outside with shadow")?;
        s_ns.setHasShadow(false);
        s_ns.invalidateShadow();
        pump();
        let without = capture_rows(rect).context("capture without shadow")?;
        let without_out = capture_cols(outside).context("capture outside without shadow")?;

        let outer: f64 = with_out
            .iter()
            .zip(&without_out)
            .map(|(a, b)| a - b)
            .fold(0.0f64, |acc, d| if d.abs() > acc.abs() { d } else { acc });
        if outer.abs() < 0.5 {
            println!(
                "\nCONTROL FAILED: turning the shadow on and off changed nothing OUTSIDE the\n\
                 \x20               window either (worst delta {outer:+.2}), so there is no\n\
                 \x20               shadow being drawn to test. The hole result below is\n\
                 \x20               still true about contamination, but says nothing about\n\
                 \x20               whether a single window could keep its elevation."
            );
        } else {
            println!(
                "\ncontrol: the shadow is real — outside the window's left edge it darkens by \
                 {outer:+.2}"
            );
        }

        // THE CONTROL. If the hole is not actually transparent, both captures
        // show the shell and agree exactly, and "no shadow" is what a filled
        // hole looks like. The first run of this probe did exactly that.
        let deepest = *without.last().unwrap_or(&f64::NAN);
        if (deepest - SHELL_GRAY).abs() < 8.0 {
            println!(
                "\nCONTROL FAILED: the deepest sampled row reads {deepest:.1}, which is the \
                 shell colour ({SHELL_GRAY:.0}),\n                not the backdrop \
                 ({BACKDROP_GRAY:.0}). The hole is being painted, so the shadow\n\
                 \x20               comparison below means nothing."
            );
            return Ok(());
        }
        if (deepest - BACKDROP_GRAY).abs() > 12.0 {
            println!(
                "\nCONTROL UNCLEAR: the deepest sampled row reads {deepest:.1}; the backdrop \
                 should be ~{BACKDROP_GRAY:.0}.\n                 Something else is under the \
                 hole. Treat the table below with suspicion."
            );
        } else {
            println!(
                "\ncontrol: the hole shows the backdrop ({deepest:.1} ≈ {BACKDROP_GRAY:.0}), \
                 so it is genuinely transparent"
            );
        }

        println!("\n  row   with-shadow  without   delta");
        let mut worst = 0.0f64;
        for i in 0..with.len().min(without.len()) {
            let d = with[i] - without[i];
            if d.abs() > worst.abs() {
                worst = d;
            }
            println!(
                "  {:>3}     {:>7.2}   {:>7.2}   {:>+6.2}",
                ROWS[i], with[i], without[i], d
            );
        }
        println!();
        translucency_test(&s_ns, rect, screen_h, scale)?;

        if worst.abs() < 0.5 && outer.abs() >= 0.5 {
            println!("RESULT: a shadow IS drawn ({outer:+.2} outside the window) and NONE of it");
            println!("        reaches the hole (worst delta {worst:+.2}).");
            println!("        A single window can keep its shadow, and the assembly gets the");
            println!("        elevation the three-window design had to give up.");
        } else if worst.abs() < 0.5 {
            println!("RESULT: nothing reaches the hole (worst delta {worst:+.2}), but no shadow");
            println!("        was drawn anywhere, so this is a clean bill on contamination");
            println!("        only. Elevation is unmeasured.");
        } else {
            println!("RESULT: the window's shadow IS inside the hole (worst delta {worst:+.2}).");
            println!("        Shadows stay off, exactly as in the three-window design and");
            println!("        for the same reason LICEcap turns them off. One window does");
            println!("        not recover the elevation.");
        }
        Ok(())
    }

    /// How translucent the window goes while recording, to read as "this is not
    /// interactive right now".
    const RECORDING_ALPHA: f64 = 0.8;

    /// Dimming the window must dim the chrome and **not** the hole.
    ///
    /// `alphaValue` composites the whole window, and the hole is already at
    /// alpha 0, so nothing should change there — 0 × 0.8 is still 0. That is an
    /// argument, not a measurement, and this project has a record of arguments
    /// about compositing being wrong in ways only a capture shows (PR #40). If
    /// the dim reached the hole it would tint every recording made on macOS.
    ///
    /// The control is the header band: it sits over the same backdrop, so if the
    /// window really did become translucent its captured value moves toward the
    /// backdrop. Without that, "the hole did not change" is equally consistent
    /// with `alphaValue` having done nothing at all.
    fn translucency_test(
        ns: &NSWindow,
        hole: ScreenPixelRect,
        screen_h: f64,
        scale: f64,
    ) -> Result<()> {
        let f = appkit_frame(ns);
        // A band inside the header, clear of the rounded corners and of the rule
        // below it.
        let band_bottom = f.y + f.h - 38.0;
        let band = ScreenPixelRect {
            x: ((f.x + 30.0) * scale) as i32,
            y: ((screen_h - (band_bottom + 26.0)) * scale) as i32,
            w: ((f.w - 60.0) * scale) as i32,
            h: (26.0 * scale) as i32,
        };

        println!("\nsingle: translucency test — does dimming the window dim the hole?");
        ns.setAlphaValue(1.0);
        pump();
        let hole_full = mean(&capture_rows(hole)?);
        let head_full = mean(&capture_rows(band)?);
        ns.setAlphaValue(RECORDING_ALPHA);
        pump();
        let hole_dim = mean(&capture_rows(hole)?);
        let head_dim = mean(&capture_rows(band)?);
        ns.setAlphaValue(1.0);
        pump();

        println!(
            "  header  alpha 1.0 = {head_full:.2}   alpha {RECORDING_ALPHA} = {head_dim:.2}   \
             delta {:+.2}",
            head_dim - head_full
        );
        println!(
            "  hole    alpha 1.0 = {hole_full:.2}   alpha {RECORDING_ALPHA} = {hole_dim:.2}   \
             delta {:+.2}",
            hole_dim - hole_full
        );

        if (head_dim - head_full).abs() < 1.0 {
            println!(
                "  CONTROL FAILED: the header did not change, so alphaValue did nothing.\n\
                 \x20                The hole result says nothing."
            );
        } else if (hole_dim - hole_full).abs() < 0.5 {
            println!(
                "  OK: the chrome dims and the hole does not. Translucency is safe to use\n\
                 \x20     as the recording cue."
            );
        } else {
            println!(
                "  PROBLEM: the dim reaches the hole, so it would tint every recording.\n\
                 \x20         Dim the widgets instead of the window."
            );
        }
        Ok(())
    }

    fn mean(v: &[f64]) -> f64 {
        let ok: Vec<f64> = v.iter().copied().filter(|x| x.is_finite()).collect();
        if ok.is_empty() {
            return f64::NAN;
        }
        ok.iter().sum::<f64>() / ok.len() as f64
    }

    /// Rows sampled inward from the hole's top edge, where a shadow cast by the
    /// header above it would be strongest.
    const ROWS: [i32; 7] = [0, 3, 6, 10, 16, 30, 120];

    /// Mean luminance of each sampled row of a real recording of `rect`.
    ///
    /// Through `AvfCapture` and `Recorder`, not `screencapture`: the question is
    /// what lands in a Glimpse recording, and only the shipping path can answer
    /// that.
    fn capture_rows(rect: ScreenPixelRect) -> Result<Vec<f64>> {
        let (w, h, px) = capture_gray(rect)?;
        Ok(ROWS
            .iter()
            .map(|&r| {
                let r = r as usize;
                if r >= h {
                    return f64::NAN;
                }
                px[r * w..(r + 1) * w]
                    .iter()
                    .map(|&v| v as f64)
                    .sum::<f64>()
                    / w as f64
            })
            .collect())
    }

    /// One real recording of `rect`, as an 8-bit grayscale buffer.
    ///
    /// Through `AvfCapture` and `Recorder`, not `screencapture`: the question is
    /// what lands in a Glimpse recording, and only the shipping path can answer
    /// that.
    fn capture_gray(rect: ScreenPixelRect) -> Result<(usize, usize, Vec<u8>)> {
        let backend = AvfCapture::discover().context("finding the screen capture device")?;
        let grab = backend.grab(&GrabRequest {
            rect,
            framerate: Some(10),
            capture_mouse: false,
        });
        let workspace = Workspace::create()?;
        let recorder = Recorder::start(&grab, workspace)?;
        std::thread::sleep(Duration::from_millis(900));
        let video = recorder.stop()?;

        let still = std::env::temp_dir().join("glimpse-single-probe.pgm");
        let out = std::process::Command::new("ffmpeg")
            .args(["-y", "-v", "error", "-i"])
            .arg(&video.path)
            .args(["-frames:v", "1", "-pix_fmt", "gray", "-f", "image2"])
            .arg(&still)
            .output()
            .context("extracting a frame")?;
        if !out.status.success() {
            anyhow::bail!("ffmpeg: {}", String::from_utf8_lossy(&out.stderr));
        }
        let parsed = parse_pgm(&std::fs::read(&still)?)?;
        let _ = std::fs::remove_file(&still);
        Ok(parsed)
    }

    /// Columns sampled leftward from the window's left edge, outside it.
    const COLS: [i32; 6] = [1, 3, 6, 10, 16, 30];

    /// Mean luminance of columns of a real recording, indexed from the RIGHT of
    /// the captured strip — which is the edge nearest the window.
    fn capture_cols(rect: ScreenPixelRect) -> Result<Vec<f64>> {
        let (w, h, px) = capture_gray(rect)?;
        Ok(COLS
            .iter()
            .map(|&d| {
                let x = w as i32 - 1 - d;
                if x < 0 {
                    return f64::NAN;
                }
                let x = x as usize;
                (0..h).map(|y| px[y * w + x] as f64).sum::<f64>() / h as f64
            })
            .collect())
    }

    /// Width, height and pixels of a binary PGM.
    ///
    /// Parsed here rather than shelled out to, so the probe has no dependency on
    /// a Python or an image library being present on whatever machine runs it.
    fn parse_pgm(data: &[u8]) -> Result<(usize, usize, Vec<u8>)> {
        let mut fields = Vec::new();
        let mut i = 0;
        while fields.len() < 4 && i < data.len() {
            while i < data.len() && data[i].is_ascii_whitespace() {
                i += 1;
            }
            if i < data.len() && data[i] == b'#' {
                while i < data.len() && data[i] != b'\n' {
                    i += 1;
                }
                continue;
            }
            let start = i;
            while i < data.len() && !data[i].is_ascii_whitespace() {
                i += 1;
            }
            fields.push(String::from_utf8_lossy(&data[start..i]).into_owned());
        }
        if fields.len() < 4 || fields[0] != "P5" {
            anyhow::bail!("not a binary PGM");
        }
        let w: usize = fields[1].parse()?;
        let h: usize = fields[2].parse()?;
        // One whitespace byte separates the header from the raster.
        let pixels = data[i + 1..].to_vec();
        if pixels.len() < w * h {
            anyhow::bail!("PGM raster is short: {} < {}", pixels.len(), w * h);
        }
        Ok((w, h, pixels))
    }

    /// Where the hole is on screen, computed from the widget rather than cached.
    ///
    /// This is the single window's whole advantage over the current design in
    /// one function: the capture rectangle is derived from `compute_bounds` on
    /// every call, the way X11 does it, so it cannot go stale when the frame
    /// moves. Today's `Frame::capture_rect` answers from a `Layout` computed
    /// once in `Frame::new` and never recomputed.
    fn hole_in_appkit(window: &gtk::Window, hole: &gtk::Box, ns: &NSWindow) -> Option<AppKitRect> {
        let b = hole.compute_bounds(window)?;
        let (tx, ty) = window.surface_transform();
        let f = appkit_frame(ns);
        // Widget coordinates count down from the window's top; AppKit counts up
        // from the screen's bottom. The flip happens once, here.
        Some(AppKitRect {
            x: f.x + b.x() as f64 + tx,
            y: f.y + f.h - (b.y() as f64 + ty) - b.height() as f64,
            w: b.width() as f64,
            h: b.height() as f64,
        })
    }

    fn pump() {
        let ctx = glib::MainContext::default();
        for _ in 0..300 {
            if !ctx.iteration(false) {
                break;
            }
        }
        std::thread::sleep(Duration::from_millis(180));
        while ctx.iteration(false) {}
    }
}
