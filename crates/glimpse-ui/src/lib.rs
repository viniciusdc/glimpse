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
pub mod hooks;
pub use hooks::PlatformHooks;

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

/* Read-only. It reports the active format rather than competing with the split
   button for the same click. */
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
