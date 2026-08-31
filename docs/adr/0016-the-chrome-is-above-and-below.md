# 0016 — The macOS chrome is above *and* below the frame

- **Status:** PROPOSED
- **Date:** 2026-08-31
- **Extends:** [ADR 0015](0015-the-frame-is-two-windows.md). Its two windows
  become three; nothing it measured changes.
- **Relates to:** [ADR 0006](0006-the-header-is-the-chrome.md),
  [ADR 0014](0014-the-chrome-is-shared-the-window-model-is-not.md)

## Context

The shared chrome now runs on macOS ([ADR 0014](0014-the-chrome-is-shared-the-window-model-is-not.md)),
and it is the same widgets X11 builds. The arrangement is not the same.

X11 stacks them in one window, in this order:

```
header
rule / progress
frame → hole          the capture region
status bar
sheet                 appears when a file is written
```

macOS put the whole chrome in the window above the frame, because
[ADR 0015](0015-the-frame-is-two-windows.md) said "a chrome window" and there was
no chrome yet to notice the word was singular:

```
chrome window     header, rule, status, sheet
frame window      border, click-through
```

Everything is present and the widths agree — measured at 646pt for both. But the
status line and the sheet sit **above** the recording area on macOS and **below**
it on X11, which is a different product, not a port of the same one.

The sheet is the worse half. It is where "saved to ~/Movies/glimpse.gif" and
"Show in Files" appear, and on X11 it opens directly under the region that was
just recorded. Above it, it reads as a banner about something that has not
happened yet.

## Decision

**Three windows: header above the frame, status below it.**

```
header window     interactive   header, rule / progress
frame window      NO clicks     border around the hole
status window     interactive   status bar, sheet
```

The frame window is unchanged, including `ignoresMouseEvents`. The chrome's
widgets are unchanged. What changes is that `assemble` on macOS distributes the
shell's children between two windows instead of putting all of them in one.

## Because

**It is the layout, and the layout is part of the design.**
[ADR 0006](0006-the-header-is-the-chrome.md) put the status line at the bottom
deliberately: the header carries what you set before recording, and the status
line carries what happened after. Reversing that on one platform makes the same
widgets mean different things.

**ADR 0015's argument was about clicks, not about counting.** Its case for two
windows was that four border strips needed corner arithmetic and an overlap test
for no benefit, since the border does not need to be clickable. None of that
applies here. A status window is not a strip: it has content, it is interactive,
and it is one rectangle.

**The alternative is ruled out by measurement, not by taste.** A single chrome
window spanning the whole height with a transparent middle would give the right
layout in one window — and [ADR 0011](0011-why-the-macos-frame-is-more-than-one-window.md)
measured that GTK does not inherit the window server's per-pixel alpha hit test,
on either renderer, with six candidate causes eliminated. That window would
swallow every click in the hole.

## Costs

**Three windows to keep in lockstep instead of two.** `addChildWindow:ordered:`
propagates move and not resize
([ADR 0011](0011-why-the-macos-frame-is-more-than-one-window.md)), so both the
frame and the status window attach to the header and a drag carries all three.
That is the same mechanism already in use, applied once more.

**The `setFrame:` trap gets a third victim.** ADR 0011 recorded that
`setFrame:display:` does not propagate to children even for the origin
component. With three windows there are two children to desynchronise instead of
one, and the rule is unchanged: never let a `setFrame:` on the parent carry an
origin change.

**The status window's height is not fixed.** The sheet is hidden until a file is
written, so the window grows when it appears. Its **top** edge is the anchor —
it must stay glued to the frame's bottom — so it has to grow downward, which is
the opposite of the header, whose bottom edge is the anchor and which grows
upward. Getting this backwards puts the sheet over the recording area.

**Resize is still not designed.** This changes nothing about #10: the frame
takes no clicks and neither the header nor the status window has an edge that
means "resize the capture region".

## What would falsify this

The status window stealing focus or ordering above the header in a way that
looks wrong when the application is not frontmost. Both are `addChildWindow`
children of the header, and ADR 0011 measured ordering only for the move case.

A status window that must be *hidden* when empty, rather than merely short. X11
hides the sheet inside a window that stays put; macOS would have an empty
window's shadow sitting under the frame. If a zero-height window renders a
shadow, this needs `orderOut:` rather than a height of zero — untested.

## Consequences

`lay_out` gains a third rectangle and
[ADR 0015](0015-the-frame-is-two-windows.md)'s two-window composition becomes a
three-window one. Its measurements stand: GTK still does not inherit alpha
hit-testing, `ignoresMouseEvents` still survives every disturbance tried, and the
frame window is still the only one that takes no clicks.

`Hole::Elsewhere` stops being enough on its own. The chrome has to hand back its
pieces so the platform can distribute them, rather than only being told whether
to include the hole.
