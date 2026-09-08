//! Can **one** GTK window be click-through in the middle on macOS?
//!
//! ```sh
//! cargo run -p glimpse-macos --example one_window_hit_test
//! ```
//!
//! [ADR 0011] measured that it cannot, and eliminated six candidate causes
//! without identifying the mechanism. That is the entire reason the macOS frame
//! is more than one window, so the elimination is load-bearing and worth
//! re-opening rather than inheriting.
//!
//! **The gap this probe exists to close.** ADR 0011's table tests
//! `NSWindow.isOpaque` and `wantsLayer`. It never tests `CALayer.opaque` on the
//! view GDK installs, which is a different property from either. The two
//! renderer rows were meant to rule the layer out — the argument being that
//! `GskGLRenderer` and `GskCairoRenderer` present different layers — but both
//! draw into **the same** `GdkMacosView` and therefore the same backing
//! `CALayer`. So "cairo failed too" does not eliminate the layer; it exercises
//! it twice. If anything in that layer tree declares itself opaque, the window
//! server composites and hit-tests the whole rectangle as solid no matter what
//! alpha GTK renders into it — which would explain every row in the table at
//! once.
//!
//! ## The discipline
//!
//! Every answer is read back from the window server with
//! `windowNumberAtPoint:belowWindowWithWindowNumber:` and **never** inferred
//! from pointer position. `XQueryPointer`'s child field taught this project the
//! difference on X11; the macOS equivalent is asking the window rather than the
//! server, and it will happily report the value you just set while the server
//! still holds the old one (AGENTS.md, [ADR 0015]).
//!
//! Both directions are checked on every pass. Points in the hole must come back
//! as **not ours**; points on the border and in the header must come back as
//! **ours**. Without the second half, a window that failed to render at all
//! passes clean — which is exactly how the spike first "succeeded".
//!
//! Treatments are cumulative and each one gets its own turn of the main loop
//! before anything is read, because a macOS window property is not in effect
//! when the call returns. The first row where the hole flips to NOT OURS is the
//! cause.
//!
//! [ADR 0011]: ../../../docs/adr/0011-why-the-macos-frame-is-more-than-one-window.md
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
    use glimpse_macos::geometry::AppKitRect;
    use glimpse_macos::window::{appkit_frame, place, window_nswindow};
    use gtk::glib;
    use gtk::prelude::*;
    use gtk4 as gtk;
    use objc2::rc::Retained;
    use objc2::runtime::AnyObject;
    use objc2::{class, msg_send};
    use objc2_app_kit::NSWindow;
    use objc2_foundation::{MainThreadMarker, NSPoint};
    use std::cell::RefCell;
    use std::rc::Rc;

    /// Deliberately thicker than the product's 3pt border. A hit test one point
    /// from an edge is a coin flip on a fractional-scale display, and a probe
    /// that answers ambiguously is worse than one that does not run.
    const BORDER: f64 = 12.0;
    /// Stands in for the chrome: the one part of a single window that must keep
    /// taking clicks.
    const HEADER: f64 = 60.0;

    /// Where the probe window goes. Fixed, so the hit points are reproducible
    /// and so it lands clear of the menu bar and the Dock.
    const FRAME: AppKitRect = AppKitRect {
        x: 300.0,
        y: 300.0,
        w: 600.0,
        h: 500.0,
    };

    /// The same shape the product wants, in one window: an opaque bar on top and
    /// a bordered box below it whose middle is nothing at all.
    const CSS: &str = "
        window.probe { background: transparent; }
        .probe-header { background: #e9ecf0; min-height: 60px; }
        .probe-frame  { border: 12px solid #4080f5; }
        .probe-hole   { background: transparent; }
    ";

    /// A `CGColorRef`, which is a CoreFoundation struct pointer and **not** an
    /// object.
    ///
    /// Declaring it as `*mut AnyObject` makes `msg_send!` panic at runtime —
    /// "expected return to have type code '^{CGColor=}', but found '@'" — which
    /// is objc2 doing its job. The encoding has to say what the thing actually
    /// is.
    #[repr(transparent)]
    #[derive(Clone, Copy)]
    struct CGColorRef(*mut std::ffi::c_void);

    // SAFETY: the type is a transparent wrapper around a pointer, and the
    // encoding below is the one the Objective-C runtime reports for CGColorRef.
    unsafe impl objc2::Encode for CGColorRef {
        const ENCODING: objc2::Encoding =
            objc2::Encoding::Pointer(&objc2::Encoding::Struct("CGColor", &[]));
    }

    /// A point to ask the window server about, and what the answer must be.
    struct Probe {
        name: &'static str,
        at: NSPoint,
        /// True where the window is painted and must keep taking clicks.
        want_ours: bool,
    }

    /// `GLIMPSE_PROBE_HOLD=1` leaves both windows on screen after measuring.
    ///
    /// The measurement is the evidence, but it is a table of window numbers and
    /// it convinces nobody who can see that the window looks perfectly correct —
    /// which it does. Rendering is not the thing that is broken. Hold mode puts
    /// the two windows side by side so the difference can be felt with a mouse,
    /// which is the only way this particular failure is perceptible at all.
    fn hold() -> bool {
        std::env::var("GLIMPSE_PROBE_HOLD").is_ok_and(|v| v != "0")
    }

    /// One thing to try, applied on top of everything before it.
    struct Treatment {
        name: &'static str,
        apply: fn(&NSWindow, &gtk::Window),
    }

    pub fn run() -> anyhow::Result<()> {
        let app = gtk::Application::builder()
            .application_id("com.vinicius.glimpse.example.onewindow")
            .build();

        let held: Rc<RefCell<Option<gtk::Window>>> = Rc::new(RefCell::new(None));
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

            let header = gtk::Box::new(gtk::Orientation::Vertical, 0);
            header.add_css_class("probe-header");
            // Named on screen, because the whole point of hold mode is putting
            // this window next to one that looks identical and behaves
            // differently. Two unlabelled blue rectangles would be a test you
            // cannot score.
            let title = gtk::Label::new(Some("GTK  —  click the middle: nothing gets through"));
            // NOT vexpand: it would propagate to the header Box and push the
            // header to over half the window, moving the hole out from under
            // every point computed from HEADER.
            title.set_valign(gtk::Align::Center);
            header.append(&title);

            let hole = gtk::Box::new(gtk::Orientation::Vertical, 0);
            hole.add_css_class("probe-hole");
            hole.set_hexpand(true);
            hole.set_vexpand(true);

            let frame = gtk::Box::new(gtk::Orientation::Vertical, 0);
            frame.add_css_class("probe-frame");
            frame.set_hexpand(true);
            frame.set_vexpand(true);
            frame.append(&hole);

            let shell = gtk::Box::new(gtk::Orientation::Vertical, 0);
            shell.append(&header);
            shell.append(&frame);

            let window = gtk::Window::builder()
                .application(app)
                .decorated(false)
                .resizable(false)
                .default_width(FRAME.w as i32)
                .default_height(FRAME.h as i32)
                .build();
            window.add_css_class("probe");
            window.set_child(Some(&shell));
            window.present();

            *held_c.borrow_mut() = Some(window.clone());

            // GTK maps asynchronously: there is no NSWindow until the loop has
            // turned, and reading one before then reports a fault in the design
            // that is really a fault in the timing.
            let app = app.clone();
            glib::timeout_add_local_once(std::time::Duration::from_millis(400), move || {
                if let Err(e) = measure(&window) {
                    eprintln!("probe: {e:#}");
                }
                if hold() {
                    // Both windows float, so neither drops behind whatever you
                    // are trying to click through onto. Without this the GTK
                    // window sinks on the first click and looks click-through
                    // for entirely the wrong reason.
                    if let Ok(ns) = window_nswindow(&window) {
                        glimpse_macos::window::set_floating(&ns);
                    }
                    println!("HOLD: both windows are up. Ctrl-C here to quit.\n");
                    println!("  LEFT  is GTK. Click inside its hole — the click stops at the");
                    println!("        window even though you can see straight through it.");
                    println!("  RIGHT is hand-built AppKit, drawn to look the same. Click inside");
                    println!("        its hole and whatever is behind it gets the click.\n");
                    println!("  Both look right. Only one behaves right, and no screenshot can");
                    println!("  tell you which — that is the entire finding.");
                } else {
                    app.quit();
                }
            });
        });

        app.run();
        Ok(())
    }

    fn measure(window: &gtk::Window) -> anyhow::Result<()> {
        let mtm = MainThreadMarker::new().expect("GTK runs on the main thread");
        let ns = window_nswindow(window)?;
        place(&ns, FRAME);

        // Placement is asynchronous too, so read the frame the server actually
        // gave us rather than the one we asked for, and derive every hit point
        // from that. A probe that computes its points from the requested
        // rectangle tests a window that may not be there.
        let got = appkit_frame(&ns);
        println!("probe: window at {},{} {}x{}", got.x, got.y, got.w, got.h);
        if (got.w - FRAME.w).abs() > 0.5 || (got.h - FRAME.h).abs() > 0.5 {
            println!("probe: NOTE the server resized us; points follow the actual frame");
        }

        dump_layers(&ns);

        let points = hit_points(got);
        let ours = ns.windowNumber();
        println!("\nprobe: our window number is {ours}");
        println!(
            "probe: {} points — {} must be OURS, {} must be NOT OURS\n",
            points.len(),
            points.iter().filter(|p| p.want_ours).count(),
            points.iter().filter(|p| !p.want_ours).count(),
        );

        let treatments = [
            Treatment {
                name: "baseline (GTK as it comes)",
                apply: |_, _| {},
            },
            Treatment {
                name: "NSWindow.setOpaque(false) + clearColor",
                apply: |ns, _| {
                    ns.setOpaque(false);
                    // SAFETY: +clearColor returns a shared, autoreleased NSColor
                    // that setBackgroundColor: retains. objc2-app-kit's NSColor
                    // feature is not enabled for this crate, so the class is
                    // looked up at runtime rather than adding a dependency for a
                    // probe.
                    unsafe {
                        let clear: *mut AnyObject = msg_send![class!(NSColor), clearColor];
                        let _: () = msg_send![ns, setBackgroundColor: clear];
                    }
                },
            },
            Treatment {
                name: "gdk_surface.set_opaque_region(empty)",
                apply: |_, w| {
                    if let Some(surface) = w.surface() {
                        surface.set_opaque_region(Some(&gtk::cairo::Region::create()));
                    }
                },
            },
            Treatment {
                name: "gdk_surface.set_INPUT_region(window minus hole)",
                apply: |_, w| punch_input_hole(w),
            },
            Treatment {
                name: "CALayer.opaque = NO, whole tree  <-- the untested one",
                apply: |ns, _| set_layer_tree_transparent(ns),
            },
            Treatment {
                name: "hasShadow = NO + invalidateShadow()",
                apply: |ns, _| {
                    ns.setHasShadow(false);
                    ns.invalidateShadow();
                },
            },
            Treatment {
                name: "whole VIEW tree opaque = NO (incl. any NSThemeFrame)",
                apply: |ns, _| set_view_tree_transparent(ns),
            },
            Treatment {
                name: "styleMask = Borderless",
                apply: |ns, _| {
                    // The raw AppKit window that passes this test is borderless.
                    // If GDK builds a titled window and merely hides the
                    // decoration, the frame view is still there and still has an
                    // opinion about the window's shape.
                    ns.setStyleMask(objc2_app_kit::NSWindowStyleMask::Borderless);
                    ns.setOpaque(false);
                    ns.invalidateShadow();
                },
            },
        ];

        // THE CONTROL. Every negative row below is worthless if the hole is not
        // actually being rendered transparent — a window painting an opaque
        // background would fail this test for a reason that has nothing to do
        // with hit testing, and the whole run would look like a finding.
        // `-l` captures this window alone and preserves its alpha.
        let shot = "/tmp/glimpse-probe-window.png";
        match std::process::Command::new("screencapture")
            .args(["-x", "-o", &format!("-l{ours}"), shot])
            .status()
        {
            Ok(s) if s.success() => println!("probe: window image with alpha at {shot}"),
            Ok(s) => println!("probe: screencapture exited {s}; the alpha control is MISSING"),
            Err(e) => println!("probe: could not run screencapture ({e}); alpha control MISSING"),
        }

        let mut first_pass: Option<&str> = None;
        for t in &treatments {
            (t.apply)(&ns, window);
            // The whole reason each treatment gets its own turn: the flag is set
            // here and the window server has not processed it yet. Reading back
            // in the same turn is how ignoresMouseEvents was very nearly
            // recorded as non-functional (ADR 0015).
            pump();

            let results: Vec<(bool, isize)> = points
                .iter()
                .map(|p| {
                    let n = NSWindow::windowNumberAtPoint_belowWindowWithWindowNumber(p.at, 0, mtm);
                    (n == ours, n)
                })
                .collect();

            let hole_clear = points
                .iter()
                .zip(&results)
                .all(|(p, (is_ours, _))| p.want_ours || !is_ours);
            let paint_holds = points
                .iter()
                .zip(&results)
                .all(|(p, (is_ours, _))| !p.want_ours || *is_ours);

            println!("--- {} ---", t.name);
            for (p, (is_ours, n)) in points.iter().zip(&results) {
                let got = if *is_ours { "OURS" } else { "not ours" };
                let want = if p.want_ours { "OURS" } else { "not ours" };
                let mark = if *is_ours == p.want_ours { "  " } else { "<-" };
                println!(
                    "  {mark} {:<16} {got:<9} (want {want:<9}) window {n}",
                    p.name
                );
            }
            println!(
                "  hole click-through: {}   painted parts still take clicks: {}\n",
                yn(hole_clear),
                yn(paint_holds),
            );

            if hole_clear && paint_holds && first_pass.is_none() {
                first_pass = Some(t.name);
            }

            // Did the treatment survive the redraw it just triggered? A flag
            // GDK resets on the next frame produces a clean-looking negative
            // result that eliminated nothing — the same shape of mistake as
            // reading a window property back instead of hit-testing it.
            if t.name.starts_with("CALayer") {
                println!("probe: layer tree AFTER the treatment and a redraw:");
                dump_layers(&ns);
                println!();
            }
            // Same reason, for the style mask: AppKit rebuilds the frame view on
            // a styleMask change and GTK may well put its own back. A treatment
            // that silently did not apply is a row that eliminated nothing.
            if t.name.starts_with("styleMask") {
                println!(
                    "probe: styleMask reads back as {:#x} (asked for 0x0)\n",
                    ns.styleMask().0
                );
            }
        }

        raw_appkit_control(mtm);

        match first_pass {
            Some(name) => {
                println!("RESULT: a single GTK window CAN be click-through in the middle.");
                println!("        First treatment that did it: {name}");
                println!("        ADR 0011's conclusion needs revisiting.");
            }
            None => {
                println!("RESULT: no treatment made the hole click-through.");
                println!("        ADR 0011 stands, and the layer is now eliminated too.");
            }
        }
        Ok(())
    }

    /// The same shape, built by hand in AppKit, in this process and this run.
    ///
    /// **This is the control, and without it the rows above prove nothing.** A
    /// negative result from the GTK window is only evidence about GTK if a
    /// hand-built window passes the identical test on the identical machine at
    /// the identical moment. If both fail, the finding is not "GTK cannot do
    /// this" but "macOS no longer does this", and ADR 0011's premise has expired
    /// rather than been confirmed — which is a far larger result and would be
    /// invisible without this half.
    fn raw_appkit_control(mtm: MainThreadMarker) {
        use objc2_foundation::{NSRect, NSSize};

        let f = AppKitRect {
            x: 950.0,
            y: 300.0,
            w: 500.0,
            h: 500.0,
        };
        println!("=== CONTROL: the same window, hand-built in AppKit ===");

        // SAFETY: a straight transcription of the documented NSWindow /
        // NSView construction sequence. Every object is retained by the window
        // or its view tree, which outlives this function because the window is
        // ordered front and never closed before the process exits.
        let ours = unsafe {
            let rect = NSRect::new(NSPoint::new(f.x, f.y), NSSize::new(f.w, f.h));
            let win: *mut AnyObject = msg_send![class!(NSWindow), alloc];
            // styleMask 0 = Borderless, backing 2 = NSBackingStoreBuffered.
            let win: *mut AnyObject = msg_send![
                win,
                initWithContentRect: rect,
                styleMask: 0usize,
                backing: 2usize,
                defer: false,
            ];
            let _: () = msg_send![win, setOpaque: false];
            let clear: *mut AnyObject = msg_send![class!(NSColor), clearColor];
            let _: () = msg_send![win, setBackgroundColor: clear];
            let _: () = msg_send![win, setHasShadow: false];
            let _: () = msg_send![win, setLevel: 3isize];

            let content: *mut AnyObject = msg_send![win, contentView];
            // GREEN, where the GTK window is blue. In hold mode these two sit
            // side by side and are meant to be indistinguishable in every
            // respect except behaviour; one deliberate difference is what stops
            // that from also making them indistinguishable from each other.
            let blue: *mut AnyObject = msg_send![
                class!(NSColor),
                colorWithSRGBRed: 0.16f64, green: 0.70f64, blue: 0.36f64, alpha: 1.0f64,
            ];
            let grey: *mut AnyObject = msg_send![
                class!(NSColor),
                colorWithSRGBRed: 0.91f64, green: 0.925f64, blue: 0.94f64, alpha: 1.0f64,
            ];

            // Content-view coordinates: bottom-left origin, so the header is at
            // the top and the four border strips ring the space below it. The
            // middle is left empty — no view, no layer, nothing.
            let inner_h = f.h - HEADER;
            let strips: [(NSRect, *mut AnyObject); 5] = [
                (
                    NSRect::new(NSPoint::new(0.0, inner_h), NSSize::new(f.w, HEADER)),
                    grey,
                ),
                (
                    NSRect::new(NSPoint::new(0.0, 0.0), NSSize::new(BORDER, inner_h)),
                    blue,
                ),
                (
                    NSRect::new(
                        NSPoint::new(f.w - BORDER, 0.0),
                        NSSize::new(BORDER, inner_h),
                    ),
                    blue,
                ),
                (
                    NSRect::new(NSPoint::new(0.0, 0.0), NSSize::new(f.w, BORDER)),
                    blue,
                ),
                (
                    NSRect::new(
                        NSPoint::new(0.0, inner_h - BORDER),
                        NSSize::new(f.w, BORDER),
                    ),
                    blue,
                ),
            ];
            for (r, color) in strips {
                let v: *mut AnyObject = msg_send![class!(NSView), alloc];
                let v: *mut AnyObject = msg_send![v, initWithFrame: r];
                let _: () = msg_send![v, setWantsLayer: true];
                let layer: *mut AnyObject = msg_send![v, layer];
                let cg: CGColorRef = msg_send![color, CGColor];
                let _: () = msg_send![layer, setBackgroundColor: cg];
                let _: () = msg_send![content, addSubview: v];
            }

            let nil: *mut AnyObject = std::ptr::null_mut();
            let _: () = msg_send![win, orderFront: nil];
            let n: isize = msg_send![win, windowNumber];
            n
        };

        pump();

        let points = hit_points(f);
        let mut hole_clear = true;
        let mut paint_holds = true;
        for p in &points {
            let n = NSWindow::windowNumberAtPoint_belowWindowWithWindowNumber(p.at, 0, mtm);
            let is_ours = n == ours;
            if p.want_ours {
                paint_holds &= is_ours;
            } else {
                hole_clear &= !is_ours;
            }
            let got = if is_ours { "OURS" } else { "not ours" };
            let want = if p.want_ours { "OURS" } else { "not ours" };
            let mark = if is_ours == p.want_ours { "  " } else { "<-" };
            println!(
                "  {mark} {:<16} {got:<9} (want {want:<9}) window {n}",
                p.name
            );
        }
        println!(
            "  hole click-through: {}   painted parts still take clicks: {}\n",
            yn(hole_clear),
            yn(paint_holds),
        );
        if hole_clear && paint_holds {
            println!("CONTROL PASSES: raw AppKit is click-through on this machine, right now.");
            println!("                So a negative GTK result above is about GTK.\n");
        } else {
            println!("CONTROL FAILS: raw AppKit is NOT click-through here either.");
            println!(
                "               ADR 0011's premise has expired; nothing above is about GTK.\n"
            );
        }
    }

    /// The X11 frontend's own mechanism, pointed at macOS.
    ///
    /// `gdk_surface_set_input_region` is *the* portable answer to this problem
    /// and it is what `glimpse_x11::ui::punch_input_hole` already ships:
    /// input region = the whole surface minus the hole. It is also the one API
    /// ADR 0011's table never names — the row there is `set_opaque_region`,
    /// which controls compositing, not hit testing. Different function,
    /// different question.
    ///
    /// `supports_input_shapes()` is reported rather than trusted, because GDK
    /// answering "no" and GDK answering "yes" then doing nothing are different
    /// findings and only one of them is a missing feature.
    fn punch_input_hole(window: &gtk::Window) {
        let Some(surface) = window.surface() else {
            println!("probe: no surface");
            return;
        };
        println!(
            "probe: display.supports_input_shapes() = {}",
            surface.display().supports_input_shapes()
        );

        let (w, h) = (surface.width(), surface.height());
        if w <= 0 || h <= 0 {
            println!("probe: surface not sized yet");
            return;
        }

        // Surface coordinates, and the same transform the X11 side applies. The
        // hole here is known from the layout rather than measured off a widget,
        // because the probe builds its own shape and does not need to discover
        // it.
        let (tx, ty) = window.surface_transform();
        let hx = (BORDER + tx).round() as i32;
        let hy = (HEADER + BORDER + ty).round() as i32;
        let hw = (FRAME.w - 2.0 * BORDER).round() as i32;
        let hh = (FRAME.h - HEADER - 2.0 * BORDER).round() as i32;

        let region =
            gtk::cairo::Region::create_rectangle(&gtk::cairo::RectangleInt::new(0, 0, w, h));
        if let Err(e) = region.subtract_rectangle(&gtk::cairo::RectangleInt::new(hx, hy, hw, hh)) {
            println!("probe: could not subtract the hole: {e}");
            return;
        }
        surface.set_input_region(Some(&region));
        println!("probe: input region set to {w}x{h} minus {hw}x{hh} at {hx},{hy}");
    }

    fn yn(b: bool) -> &'static str {
        if b {
            "YES"
        } else {
            "no"
        }
    }

    /// Nine points: five in the hole that must not be ours, three on the border
    /// and one in the header that must be.
    fn hit_points(f: AppKitRect) -> Vec<Probe> {
        // AppKit: y counts up from the bottom of the primary screen, so the
        // header is at the TOP of the window and the hole below it.
        let header_bottom = f.y + f.h - HEADER;
        let hole = AppKitRect {
            x: f.x + BORDER,
            y: f.y + BORDER,
            w: f.w - 2.0 * BORDER,
            h: header_bottom - f.y - 2.0 * BORDER,
        };
        let inset = 25.0;
        vec![
            Probe {
                name: "hole centre",
                at: NSPoint::new(hole.x + hole.w / 2.0, hole.y + hole.h / 2.0),
                want_ours: false,
            },
            Probe {
                name: "hole bl",
                at: NSPoint::new(hole.x + inset, hole.y + inset),
                want_ours: false,
            },
            Probe {
                name: "hole br",
                at: NSPoint::new(hole.x + hole.w - inset, hole.y + inset),
                want_ours: false,
            },
            Probe {
                name: "hole tl",
                at: NSPoint::new(hole.x + inset, hole.y + hole.h - inset),
                want_ours: false,
            },
            Probe {
                name: "hole tr",
                at: NSPoint::new(hole.x + hole.w - inset, hole.y + hole.h - inset),
                want_ours: false,
            },
            Probe {
                name: "border left",
                at: NSPoint::new(f.x + BORDER / 2.0, hole.y + hole.h / 2.0),
                want_ours: true,
            },
            Probe {
                name: "border right",
                at: NSPoint::new(f.x + f.w - BORDER / 2.0, hole.y + hole.h / 2.0),
                want_ours: true,
            },
            Probe {
                name: "border bottom",
                at: NSPoint::new(f.x + f.w / 2.0, f.y + BORDER / 2.0),
                want_ours: true,
            },
            Probe {
                name: "header",
                at: NSPoint::new(f.x + f.w / 2.0, header_bottom + HEADER / 2.0),
                want_ours: true,
            },
        ]
    }

    /// Let the main loop turn so the window server processes what was just set.
    fn pump() {
        let ctx = glib::MainContext::default();
        for _ in 0..200 {
            if !ctx.iteration(false) {
                break;
            }
        }
        std::thread::sleep(std::time::Duration::from_millis(120));
        while ctx.iteration(false) {}
    }

    /// Walk the content view's layer tree and tell every layer it is not opaque.
    ///
    /// The one thing ADR 0011 never tried. What each layer *is* gets printed
    /// first by [`dump_layers`], because "we set it to NO" is only interesting
    /// next to what it was.
    fn set_layer_tree_transparent(ns: &NSWindow) {
        let Some(view) = ns.contentView() else {
            println!("probe: no contentView");
            return;
        };
        // SAFETY: `layer` is a plain getter with no ownership transfer, and the
        // view is alive for the duration of this call.
        unsafe {
            let layer: *mut AnyObject = msg_send![&*view, layer];
            if layer.is_null() {
                println!("probe: contentView has no layer");
                return;
            }
            set_opaque_recursive(layer, false);
        }
    }

    /// Every view from the root down, and every layer each one owns.
    ///
    /// Broader than [`set_layer_tree_transparent`] on purpose: that one starts
    /// at the content view and so cannot touch a frame view above it.
    fn set_view_tree_transparent(ns: &NSWindow) {
        let Some(view) = ns.contentView() else { return };
        // SAFETY: the view tree is alive for the duration of the call, and both
        // setters are plain property writes.
        unsafe {
            let v: *mut AnyObject = Retained::as_ptr(&view) as *mut AnyObject;
            set_views_opaque_recursive(root_view(v), false);
        }
    }

    /// SAFETY: `view` must be a live `NSView`.
    unsafe fn set_views_opaque_recursive(view: *mut AnyObject, opaque: bool) {
        let layer: *mut AnyObject = msg_send![view, layer];
        if !layer.is_null() {
            set_opaque_recursive(layer, opaque);
        }
        let subs: *mut AnyObject = msg_send![view, subviews];
        if subs.is_null() {
            return;
        }
        let count: usize = msg_send![subs, count];
        for i in 0..count {
            let sub: *mut AnyObject = msg_send![subs, objectAtIndex: i];
            if !sub.is_null() {
                set_views_opaque_recursive(sub, opaque);
            }
        }
    }

    /// SAFETY: `layer` must be a live `CALayer`.
    unsafe fn set_opaque_recursive(layer: *mut AnyObject, opaque: bool) {
        let _: () = msg_send![layer, setOpaque: opaque];
        let subs: *mut AnyObject = msg_send![layer, sublayers];
        if subs.is_null() {
            return;
        }
        let count: usize = msg_send![subs, count];
        for i in 0..count {
            let sub: *mut AnyObject = msg_send![subs, objectAtIndex: i];
            if !sub.is_null() {
                set_opaque_recursive(sub, opaque);
            }
        }
    }

    /// What GDK actually installed, so the treatment below has something to be
    /// measured against.
    fn dump_layers(ns: &Retained<NSWindow>) {
        println!(
            "\nprobe: NSWindow.isOpaque = {} styleMask = {:#x}",
            ns.isOpaque(),
            ns.styleMask().0
        );
        let Some(view) = ns.contentView() else {
            println!("probe: no contentView");
            return;
        };
        // SAFETY: plain getters on a live view; nothing is transferred.
        unsafe {
            // From the ROOT of the view tree, not from the content view. GTK
            // asks for an undecorated window, but if GDK still builds a titled
            // NSWindow then AppKit puts an NSThemeFrame ABOVE the content view,
            // with a layer of its own that a walk starting at contentView never
            // sees. The first pass of this probe made exactly that mistake and
            // its negative result was worth nothing.
            let v: *mut AnyObject = Retained::as_ptr(&view) as *mut AnyObject;
            let root = root_view(v);
            print_view(root, 0);
        }
    }

    /// The top of the view hierarchy this window owns.
    ///
    /// SAFETY: `view` must be a live `NSView`.
    unsafe fn root_view(view: *mut AnyObject) -> *mut AnyObject {
        let mut cur = view;
        loop {
            let sup: *mut AnyObject = msg_send![cur, superview];
            if sup.is_null() {
                return cur;
            }
            cur = sup;
        }
    }

    /// SAFETY: `view` must be a live `NSView`.
    unsafe fn print_view(view: *mut AnyObject, depth: usize) {
        let wants: bool = msg_send![view, wantsLayer];
        let opaque: bool = msg_send![view, isOpaque];
        println!(
            "probe: {:indent$}view  {:<22} wantsLayer={wants:<5} isOpaque={opaque}",
            "",
            class_of(view),
            indent = depth * 2 + 7
        );
        let layer: *mut AnyObject = msg_send![view, layer];
        if !layer.is_null() {
            print_layer(layer, depth + 1);
        }
        let subs: *mut AnyObject = msg_send![view, subviews];
        if subs.is_null() {
            return;
        }
        let count: usize = msg_send![subs, count];
        for i in 0..count {
            let sub: *mut AnyObject = msg_send![subs, objectAtIndex: i];
            if !sub.is_null() {
                print_view(sub, depth + 1);
            }
        }
    }

    /// SAFETY: `layer` must be a live `CALayer`.
    unsafe fn print_layer(layer: *mut AnyObject, depth: usize) {
        let opaque: bool = msg_send![layer, isOpaque];
        // The geometry is the point. "Five tiles are opaque" says nothing until
        // you know whether any of them covers the hole: if the opaque ones are
        // exactly the header and the border, layer opacity is not the mechanism
        // and the search moves elsewhere.
        let r: objc2_foundation::NSRect = msg_send![layer, frame];
        println!(
            "probe: {:indent$}layer {:<14} opaque={opaque:<5} frame {:>6.1},{:>6.1} {:>5.1}x{:<5.1}",
            "",
            class_of(layer),
            r.origin.x,
            r.origin.y,
            r.size.width,
            r.size.height,
            indent = depth * 2 + 7
        );
        let subs: *mut AnyObject = msg_send![layer, sublayers];
        if subs.is_null() {
            return;
        }
        let count: usize = msg_send![subs, count];
        for i in 0..count {
            let sub: *mut AnyObject = msg_send![subs, objectAtIndex: i];
            if !sub.is_null() {
                print_layer(sub, depth + 1);
            }
        }
    }

    /// SAFETY: `obj` must be a live Objective-C object.
    unsafe fn class_of(obj: *mut AnyObject) -> String {
        match obj.as_ref() {
            Some(o) => o.class().name().to_string_lossy().into_owned(),
            None => "<null>".to_string(),
        }
    }
}
