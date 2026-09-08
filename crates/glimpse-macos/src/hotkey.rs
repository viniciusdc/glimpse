//! A global hotkey, through Carbon, because the alternative needs a permission.
//!
//! The second way to stop a recording while the window takes no clicks
//! ([ADR 0017](../../docs/adr/0017-click-through-is-a-mode-not-a-window.md)).
//! The menu bar item is the first and the one that always works; this one is
//! faster and is what the chrome shows when it registers.
//!
//! ## Why Carbon, which is deprecated
//!
//! The modern spelling is `NSEvent.addGlobalMonitorForEventsMatchingMask`, and
//! it requires Accessibility. `AXIsProcessTrusted` was measured `false` on a
//! developer's own machine, and a screen recorder that needed a *second*
//! permission prompt — on top of Screen Recording — merely to stop a recording
//! would be trading a working control for a prompt. `RegisterEventHotKey` needs
//! no permission at all, has needed none since 2003, and is still present.
//!
//! ## Registration is allowed to fail
//!
//! Another application may already own the combination. When that happens the
//! hotkey is simply not a stop path, and nothing claims it is: the caller only
//! records it in [`crate::stop::StopPaths`] on success, so the chrome never
//! shows a key that nothing is listening for. That is
//! [ADR 0012](../../docs/adr/0012-a-setting-a-backend-cannot-honour.md) applied
//! to a keystroke.

use crate::shortcut::Shortcut;
use std::ffi::c_void;

#[allow(non_camel_case_types)]
type OSStatus = i32;
type EventTargetRef = *mut c_void;
type EventHotKeyRef = *mut c_void;
type EventHandlerRef = *mut c_void;
type EventHandlerCallRef = *mut c_void;
type EventRef = *mut c_void;

#[repr(C)]
#[derive(Clone, Copy)]
struct EventTypeSpec {
    event_class: u32,
    event_kind: u32,
}

#[repr(C)]
#[derive(Clone, Copy)]
struct EventHotKeyID {
    signature: u32,
    id: u32,
}

type EventHandlerProc = extern "C" fn(EventHandlerCallRef, EventRef, *mut c_void) -> OSStatus;

// 'keyb', and the hot-key-pressed kind, from `CarbonEvents.h`.
const K_EVENT_CLASS_KEYBOARD: u32 = 0x6b657962;
const K_EVENT_HOT_KEY_PRESSED: u32 = 5;
/// 'glmp'. Any four bytes; it only has to be ours.
const SIGNATURE: u32 = 0x676c6d70;

#[link(name = "Carbon", kind = "framework")]
extern "C" {
    fn RegisterEventHotKey(
        key_code: u32,
        modifiers: u32,
        hot_key_id: EventHotKeyID,
        target: EventTargetRef,
        options: u32,
        out_ref: *mut EventHotKeyRef,
    ) -> OSStatus;
    fn UnregisterEventHotKey(hot_key: EventHotKeyRef) -> OSStatus;
    fn GetApplicationEventTarget() -> EventTargetRef;
    fn InstallEventHandler(
        target: EventTargetRef,
        handler: EventHandlerProc,
        num_types: u32,
        list: *const EventTypeSpec,
        user_data: *mut c_void,
        out_ref: *mut EventHandlerRef,
    ) -> OSStatus;
    fn RemoveEventHandler(handler: EventHandlerRef) -> OSStatus;
}

/// What the hotkey does when pressed. Boxed so its address is stable — Carbon
/// keeps the raw pointer for the life of the handler.
type Action = Box<dyn Fn()>;

extern "C" fn on_hotkey(
    _call: EventHandlerCallRef,
    _event: EventRef,
    user_data: *mut c_void,
) -> OSStatus {
    if !user_data.is_null() {
        // SAFETY: `user_data` is the pointer handed to `InstallEventHandler`,
        // which is the address of the `Action` owned by the `HotKey` below. The
        // handler is removed in `Drop` before that box is freed, so it cannot be
        // called with a dangling pointer.
        let action = unsafe { &*(user_data as *const Action) };
        action();
    }
    // noErr. Consuming the event rather than passing it on: the whole point is
    // that the frontmost application does not also see the keystroke.
    0
}

/// A registered global hotkey. Dropping it unregisters.
pub struct HotKey {
    hot_key: EventHotKeyRef,
    handler: EventHandlerRef,
    /// Kept alive and never moved: `on_hotkey` dereferences its address.
    _action: Box<Action>,
    /// What to show the user, e.g. `⌃⌥S`.
    pub display: String,
}

impl HotKey {
    /// Register `shortcut` system-wide.
    ///
    /// `None` if another application already owns the combination, or if Carbon
    /// refuses for any other reason. The caller must treat that as "there is no
    /// hotkey" rather than retrying with something else — a stop path the user
    /// did not choose is worse than one less stop path.
    pub fn register(shortcut: &Shortcut, action: impl Fn() + 'static) -> Option<Self> {
        let boxed: Box<Action> = Box::new(Box::new(action));
        let user_data = &*boxed as *const Action as *mut c_void;

        let spec = EventTypeSpec {
            event_class: K_EVENT_CLASS_KEYBOARD,
            event_kind: K_EVENT_HOT_KEY_PRESSED,
        };
        let mut handler: EventHandlerRef = std::ptr::null_mut();
        // SAFETY: the target is the process's own event target, the handler is a
        // plain extern "C" fn, and `user_data` points at a box this struct owns
        // and outlives the handler.
        let status = unsafe {
            InstallEventHandler(
                GetApplicationEventTarget(),
                on_hotkey,
                1,
                &spec,
                user_data,
                &mut handler,
            )
        };
        if status != 0 {
            eprintln!("glimpse: could not install the hotkey handler (OSStatus {status})");
            return None;
        }

        let mut hot_key: EventHotKeyRef = std::ptr::null_mut();
        // SAFETY: same target; `hot_key` is written on success only.
        let status = unsafe {
            RegisterEventHotKey(
                shortcut.key_code,
                shortcut.modifiers,
                EventHotKeyID {
                    signature: SIGNATURE,
                    id: 1,
                },
                GetApplicationEventTarget(),
                0,
                &mut hot_key,
            )
        };
        if status != 0 || hot_key.is_null() {
            // The expected failure, not an exceptional one: something else owns
            // the combination. Say which one, because the user chose it and is
            // the only person who can choose another.
            eprintln!(
                "glimpse: {} is already taken by another application, so it will not \
                 stop a recording (OSStatus {status})",
                shortcut.display
            );
            // SAFETY: `handler` was installed above and is removed exactly once.
            unsafe { RemoveEventHandler(handler) };
            return None;
        }

        Some(Self {
            hot_key,
            handler,
            _action: boxed,
            display: shortcut.display.clone(),
        })
    }
}

impl Drop for HotKey {
    fn drop(&mut self) {
        // Handler first, then the key. The other order leaves a window in which
        // an in-flight press reaches a handler whose action is about to be
        // freed.
        // SAFETY: both were created in `register` and are released once.
        unsafe {
            RemoveEventHandler(self.handler);
            UnregisterEventHotKey(self.hot_key);
        }
    }
}
