//! The menu bar item, which is how a recording gets stopped on macOS.
//!
//! While a recording runs the window takes no clicks anywhere, so the Stop
//! button cannot be pressed — that is the whole shape of
//! [ADR 0017](../../docs/adr/0017-click-through-is-a-mode-not-a-window.md), not
//! an oversight. Something outside the window has to be able to stop it.
//!
//! **This is the primary stop path, and it is primary because it needs no
//! permission.** A global hotkey is the other candidate and is the obvious one
//! to reach for, but `NSEvent.addGlobalMonitorForEventsMatchingMask` requires
//! Accessibility, which was measured absent on a developer's own machine. An
//! app that made stopping a recording depend on a second permission prompt —
//! on top of Screen Recording — would be trading a reachable control for an
//! unreachable one.
//!
//! ## Why there is an Objective-C class in here
//!
//! `NSMenuItem` dispatches through target/action: a selector sent to an object.
//! There is no closure-shaped API for it, and sending to a nil target walks the
//! responder chain into GDK's own delegate, which knows nothing about Glimpse.
//! So the target is a small class defined here whose one method calls a Rust
//! closure. It holds the closure and nothing else.

use objc2::rc::Retained;
use objc2::runtime::{AnyObject, NSObject};
use objc2::{define_class, sel, DefinedClass, MainThreadOnly};
use objc2_app_kit::{NSImage, NSMenu, NSMenuItem, NSStatusBar, NSStatusItem};
use objc2_foundation::{MainThreadMarker, NSString};

/// What the item does when it is chosen.
///
/// `Rc<dyn Fn()>` rather than a plain closure because the ivars have to be a
/// concrete type, and the callback reaches back into the chrome through a weak
/// reference — the chrome must not be kept alive by its own menu bar item.
type Action = std::rc::Rc<dyn Fn()>;

/// Width of the item in the bar, in points. Wide enough for the short title
/// below and narrow enough not to crowd a menu bar that already has ten things
/// in it.
const ITEM_WIDTH: f64 = 30.0;

struct Ivars {
    action: Action,
}

define_class!(
    // SAFETY:
    // - NSObject has no subclassing requirements.
    // - This type does not implement Drop.
    #[unsafe(super(NSObject))]
    // AppKit sends the action on the main thread, and the closure it calls
    // touches GTK widgets, which are main-thread-only.
    #[thread_kind = MainThreadOnly]
    #[name = "GlimpseMenuBarTarget"]
    #[ivars = Ivars]
    struct Target;

    impl Target {
        #[unsafe(method(glimpseStop:))]
        fn glimpse_stop(&self, _sender: Option<&AnyObject>) {
            (self.ivars().action)();
        }
    }
);

impl Target {
    fn new(mtm: MainThreadMarker, action: Action) -> Retained<Self> {
        let this = Self::alloc(mtm).set_ivars(Ivars { action });
        // SAFETY: the standard NSObject initialiser on a freshly allocated
        // instance of a class whose superclass is NSObject.
        unsafe { objc2::msg_send![super(this), init] }
    }
}

/// Glimpse's item in the system menu bar.
///
/// Held for the lifetime of the application. Dropping it removes the item, and
/// with it the only way to stop a recording.
pub struct MenuBarItem {
    item: Retained<NSStatusItem>,
    /// The action target, kept alive explicitly. `NSMenuItem` does **not**
    /// retain its target — it holds a weak reference, so a target dropped after
    /// installation leaves a menu item that either does nothing or messages a
    /// freed object.
    _target: Retained<Target>,
    mtm: MainThreadMarker,
}

impl MenuBarItem {
    /// Put the item in the menu bar. `on_stop` runs when Stop is chosen.
    pub fn install(mtm: MainThreadMarker, on_stop: impl Fn() + 'static) -> Self {
        let target = Target::new(mtm, std::rc::Rc::new(on_stop));

        let bar = NSStatusBar::systemStatusBar();
        // A fixed length rather than `NSVariableStatusItemLength`, so the item's
        // width does not depend on an AppKit layout pass sizing it to its
        // button. Changing this alone did not fix placement — the fix was
        // waiting longer (`await_placement`) — but a fixed width is one less
        // thing depending on a pass that arrives late under GTK.
        let item = bar.statusItemWithLength(ITEM_WIDTH);

        let menu = NSMenu::new(mtm);
        let stop = NSMenuItem::new(mtm);
        stop.setTitle(&NSString::from_str("Stop recording"));
        // SAFETY: `glimpseStop:` is defined on Target above and takes one
        // argument, which is the shape AppKit sends an action in.
        unsafe {
            stop.setTarget(Some(&target));
            stop.setAction(Some(sel!(glimpseStop:)));
        }
        menu.addItem(&stop);
        item.setMenu(Some(&menu));

        let this = Self {
            item,
            _target: target,
            mtm,
        };
        this.set_recording(false);
        this
    }

