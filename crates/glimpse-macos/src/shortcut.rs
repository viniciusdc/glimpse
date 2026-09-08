//! Parsing a key combination, and saying it back the way macOS writes it.
//!
//! Deliberately toolkit-free and **not** gated on macOS, for the same reason
//! [`crate::geometry`] and [`crate::grab`] are not: this is string handling, it
//! is where a configurable shortcut silently becomes the wrong shortcut, and it
//! costs nothing to test on a machine with no screen. Linux CI checks it.
//!
//! The Carbon registration that consumes this lives in [`crate::hotkey`] and
//! does need AppKit.
//!
//! ## Why the display string is derived and not stored
//!
//! The chrome shows this where the Stop button would be
//! ([ADR 0017](../../docs/adr/0017-click-through-is-a-mode-not-a-window.md)), and
//! the binding is configurable. Two independent strings — one to register, one
//! to display — would drift the first time anyone edited `config.toml`, and the
//! label would then be confidently wrong about the only way to stop a recording.
//! So there is one input and the label is computed from it.

/// A parsed key combination, ready to register and ready to show.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Shortcut {
    /// Carbon virtual key code.
    pub key_code: u32,
    /// Carbon modifier mask.
    pub modifiers: u32,
    /// How macOS would write it: `⌃⌥S`.
    pub display: String,
}

// Carbon modifier masks, from `Events.h`. Not the AppKit ones — `NSEventModifierFlags`
// uses entirely different values, and mixing them registers a combination
// nobody can type.
const CMD: u32 = 0x0100;
const SHIFT: u32 = 0x0200;
const OPTION: u32 = 0x0800;
const CONTROL: u32 = 0x1000;

/// Carbon virtual key codes for the keys worth binding, from `Events.h`.
///
/// Letters only, plus a few obvious extras. Deliberately not exhaustive: a
/// shortcut that stops a recording wants to be typeable on any layout, and the
/// codes are positional, so exotic keys differ between layouts in ways this
/// table cannot express honestly.
fn key_code(name: &str) -> Option<u32> {
    Some(match name {
        "a" => 0,
        "s" => 1,
        "d" => 2,
        "f" => 3,
        "h" => 4,
        "g" => 5,
        "z" => 6,
        "x" => 7,
        "c" => 8,
        "v" => 9,
        "b" => 11,
        "q" => 12,
        "w" => 13,
        "e" => 14,
        "r" => 15,
        "y" => 16,
        "t" => 17,
        "o" => 31,
        "u" => 32,
        "i" => 34,
        "p" => 35,
        "l" => 37,
        "j" => 38,
        "k" => 40,
        "n" => 45,
        "m" => 46,
        "space" => 49,
        "escape" | "esc" => 53,
        "period" | "." => 47,
        "slash" | "/" => 44,
        _ => return None,
    })
}

/// Parse a spec like `ctrl+opt+s`.
///
/// Case-insensitive, whitespace tolerant. Returns `None` for anything it cannot
/// register — an unknown key, or a combination with no modifier at all.
///
/// **A bare key is refused on purpose.** This registers *globally*: it fires
/// whatever application has focus. Binding `s` alone would swallow the letter s
/// system-wide for as long as Glimpse is running, which is not a thing a user
/// can be allowed to do to themselves by editing one line of TOML.
pub fn parse(spec: &str) -> Option<Shortcut> {
    let mut modifiers = 0;
    let mut key: Option<String> = None;

    for part in spec.split('+') {
        let part = part.trim().to_ascii_lowercase();
        match part.as_str() {
            "cmd" | "command" | "super" => modifiers |= CMD,
            "shift" => modifiers |= SHIFT,
            "opt" | "option" | "alt" => modifiers |= OPTION,
            "ctrl" | "control" => modifiers |= CONTROL,
            "" => return None,
            _ => {
                // A second non-modifier means the spec names two keys, which is
                // not a thing. Refusing beats registering the last one and
                // pretending the line said that.
                if key.is_some() {
                    return None;
                }
                key = Some(part);
            }
        }
    }

    let key = key?;
    let key_code = key_code(&key)?;
    if modifiers == 0 {
        return None;
    }

    Some(Shortcut {
        key_code,
        modifiers,
        display: display(modifiers, &key),
    })
}

/// In macOS order — control, option, shift, command — which is how every menu in
/// the system writes it, and therefore the only order that will not read as a
/// typo.
fn display(modifiers: u32, key: &str) -> String {
    let mut s = String::new();
    if modifiers & CONTROL != 0 {
        s.push('⌃');
    }
    if modifiers & OPTION != 0 {
        s.push('⌥');
    }
    if modifiers & SHIFT != 0 {
        s.push('⇧');
    }
    if modifiers & CMD != 0 {
        s.push('⌘');
    }
    match key {
        "space" => s.push('␣'),
        "escape" | "esc" => s.push_str("Esc"),
        "period" | "." => s.push('.'),
        "slash" | "/" => s.push('/'),
        k => s.push_str(&k.to_ascii_uppercase()),
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_plain_combination_parses() {
        let s = parse("ctrl+opt+s").expect("ctrl+opt+s is registrable");
        assert_eq!(s.key_code, 1);
        assert_eq!(s.modifiers, CONTROL | OPTION);
        assert_eq!(s.display, "⌃⌥S");
    }

    #[test]
    fn spelling_and_spacing_do_not_matter() {
        // config.toml is edited by hand, so "Cmd + Shift + S" has to mean what
        // it looks like it means.
        assert_eq!(parse("Cmd + Shift + S"), parse("command+shift+s"));
        assert_eq!(parse("ALT+q"), parse("option+q"));
    }

    #[test]
    fn modifiers_are_written_in_the_system_order() {
        // Not the order they were typed in. Every macOS menu writes ⌃⌥⇧⌘, and a
        // label that writes them differently reads as a mistake.
        assert_eq!(parse("cmd+ctrl+shift+opt+k").unwrap().display, "⌃⌥⇧⌘K");
    }

    #[test]
    fn a_key_with_no_modifier_is_refused() {
        // This registers globally. A bare key would swallow that key in every
        // application for as long as Glimpse runs.
        assert_eq!(parse("s"), None);
        assert_eq!(parse("escape"), None);
    }

    #[test]
    fn nonsense_is_refused_rather_than_guessed() {
        assert_eq!(parse(""), None);
        assert_eq!(parse("cmd+"), None);
        assert_eq!(parse("cmd+nope"), None);
        // Two keys is not a combination anyone meant.
        assert_eq!(parse("cmd+s+d"), None);
    }

    #[test]
    fn modifiers_alone_are_refused() {
        // `cmd+shift` has nothing to press. Registering it would consume every
        // Cmd-Shift chord in every application.
        assert_eq!(parse("cmd+shift"), None);
    }

    #[test]
    fn the_carbon_masks_are_not_the_appkit_ones() {
        // AppKit's NSEventModifierFlagCommand is 1 << 20; Carbon's cmdKey is
        // 0x0100. Registering with the wrong family produces a hotkey nobody can
        // type and no error anywhere.
        assert_eq!(CMD, 0x0100);
        assert_eq!(SHIFT, 0x0200);
        assert_eq!(OPTION, 0x0800);
        assert_eq!(CONTROL, 0x1000);
    }
}
