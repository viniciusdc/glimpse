//! What can stop a recording while the window takes no clicks.
//!
//! The chrome asks for this through `PlatformHooks::stop_hint` and shows the
//! answer where the Stop button would go
//! ([ADR 0017](../../docs/adr/0017-click-through-is-a-mode-not-a-window.md)).
//! The frontend installs the stop paths *after* the chrome exists, so the two
//! need something to share — this.
//!
//! **The hint is set by whatever succeeded in installing itself, never written
//! as a literal.** A hardcoded shortcut would disagree with the binding the
//! moment anyone changed it, and a hotkey registration can fail outright when
//! another application already owns the combination. A hint naming a key that
//! nothing is listening for is the failure
//! [ADR 0012](../../docs/adr/0012-a-setting-a-backend-cannot-honour.md)
//! describes: a control that looks live and does nothing. Empty here means the
//! chrome shows its own button, which is wrong in a different and more honest
//! way — the button is at least visibly a button.

use std::cell::RefCell;
use std::rc::Rc;

/// The stop paths that are actually installed, shared between the frontend that
/// installs them and the hook that reports them.
#[derive(Clone, Default)]
pub struct StopPaths(Rc<RefCell<Vec<String>>>);

impl StopPaths {
    /// Record a stop path that is now live. Called only after the thing it
    /// names has been successfully installed.
    pub fn add(&self, hint: impl Into<String>) {
        self.0.borrow_mut().push(hint.into());
    }

    /// What to show in place of the action button, or `None` if nothing can
    /// stop a recording from outside the window.
    ///
    /// Joined with "or" rather than picking one, because a user who cannot
    /// reach the button benefits from knowing every way out, and there will
    /// never be more than two.
    pub fn hint(&self) -> Option<String> {
        let paths = self.0.borrow();
        if paths.is_empty() {
            None
        } else {
            Some(paths.join(" or "))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nothing_installed_means_no_hint() {
        // The chrome falls back to its own button on None. Claiming a stop path
        // that does not exist would be worse than showing an unreachable
        // button, because a button at least looks like a button.
        assert_eq!(StopPaths::default().hint(), None);
    }

    #[test]
    fn the_hint_names_every_installed_path() {
        let s = StopPaths::default();
        s.add("menu bar");
        assert_eq!(s.hint().as_deref(), Some("menu bar"));
        s.add("⌥⌘S");
        assert_eq!(s.hint().as_deref(), Some("menu bar or ⌥⌘S"));
    }

    #[test]
    fn clones_share_one_list() {
        // The frontend installs into one handle and the hook reads from
        // another. If a clone had its own list, the hint would stay empty and
        // the chrome would show an unreachable button forever.
        let a = StopPaths::default();
        let b = a.clone();
        a.add("menu bar");
        assert_eq!(b.hint().as_deref(), Some("menu bar"));
    }
}
