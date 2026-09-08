//! Glimpse's chrome: the palette, the stylesheet, and the small formatters the
//! header shows.
//!
//! Shared between frontends per
//! [ADR 0014](../docs/adr/0014-the-chrome-is-shared-the-window-model-is-not.md).
//! None of this has an opinion about the platform: it is colours, CSS and string
//! formatting.
//!
//! The stylesheet is the reason this crate exists rather than the code being
//! copied. [ADR 0006](../docs/adr/0006-the-header-is-the-chrome.md) records that
//! the design document's tokens are "ported verbatim into the CSS rather than
//! approximated, so the app and the mock cannot drift apart on colour". Two
//! copies would reintroduce exactly that drift, and silently — nothing compares
//! two stylesheets.
//!
//! The window model deliberately stays in the frontends. A single shaped window
//! and two windows around a hole are genuinely different things; see
//! [ADR 0015](../docs/adr/0015-the-frame-is-two-windows.md).

/// Colours that differ between the light and dark palettes.
///
/// The stylesheet itself is written once and the palette substituted in, so the
/// two themes cannot drift apart structurally — only in colour. The accent and
/// the recording red are deliberately absent: they are the same in both, because
/// they carry meaning rather than mood.
pub mod chrome;
pub mod hooks;
pub use chrome::{Chrome, ChromeParts, Hole};
pub use hooks::PlatformHooks;

/// Whether the window manager draws the frame, rather than the header being the
/// chrome.
///
/// `GLIMPSE_DECORATIONS=server` hands the frame back to the window manager,
/// because an undecorated window that cannot be resized would break the product
/// — the frame's size *is* the capture region.
///
/// One function, because there are two callers: the chrome sets `decorated` from
/// it, and the platform decides from it whether to install its own resize edges.
/// Those were two separate reads with the comparison written out twice, agreeing
/// only because both spellings matched.
pub fn system_decorations() -> bool {
    std::env::var("GLIMPSE_DECORATIONS")
        .map(|v| v == "server")
        .unwrap_or(false)
}

pub struct Palette {
    header_bg: &'static str,
    /// The header tints while recording. One of three cues on a window whose
    /// middle is invisible, and it costs no chrome.
    header_rec: &'static str,
    /// Track the progress bar rides in — it replaces the header's bottom
    /// hairline rather than adding a row.
    rule: &'static str,
    sheet_bg: &'static str,
    sheet_fg: &'static str,
    outline: &'static str,
    meta: &'static str,
    emphasis: &'static str,
    chip_line: &'static str,
    hover: &'static str,
    status_bg: &'static str,
    link: &'static str,
    link_hover: &'static str,
    shadow: &'static str,
}

pub const DARK: Palette = Palette {
    header_bg: "#282c33",
    header_rec: "#302a2c",
    rule: "rgba(0,0,0,0.45)",
    sheet_bg: "rgba(16,18,22,0.95)",
    sheet_fg: "#c3c9d2",
    outline: "rgba(255,255,255,0.16)",
    meta: "#8b939e",
    emphasis: "#c3c9d2",
    chip_line: "rgba(255,255,255,0.14)",
    hover: "rgba(255,255,255,0.08)",
    status_bg: "rgba(16,18,22,0.92)",
    link: "#8ab4f8",
    link_hover: "#b8d0fb",
    shadow: "rgba(0,0,0,0.6)",
};