    /// The geometry [`Self::placed`] judges, as text.
    ///
    /// Worth reporting rather than only deciding on, because "not placed" has
    /// more than one cause and the numbers distinguish them: `28x0` at the
    /// origin is "asked too early", a real size at a negative y is "the system
    /// has nowhere to put it".
    pub fn report(&self) -> String {
        match self.item.button(self.mtm).and_then(|b| b.window()) {
            Some(w) => {
                let f = w.frame();
                format!(
                    "menu bar item: window {}x{} at {},{}; isVisible={}",
                    f.size.width,
                    f.size.height,
                    f.origin.x,
                    f.origin.y,
                    self.item.isVisible(),
                )
            }
            None => "menu bar item: no button or no window".to_string(),
        }
    }

    /// Did the system actually give the item a slot in the menu bar?
    ///
    /// **`isVisible` does not answer this.** It reports the application's own
    /// intent and stays `true` for an item the system never placed. The item is
    /// then created, retained, and invisible — which is indistinguishable from
    /// working unless the geometry is read back.
    ///
    /// So the question is asked of the geometry: an unplaced item's window sits
    /// off-screen, measured at `(0, -33)`, entirely below the display. A placed
    /// one lands in the bar, measured at `(1030, 949)`.
    ///
    /// **Must be called after several turns of the run loop, not one.** Under
    /// GTK placement took roughly a second; immediately after creation the
    /// window is `30x0` at the origin whether or not it will ever be placed, so
    /// a check run there reports failure for everything. See `await_placement`.
    pub fn placed(&self) -> bool {
        let Some(button) = self.item.button(self.mtm) else {
            return false;
        };
        let Some(window) = button.window() else {
            return false;
        };
        let f = window.frame();
        // The menu bar is at the top of the primary screen, so a placed item has
        // positive height and sits above the origin. Anything at or below y=0 is
        // off the display.
        f.size.height > 0.0 && f.origin.y > 0.0
    }

    /// Show whether a recording is running.
    ///
    /// The item is always present, not only while recording. A menu bar item
    /// that appeared at the moment the window stopped accepting clicks would be
    /// asking the user to discover a new control at the exact moment they need
    /// one, and the first thing anyone does when an application stops responding
    /// is look at the application, not at the menu bar.
    pub fn set_recording(&self, recording: bool) {
        let Some(button) = self.item.button(self.mtm) else {
            return;
        };

        // An SF Symbol, which is what the rest of the menu bar is drawn from, so
        // it inherits the right weight and tints itself for light and dark
        // without shipping an asset — the app bundle that would hold one is
        // decided but unbuilt (ADR 0013).
        let name = NSString::from_str(if recording {
            "record.circle.fill"
        } else {
            "record.circle"
        });
        let description = NSString::from_str(if recording {
            "Glimpse is recording"
        } else {
            "Glimpse"
        });
        let symbol =
            NSImage::imageWithSystemSymbolName_accessibilityDescription(&name, Some(&description));

        match symbol {
            Some(image) => {
                // Template, so the menu bar tints it rather than drawing our
                // colours over its own background.
                image.setTemplate(true);
                button.setImage(Some(&image));
                button.setTitle(&NSString::from_str(""));
            }
            // A named symbol can be missing on an older system. Falling back to
            // text keeps the item findable, which is the whole job — an item
            // that renders as nothing is an item that cannot stop a recording.
            None => button.setTitle(&NSString::from_str(if recording { "REC" } else { "G" })),
        }

        button.setToolTip(Some(&NSString::from_str(if recording {
            "Glimpse is recording — click to stop"
        } else {
            "Glimpse"
        })));
    }
}
