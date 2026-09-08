//! Do clicks actually land on the window behind? Ask by clicking.
//!
//! ```sh
//! cargo run -p glimpse-macos --example click_through_blackboard
//! ```
//!
//! `one_window_hit_test` asks the window server which window *would* be hit.
//! This one posts a real click and looks at whether something behind reacted.
//! The difference matters: the first tests an opinion, this tests the
//! consequence — whether a person can touch the application they are recording.
//! If those two ever disagreed, this is the half that would be telling the
//! truth.
//!
//! ## The board
//!
//! A large opaque window that draws a dot wherever it is clicked and counts the
//! clicks it receives. The frames under test are placed on top of it. A click
//! aimed at a transparent middle either raises the count or it does not, and
//! that is the whole measurement.
//!
//! ## Why this needs more controls than it looks like
//!
//! Synthesised clicks are silently discarded when the posting process is not
//! trusted for Accessibility. That failure looks *exactly* like a successful
//! finding: nothing gets through anywhere, every row reads "blocked", and the
//! conclusion is confidently wrong. AGENTS.md has a name for this — a check
//! whose premise expired does not fail, it lies. So:
//!
//! * **`AXIsProcessTrusted` is reported** before anything is posted.
//! * **A click on the bare board** must register. If it does not, the run is
//!   inconclusive and says so instead of producing a table.
//! * **A click on each frame's opaque part** must NOT register. Without this a
//!   board that stopped responding for any reason would report every frame as
//!   perfectly click-blocking.
//! * **A hand-built AppKit frame** is measured alongside the GTK one, so the
//!   result is a comparison rather than an absolute claim about this machine.
//!
//! ## It clicks on a real desktop
//!
//! There is no `Xvfb` on macOS, so this moves your actual pointer and posts
//! actual clicks. Every point is checked against the window server first and the
//! run aborts rather than clicking anywhere that is not one of this process's
//! own windows. The pointer is put back where it was afterwards.

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
    use glimpse_macos::window::{place, set_floating, window_nswindow};
    use gtk::glib;
    use gtk::prelude::*;
    use gtk4 as gtk;
    use objc2::rc::Retained;
    use objc2::runtime::AnyObject;
    use objc2::{class, msg_send};
    use objc2_app_kit::{NSScreen, NSWindow};
    use objc2_foundation::{MainThreadMarker, NSPoint, NSRect, NSSize};
    use std::cell::RefCell;
    use std::rc::Rc;

    const BORDER: f64 = 14.0;
    const HEADER: f64 = 56.0;

    /// The board, and the two frames sitting on it. AppKit coordinates.
    const BOARD: AppKitRect = AppKitRect {
        x: 150.0,
        y: 120.0,
        w: 1200.0,
        h: 760.0,
    };
    const GTK_FRAME: AppKitRect = AppKitRect {
        x: 300.0,
        y: 300.0,
        w: 500.0,
        h: 450.0,
    };
    const RAW_FRAME: AppKitRect = AppKitRect {
        x: 880.0,
        y: 300.0,
        w: 450.0,
        h: 450.0,
    };

    const CSS: &str = "
        window.board  { background: #14161a; }
        window.probe  { background: transparent; }
        .probe-header { background: #e9ecf0; min-height: 56px; }
        .probe-frame  { border: 14px solid #4080f5; }
        .probe-hole   { background: transparent; }
    ";

    // CoreGraphics event synthesis. CGPoint and NSPoint are the same two f64s.
    type CGEventRef = *mut std::ffi::c_void;
    const K_CG_EVENT_LEFT_MOUSE_DOWN: u32 = 1;
    const K_CG_EVENT_LEFT_MOUSE_UP: u32 = 2;
    const K_CG_MOUSE_BUTTON_LEFT: u32 = 0;
    const K_CG_HID_EVENT_TAP: u32 = 0;

    // One block per framework: stacking two `#[link]` attributes on a single
    // block repeats `kind = "framework"`, which clippy rejects.
    #[link(name = "CoreGraphics", kind = "framework")]
    extern "C" {
        fn CGEventCreateMouseEvent(
            source: *mut std::ffi::c_void,
            mouse_type: u32,
            position: NSPoint,
            button: u32,
        ) -> CGEventRef;
        fn CGEventPost(tap: u32, event: CGEventRef);
        fn CGWarpMouseCursorPosition(position: NSPoint) -> i32;
        fn CFRelease(cf: *mut std::ffi::c_void);
    }

    #[link(name = "ApplicationServices", kind = "framework")]
    extern "C" {
        fn AXIsProcessTrusted() -> bool;
    }

    /// One aimed click, and what must happen to it.
    struct Shot {
        name: &'static str,
        /// AppKit coordinates: y counts up from the bottom of the primary screen.
        at: NSPoint,
        /// True if the board behind must receive this click.
        want_through: bool,
    }

    struct Board {
        window: gtk::Window,
        hits: Rc<RefCell<Vec<(f64, f64)>>>,
    }

    pub fn run() -> anyhow::Result<()> {
        let app = gtk::Application::builder()
            .application_id("com.vinicius.glimpse.example.blackboard")
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

            let board = build_board(app);
            let probe = build_gtk_probe(app);
            held_c.borrow_mut().push(board.window.clone());
            held_c.borrow_mut().push(probe.clone());

            let app = app.clone();
            glib::timeout_add_local_once(std::time::Duration::from_millis(500), move || {
                if let Err(e) = measure(&board, &probe) {
                    eprintln!("blackboard: {e:#}");
                }
                if hold() {
                    println!("HOLD: the board and both frames are up. Ctrl-C to quit.");
                    println!("      Click their middles yourself — a dot appears where a");
                    println!("      click reached the board.");
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

    /// The board: opaque, counts clicks, and draws a dot at each one.
    ///
    /// The dot is not decoration. A count that goes up tells you a click landed
    /// somewhere on the board; a dot tells you it landed *where you aimed it*,
    /// which is the difference between a click that came through the hole and a
    /// click that came through somewhere else entirely.
    fn build_board(app: &gtk::Application) -> Board {
        let hits: Rc<RefCell<Vec<(f64, f64)>>> = Rc::new(RefCell::new(Vec::new()));

        let area = gtk::DrawingArea::new();
        area.set_hexpand(true);
        area.set_vexpand(true);
        {
            let hits = hits.clone();
            area.set_draw_func(move |_, cr, _, _| {
                for (x, y) in hits.borrow().iter() {
                    cr.set_source_rgb(1.0, 0.35, 0.25);
                    cr.arc(*x, *y, 9.0, 0.0, std::f64::consts::TAU);
                    let _ = cr.fill();
                }
            });
        }

        let click = gtk::GestureClick::new();
        {
            let hits = hits.clone();
            let area = area.clone();
            click.connect_pressed(move |_, _, x, y| {
                hits.borrow_mut().push((x, y));
                area.queue_draw();
            });
        }
        area.add_controller(click);

        let window = gtk::Window::builder()
            .application(app)
            .decorated(false)
            .resizable(false)
            .default_width(BOARD.w as i32)
            .default_height(BOARD.h as i32)
            .build();
        window.add_css_class("board");
        window.set_child(Some(&area));
        window.present();

        Board { window, hits }
    }

    fn build_gtk_probe(app: &gtk::Application) -> gtk::Window {
        let header = gtk::Box::new(gtk::Orientation::Vertical, 0);
        header.add_css_class("probe-header");
        // NOT vexpand. A vexpanding child makes the Box above it vexpand too, so
        // the header grew to over half the window and the "hole" click point,
        // computed from HEADER, landed inside the header instead. That would
        // have reported the hole as blocking clicks when the click never reached
        // it — the measurement agreeing with itself while being wrong, which is
        // only visible in a picture (ADR 0000).
        let title = gtk::Label::new(Some("GTK"));
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
            .default_width(GTK_FRAME.w as i32)
            .default_height(GTK_FRAME.h as i32)
            .build();
        window.add_css_class("probe");
        window.set_child(Some(&shell));
        window.present();
        window
    }

    fn measure(board: &Board, probe: &gtk::Window) -> anyhow::Result<()> {
        let mtm = MainThreadMarker::new().expect("GTK runs on the main thread");

        let board_ns = window_nswindow(&board.window)?;
        place(&board_ns, BOARD);
        let probe_ns = window_nswindow(probe)?;
        place(&probe_ns, GTK_FRAME);
        set_floating(&probe_ns);

        let raw = build_raw_frame();
        pump();

        // Only the primary screen's height is used for the flip, and only the
        // integer backing factor is ever trusted — never anything derived from a
        // monitor's reported physical size.
        let screen_h = NSScreen::screens(mtm)
            .iter()
            .next()
            .map(|s| s.frame().size.height)
            .ok_or_else(|| anyhow::anyhow!("no screens"))?;

        let ours = [board_ns.windowNumber(), probe_ns.windowNumber(), raw.number];
        println!(
            "blackboard: windows — board {} gtk {} appkit {}",
            ours[0], ours[1], ours[2]
        );
        // SAFETY: a pure query with no arguments and no side effects.
        let trusted = unsafe { AXIsProcessTrusted() };
        println!("blackboard: AXIsProcessTrusted = {trusted}");

        // Asking first, rather than discovering it from the control afterwards.
        // Posting five clicks that the system will silently discard moves a real
        // pointer across a real desktop to learn something one function call
        // already knew.
        if !trusted {
            println!(
                "\nblackboard: this process may not post synthesised clicks, so the\n\
                 automated half cannot run. Nothing was clicked and the pointer\n\
                 was not moved.\n\n\
                 Two ways forward:\n\n\
                 1. Click it yourself — no permission needed:\n\
                 \x20     GLIMPSE_PROBE_HOLD=1 cargo run -p glimpse-macos --example \
                 click_through_blackboard\n\
                 \x20  A red dot appears wherever a click reaches the board. Click the\n\
                 \x20  middle of the blue GTK frame, then the middle of the green AppKit\n\
                 \x20  one, and compare.\n\n\
                 2. Automate it — System Settings > Privacy & Security > Accessibility,\n\
                 \x20  add the terminal you are running this from, then re-run."
            );
            return Ok(());
        }

        let shots = shots();

        // PRE-FLIGHT. Nothing is clicked until every target is confirmed to be
        // over one of our own windows. This is a screen somebody uses; a stray
        // synthesised click on whatever happens to be underneath is not an
        // acceptable failure mode for a test.
        for s in &shots {
            let n = NSWindow::windowNumberAtPoint_belowWindowWithWindowNumber(s.at, 0, mtm);
            if !ours.contains(&n) {
                anyhow::bail!(
                    "aborting before clicking: '{}' at {:?} is over window {n}, \
                     which is not ours. Nothing was clicked.",
                    s.name,
                    s.at
                );
            }
        }
        println!("blackboard: pre-flight clear — every target is one of our windows\n");

        // Put the pointer back afterwards. It is the user's pointer.
        let restore = unsafe {
            let p: NSPoint = msg_send![class!(NSEvent), mouseLocation];
            p
        };

        let mut rows = Vec::new();
        for s in &shots {
            let before = board.hits.borrow().len();
            click_at(s.at, screen_h);
            pump();
            let after = board.hits.borrow().len();
            rows.push((s, after > before, after - before));
        }

        unsafe {
            CGWarpMouseCursorPosition(NSPoint::new(restore.x, screen_h - restore.y));
        }

        // The control decides whether any of the rest can be read at all.
        let bare_ok = rows
            .iter()
            .find(|(s, _, _)| s.name.starts_with("bare board"))
            .map(|(_, through, _)| *through)
            .unwrap_or(false);

        println!("--- did the click reach the board? ---");
        for (s, through, n) in &rows {
            let got = if *through { "REACHED" } else { "blocked" };
            let want = if s.want_through { "REACHED" } else { "blocked" };
            let mark = if *through == s.want_through {
                "  "
            } else {
                "<-"
            };
            println!(
                "  {mark} {:<28} {got:<8} (want {want:<8}) dots +{n}",
                s.name
            );
        }
        println!();

        if !bare_ok {
            println!("INCONCLUSIVE: a click on the bare board did not register.");
            println!("              Synthesised clicks are being dropped — most likely this");
            println!("              process is not trusted for Accessibility. Nothing below");
            println!("              the control line means anything; the run is not evidence");
            println!("              that frames block clicks.");
            return Ok(());
        }

        let gtk_through = rows
            .iter()
            .find(|(s, _, _)| s.name.starts_with("GTK hole"))
            .map(|(_, t, _)| *t)
            .unwrap_or(false);
        let raw_through = rows
            .iter()
            .find(|(s, _, _)| s.name.starts_with("AppKit hole"))
            .map(|(_, t, _)| *t)
            .unwrap_or(false);

        println!("RESULT (control passed: a bare-board click does register)");
        println!(
            "  GTK    transparent middle: {}",
            if gtk_through {
                "clicks pass through"
            } else {
                "clicks are SWALLOWED"
            }
        );
        println!(
            "  AppKit transparent middle: {}",
            if raw_through {
                "clicks pass through"
            } else {
                "clicks are SWALLOWED"
            }
        );
        if !gtk_through && raw_through {
            println!("\n  Same shape, same screen, same run. Only the toolkit differs.");
        }
        Ok(())
    }

    /// Five aimed clicks: one control on bare board, and for each frame one at
    /// its opaque part and one at its hole.
    fn shots() -> Vec<Shot> {
        let gtk_hole_y = GTK_FRAME.y + (GTK_FRAME.h - HEADER) / 2.0;
        let raw_hole_y = RAW_FRAME.y + (RAW_FRAME.h - HEADER) / 2.0;
        vec![
            Shot {
                name: "bare board (control)",
                at: NSPoint::new(BOARD.x + 60.0, BOARD.y + 60.0),
                want_through: true,
            },
            Shot {
                name: "GTK header (opaque)",
                at: NSPoint::new(
                    GTK_FRAME.x + GTK_FRAME.w / 2.0,
                    GTK_FRAME.y + GTK_FRAME.h - HEADER / 2.0,
                ),
                want_through: false,
            },
            Shot {
                name: "GTK hole (transparent)",
                at: NSPoint::new(GTK_FRAME.x + GTK_FRAME.w / 2.0, gtk_hole_y),
                want_through: true,
            },
            Shot {
                name: "AppKit header (opaque)",
                at: NSPoint::new(
                    RAW_FRAME.x + RAW_FRAME.w / 2.0,
                    RAW_FRAME.y + RAW_FRAME.h - HEADER / 2.0,
                ),
                want_through: false,
            },
            Shot {
                name: "AppKit hole (transparent)",
                at: NSPoint::new(RAW_FRAME.x + RAW_FRAME.w / 2.0, raw_hole_y),
                want_through: true,
            },
        ]
    }

    /// Post a real left click at an AppKit point.
    ///
    /// CGEvent works in display coordinates with a **top-left** origin, which is
    /// the opposite of everything else in this file, so the flip happens here and
    /// only here.
    fn click_at(at: NSPoint, screen_h: f64) {
        let p = NSPoint::new(at.x, screen_h - at.y);
        // SAFETY: both events are created here, posted once, and released.
        unsafe {
            let null = std::ptr::null_mut();
            let down = CGEventCreateMouseEvent(
                null,
                K_CG_EVENT_LEFT_MOUSE_DOWN,
                p,
                K_CG_MOUSE_BUTTON_LEFT,
            );
            let up =
                CGEventCreateMouseEvent(null, K_CG_EVENT_LEFT_MOUSE_UP, p, K_CG_MOUSE_BUTTON_LEFT);
            if down.is_null() || up.is_null() {
                println!("blackboard: could not create a mouse event");
                return;
            }
            CGEventPost(K_CG_HID_EVENT_TAP, down);
            std::thread::sleep(std::time::Duration::from_millis(30));
            CGEventPost(K_CG_HID_EVENT_TAP, up);
            CFRelease(down);
            CFRelease(up);
        }
    }

    struct RawFrame {
        number: isize,
    }

    /// The comparison: the same frame, hand-built in AppKit.
    fn build_raw_frame() -> RawFrame {
        let f = RAW_FRAME;
        // SAFETY: a straight transcription of the documented NSWindow/NSView
        // construction sequence. Everything is retained by the window or its
        // view tree, which outlives this call.
        let number = unsafe {
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
            let green: *mut AnyObject = msg_send![
                class!(NSColor),
                colorWithSRGBRed: 0.16f64, green: 0.70f64, blue: 0.36f64, alpha: 1.0f64,
            ];
            let grey: *mut AnyObject = msg_send![
                class!(NSColor),
                colorWithSRGBRed: 0.91f64, green: 0.925f64, blue: 0.94f64, alpha: 1.0f64,
            ];

            let inner = f.h - HEADER;
            let strips: [(NSRect, *mut AnyObject); 5] = [
                (
                    NSRect::new(NSPoint::new(0.0, inner), NSSize::new(f.w, HEADER)),
                    grey,
                ),
                (
                    NSRect::new(NSPoint::new(0.0, 0.0), NSSize::new(BORDER, inner)),
                    green,
                ),
                (
                    NSRect::new(NSPoint::new(f.w - BORDER, 0.0), NSSize::new(BORDER, inner)),
                    green,
                ),
                (
                    NSRect::new(NSPoint::new(0.0, 0.0), NSSize::new(f.w, BORDER)),
                    green,
                ),
                (
                    NSRect::new(NSPoint::new(0.0, inner - BORDER), NSSize::new(f.w, BORDER)),
                    green,
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
        RawFrame { number }
    }

    /// A `CGColorRef` is a CoreFoundation struct pointer, not an object.
    /// Declaring it as `*mut AnyObject` makes `msg_send!` panic at runtime,
    /// which is objc2 correctly refusing a wrong encoding.
    #[repr(transparent)]
    #[derive(Clone, Copy)]
    struct CGColorRef(*mut std::ffi::c_void);

    // SAFETY: a transparent pointer wrapper, with the encoding the runtime
    // reports for CGColorRef.
    unsafe impl objc2::Encode for CGColorRef {
        const ENCODING: objc2::Encoding =
            objc2::Encoding::Pointer(&objc2::Encoding::Struct("CGColor", &[]));
    }

    /// Let the main loop turn so posted events are actually delivered.
    fn pump() {
        let ctx = glib::MainContext::default();
        for _ in 0..300 {
            if !ctx.iteration(false) {
                break;
            }
        }
        std::thread::sleep(std::time::Duration::from_millis(150));
        while ctx.iteration(false) {}
    }

    /// Unused today, kept so the board window is not dropped by accident.
    #[allow(dead_code)]
    fn keep(_: &Retained<NSWindow>) {}
}