pub const LIGHT: Palette = Palette {
    header_bg: "#e9ecf0",
    header_rec: "#f6e9e9",
    rule: "rgba(0,0,0,0.14)",
    sheet_bg: "rgba(248,249,251,0.97)",
    sheet_fg: "#3b424b",
    outline: "rgba(0,0,0,0.18)",
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
pub fn stylesheet(p: &Palette) -> String {
    format!(
        r#"
window.glimpse {{ background: transparent; }}

/* The chrome as SEPARATE windows, which is macOS (ADR 0016). Harmless on X11,
   where nothing carries these classes.

   The shell needs a background there and does not here. On X11 the shell wraps
   the hole, so it must stay transparent; the hairline under the header composites
   over the window and nobody notices, because the window is one piece. Split into
   three windows, that same hairline is 14% black over the DESKTOP -- a 2px strip
   of whatever is behind the app, right where the join should be invisible.

   Measured on macOS before this rule, mid-edge:

       y 538..553   rgb(229,234,238)   header
       y 554..557   rgb(23,23,21)      the desktop, through the hairline
       y 558..563   rgb(0,128,235)     the frame's border

   The colours are the palette's, not literals in `glimpse-macos`: that crate
   cannot see the theme, and a hardcoded light grey is wrong the moment somebody
   switches to dark. */
window.glimpse-above .glimpse-shell {{ background: {header_bg}; }}
window.glimpse-below .glimpse-shell {{ background: {status_bg}; }}

.glimpse-shell {{
  border-radius: 10px;
  box-shadow: 0 30px 80px {shadow};
}}

.glimpse-header {{
  background: {header_bg};
  border-radius: 10px 10px 0 0;
  min-height: 44px;
  padding: 0 12px;
}}
.state-recording .glimpse-header,
.state-stopping  .glimpse-header {{ background: {header_rec}; }}

/* The hairline under the header. Progress replaces it rather than adding a
   row — the bar is the same 2px the border already spent. */
.glimpse-rule {{ background: {rule}; min-height: 2px; }}
.glimpse-progress {{ background: {rule}; min-height: 2px; }}
.glimpse-progress trough {{ background: transparent; min-height: 2px; border: 0; }}
.glimpse-progress progress {{ background: #3689e6; min-height: 2px; border: 0; }}
.glimpse-meta {{
  color: {meta};
  font-size: 12px;
  font-feature-settings: "tnum";
}}
.glimpse-elapsed {{ color: {emphasis}; }}
/* Promoted while recording: the two facts that matter are that it is recording
   and for how long. */
.state-recording .glimpse-elapsed,
.state-stopping  .glimpse-elapsed {{ font-size: 15px; font-weight: 500; color: #f0d4d4; }}
.glimpse-rec-label {{
  font-size: 12px;
  letter-spacing: 1.2px;
  color: #d78f8f;
}}
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
/* The action button while the window is passing clicks through: it is a label
   naming what stops the recording, not a control. It must not read as pressable,
   because it is not — the window it sits on takes no clicks at all (ADR 0017). */
/* Written with the state class in the selector, not just `.glimpse-action-hint`.
   Passthrough is only ever on while recording, and `.state-recording
   .glimpse-action` matches the same element at the SAME specificity, so source
   order decided it and the later rule won: the hint painted as a red button
   that could not be pressed, which is the exact thing it exists to avoid.
   Naming both classes puts it a class ahead rather than relying on where in
   this string the block happens to sit. */
.glimpse-action.glimpse-action-hint,
.state-recording .glimpse-action.glimpse-action-hint,
.state-stopping  .glimpse-action.glimpse-action-hint {{
  background: transparent;
  border: 1px solid {chip_line};
  border-radius: 14px;
  /* The palette's text colour, NOT white. White belongs to the blue and red
     button fills; on a transparent pill over the light recording header
     (#f6e9e9) it is illegible, and the whole window is dimmed to 0.8 on top of
     that. This has to stay readable in both themes at reduced opacity, because
     it is the only thing on screen saying how to stop the recording. */
  color: {emphasis};
  font-size: 12px;
  padding: 0 14px;
}}
.glimpse-action.glimpse-action-hint:hover,
.state-recording .glimpse-action.glimpse-action-hint:hover {{ background: transparent; }}
/* The arrow half of the split button.
   A GtkMenuButton is a CONTAINER, not a button: it wraps an internal GtkButton
   that carries the theme's own background, border, radius, shadow and metrics.
   Styling only the MenuButton leaves that inner node untouched, so the arrow
   rendered as a default-themed white box with its own drop shadow, taller than
   the blue half beside it and overflowing it top and bottom — on both
   platforms, visible in the Linux CI screenshot as much as on macOS.
   `.glimpse-menu > button` below already does this for the hamburger; the split
   button was simply missed. The Record half needs no such rule because it is a
   plain GtkButton with no inner node. */
.glimpse-action-arrow {{
  border-radius: 0 14px 14px 0;
  /* Zero, so the padding is the inner button's alone. Both would double it. */
  padding: 0;
  min-width: 28px;
  border-left: 1px solid rgba(0,0,0,0.22);
}}
.glimpse-action-arrow > button {{
  background: transparent;
  color: #ffffff;
  border: 0;
  border-radius: 0 14px 14px 0;
  box-shadow: none;
  min-height: 28px;
  min-width: 28px;
  padding: 0 6px;
  margin: 0;
}}
/* Transparent through every state, so the hover belongs to the split button as
   a whole rather than lighting up one half of it. */
.glimpse-action-arrow > button:hover,
.glimpse-action-arrow > button:active,
.glimpse-action-arrow > button:checked {{
  background: transparent;
  box-shadow: none;
}}
.glimpse-action:hover {{ background: #4a97ea; }}
.glimpse-action:disabled {{ opacity: 0.55; }}
.state-recording .glimpse-action,
.state-stopping  .glimpse-action {{ background: #c6262e; }}
.state-recording .glimpse-action:hover {{ background: #d2343c; }}

.glimpse-bullet {{ background: #ffffff; min-width: 9px; min-height: 9px; border-radius: 50%; }}
.state-recording .glimpse-bullet,
.state-stopping  .glimpse-bullet {{ min-width: 8px; min-height: 8px; border-radius: 0; }}

/* Read-only. It reports the active format rather than competing with the split
   button for the same click. */
/* The chip and the menu sit next to each other, so they share a box: the same
   height and the same corner radius. They differ in border on purpose — the
   chip is read-only and says so with an outline, the menu is a control and
   reveals itself on hover — but a 20px outline beside a 26px button read as a
   mistake rather than as a distinction.
   Height comes from `min-height` rather than vertical padding, because padding
   on a label and padding on a button's inner node do not produce the same box
   and that is what let them drift apart. */
.glimpse-chip {{
  color: {meta};
  font-size: 10.5px;
  font-weight: 500;
  letter-spacing: 0.6px;
  border: 1px solid {chip_line};
  border-radius: 5px;
  min-height: 22px;
  padding: 0 7px;
}}
.glimpse-menu {{
  color: {meta};
  background: none;
  border: 0;
  box-shadow: none;
  min-height: 22px;
  min-width: 22px;
  padding: 0;
}}
.glimpse-menu:hover {{ background: {hover}; border-radius: 5px; }}
/* Same container-versus-inner-node trap as `.glimpse-action-arrow` above: a
   GtkMenuButton wraps a GtkButton, and the rules on `.glimpse-menu` never
   reached it. This one HAD a `> button` rule, but it only set padding and
   min-height — so the theme's background, border, radius and drop shadow were
   still painting, which is why the hamburger rendered as a cream box larger
   than the chip beside it. Setting a couple of properties on the inner node is
   not the same as styling it. */
.glimpse-menu > button {{
  background: transparent;
  color: {meta};
  border: 0;
  border-radius: 5px;
  box-shadow: none;
  min-height: 22px;
  min-width: 22px;
  padding: 0 4px;
  margin: 0;
}}
.glimpse-menu > button:hover,
.glimpse-menu > button:active,
.glimpse-menu > button:checked {{
  background: transparent;
  box-shadow: none;
}}

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

/* The result sheet. Replaces the status strip when there is something to say
   that does not fit on one line — a path, a cause, real buttons. Costs nothing
   at idle because it is not there. */
.glimpse-sheet {{
  background: {sheet_bg};
  border-top: 1px solid rgba(54,137,230,0.5);
  border-radius: 0 0 10px 10px;
  min-height: 56px;
  padding: 0 12px 0 14px;
}}
.state-aborted .glimpse-sheet {{ border-top-color: rgba(229,165,10,0.55); }}
.glimpse-sheet-title {{ color: {sheet_fg}; font-size: 11.5px; font-weight: 500; }}
.state-aborted .glimpse-sheet-title {{ color: #e0b45c; }}
.glimpse-path {{
  font-family: monospace;
  font-size: 11px;
  color: {meta};
}}
.glimpse-sheet-button {{
  min-height: 24px;
  padding: 0 10px;
  border-radius: 5px;
  border: 1px solid {outline};
  background: none;
  box-shadow: none;
  font-size: 11px;
  color: {sheet_fg};
}}
.glimpse-sheet-button:hover {{ background: {hover}; }}

/* Settings popover: grouped inline controls, no navigation, no modal. */
.glimpse-group {{
  font-size: 10.5px;
  font-weight: 600;
  letter-spacing: 1px;
  color: {meta};
  padding: 4px 4px 2px;
}}
.glimpse-row {{ padding: 4px 4px; }}
.glimpse-row label {{ font-size: 12.5px; color: {sheet_fg}; }}
.glimpse-seg button {{
  min-height: 24px;
  padding: 0 10px;
  font-size: 11.5px;
  border-radius: 0;
  border: 1px solid {outline};
  background: none;
  box-shadow: none;
  color: {sheet_fg};
}}
.glimpse-seg button:first-child {{ border-radius: 5px 0 0 5px; }}
.glimpse-seg button:last-child {{ border-radius: 0 5px 5px 0; }}
.glimpse-seg button:checked {{ background: #3689e6; color: #ffffff; border-color: #3689e6; }}
.glimpse-grip-debug {{ background: rgba(255,0,255,0.6); }}
"#,
        shadow = p.shadow,
        header_bg = p.header_bg,
        meta = p.meta,
        emphasis = p.emphasis,
        chip_line = p.chip_line,
        hover = p.hover,
        status_bg = p.status_bg,
        link = p.link,
        link_hover = p.link_hover,
        outline = p.outline,
        sheet_fg = p.sheet_fg,
        sheet_bg = p.sheet_bg,
        rule = p.rule,
        header_rec = p.header_rec,
    )
}

/// A byte count as the header shows it.
pub fn human_size(bytes: u64) -> String {
    const KB: f64 = 1024.0;
    let b = bytes as f64;
    if b < KB {
        format!("{bytes} B")
    } else if b < KB * KB {
        format!("{:.0} KB", b / KB)
    } else {
        format!("{:.1} MB", b / (KB * KB))
    }
}

/// A path with `$HOME` collapsed to `~`, as the status line shows it.
pub fn display_path(p: &std::path::Path) -> String {
    let s = p.display().to_string();
    match std::env::var("HOME") {
        Ok(home) if !home.is_empty() && s.starts_with(&home) => s.replacen(&home, "~", 1),
        _ => s,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Both themes must define every token. A palette missing one would fail to
    /// compile, but a stylesheet that silently drops a substitution would not —
    /// it would render with a hole in it.
    #[test]
    fn both_themes_produce_a_stylesheet_with_no_unsubstituted_tokens() {
        for p in [&DARK, &LIGHT] {
            let css = stylesheet(p);
            assert!(!css.contains("{header_bg}"), "token left unsubstituted");
            assert!(css.contains("window.glimpse"), "stylesheet looks empty");
        }
    }

    /// The two themes must differ. A copy-paste that left both pointing at the
    /// same colours would look deliberate and be wrong.
    #[test]
    fn the_two_themes_are_not_the_same() {
        assert_ne!(stylesheet(&DARK), stylesheet(&LIGHT));
    }

    #[test]
    fn sizes_are_human_readable_at_each_scale() {
        assert_eq!(human_size(512), "512 B");
        assert_eq!(human_size(2048), "2 KB");
        assert_eq!(human_size(5 * 1024 * 1024), "5.0 MB");
    }

    /// Collapsing $HOME is cosmetic, but a path that is not under it must come
    /// back untouched rather than mangled.
    #[test]
    fn a_path_outside_home_is_left_alone() {
        let p = std::path::Path::new("/tmp/glimpse.gif");
        assert_eq!(display_path(p), "/tmp/glimpse.gif");
    }
}
