# 0008 — Settings, and what the theme is allowed to change

- **Status:** ACCEPTED
- **Date:** 2026-08-26

## Context

Recording went to a hard-coded path and the interface was hard-coded dark. Both
needed to become choices, which means Glimpse needs somewhere to keep them.

## Decision

`~/.config/glimpse/config.toml`, holding theme, format, output directory,
framerate and cursor capture.

**Written on every change, not at exit.** A screen recorder is the kind of
application people close abruptly — by closing the window mid-session, or by
killing it. A preference that only survives a clean shutdown is a preference that
does not reliably survive.

**Loading never fails.** A missing file is the default configuration, and a
corrupt one is reported on stderr and then ignored. Losing a preference is
annoying; refusing to launch a screen recorder because a settings file has a
stray bracket in it is worse. A partial file keeps the defaults for everything it
does not mention, because hand-edited config files are normal.

### The user chooses the directory. Glimpse chooses the filename.

"Save recordings to…" picks a *folder*. The file is still named `glimpse.gif` or
`glimpse.mp4`, and is still disambiguated to `glimpse-1.gif` rather than
overwriting ([ADR 0005](0005-gif-encoding-and-the-atomic-commit.md)).

A full save-as dialog on every recording would be the obvious alternative and is
wrong for this tool: the entire point is to press one button and get a file. A
modal prompt between "stop" and "done" turns a two-click operation into a
four-click one, every single time. Choosing the folder once is the setting people
actually want.

The default is `XDG_VIDEOS_DIR` if the user has one, otherwise `$HOME`. Not
`/tmp` — a recording somebody just made is not scratch data.

### Themes may change mood, not meaning

Three options: **Follow system**, **Light**, **Dark**. `System` reads the
desktop's dark-mode preference and keeps following it while the app runs, so
switching the desktop theme switches Glimpse without a restart. An explicit choice
also sets GTK's own preference, so the menu and the folder chooser match the
window they were opened from rather than fighting it.

The stylesheet is written once and a palette substituted in, so the two themes
cannot drift apart structurally — only in colour.

**Three colours are identical in both themes**, deliberately: the accent blue, the
recording red, and the abort amber. They are not decoration — they are how the
window says *idle*, *recording* and *something went wrong*. A theme that
recoloured those would make the interface prettier and less truthful.

## Costs accepted

- Framerate and cursor capture are in the file but have no interface yet. They can
  be edited by hand; a settings surface for them is still outstanding.
- `System` depends on GTK reporting the desktop's preference. On a desktop that
  does not set it, `System` resolves to dark and the user picks explicitly. That
  is a graceful failure, not a correct one.
- Settings are written on every change, so a user who flips theme repeatedly
  writes the file repeatedly. It is a few hundred bytes; this is not a real cost,
  but it is a choice rather than an oversight.
