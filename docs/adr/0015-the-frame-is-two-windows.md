# 0015 — The frame is two windows, and one of them takes no clicks

- **Status:** PROPOSED
- **Date:** 2026-08-28
- **Supersedes:** the composition in
  [ADR 0011](0011-why-the-macos-frame-is-more-than-one-window.md). Its
  measurements stand; its conclusion does not.
- **Relates to:** [ADR 0006](0006-the-header-is-the-chrome.md),
  [ADR 0014](0014-the-chrome-is-shared-the-window-model-is-not.md)

## Context

[ADR 0011](0011-why-the-macos-frame-is-more-than-one-window.md) concluded that
the macOS frame must be five windows — a header and four border strips — with
nothing over the hole. The reasoning was: a GTK window takes clicks across its
whole frame on macOS, therefore anything covering the hole swallows clicks,
therefore do not cover it.

The measurement behind that is correct and still holds. GTK does not inherit the
window server's per-pixel alpha hit test, on either renderer, and six candidate
causes were eliminated.

The conclusion does not follow. It assumed every window is interactive.

## What was measured

`NSWindow.ignoresMouseEvents` is a whole-window property. ADR 0011 quotes issue
#1 dismissing it for exactly that reason, and this record made the same mistake:
treating "cannot be regional" as "cannot be useful". Whole-window is precisely
what a purely decorative layer wants.

A GTK4 window on `GdkMacosDisplay` with `setIgnoresMouseEvents(true)`:

```
ignoresMouseEvents reads back as: true

hole    → clear     (all 5 points)
border  → NOT OURS  (all 4 points)
```

Every point passes through, including the border. Against ADR 0011's assertions
both halves "fail" — which is the intended result here, because the window is
meant to take no clicks at all.

**Two controls were needed to believe it.**

The first attempt reported the flag doing nothing. Before accepting that, an
opaque raw `NSWindow` was tested with the same flag and the same hit test, to
establish that `windowNumberAtPoint:` can see the property at all — a check
built to detect per-pixel alpha need not respect an event-routing flag, and a
blind instrument would have produced a confident false negative:

```
opaque window, ignoresMouseEvents=false -> ours
opaque window, ignoresMouseEvents=true  -> not ours
```

It can see it. Which made the GTK result suspect for a second reason: the
control pumped the run loop after setting the flag and the GTK probe did not.
The window server processes the change asynchronously, so the probe was reading
the state from before it landed. With a pump, the flag works.

Without either control, "ignoresMouseEvents does nothing on GTK" would have
entered this record as measured fact.

## Decision

**The macOS frame is two windows.**

**A chrome window.** Interactive, ordinary. The header from
[ADR 0006](0006-the-header-is-the-chrome.md) — size readout, split button,
format chip, menu, status — plus dragging and resizing.

**A frame window.** Draws the border, transparent in the middle, and sets
`ignoresMouseEvents(true)` so it takes no clicks anywhere. Purely visual. Sized
and positioned from the chrome, and bound to it with `addChildWindow:ordered:`
so a move propagates.

## Because

**The border does not need to be clickable.** That was the unexamined premise.
ADR 0006 already makes the header the drag handle on X11, so moving the frame by
its edge was never the interaction; it was an incidental consequence of the edge
being a window.

**Two is materially simpler than five.** Four strips need corner arithmetic, a
test that they do not overlap, and placement that keeps four rectangles
consistent. One border window has none of that.

**It removes the resize hazard entirely.** ADR 0011 records that
`setFrame:display:` does not propagate to children even for the origin
component, so a resize written the obvious way silently desynchronises the
strips. With one visual window there are no strips to desynchronise, and resize
becomes resizing one window.

**It makes [ADR 0014](0014-the-chrome-is-shared-the-window-model-is-not.md)
cleaner.** That record proposes extracting the chrome into `glimpse-ui` because
`ui.rs` is 2248 lines of which 18 name X11. A design with an actual chrome
window is a more natural host for it than a header improvised above four strips.

## Costs

**The border cannot be grabbed.** No dragging or resizing from the frame's edge,
because the frame takes no clicks. Both must come from the chrome. On X11 the
header is already the drag handle, so dragging is unchanged; **resize is the real
loss**, since X11 resizes from the edges via eight overlay widgets and
`begin_resize`. macOS will need that affordance somewhere in the chrome, and it
is not yet designed.

**A decorative window over the recording area is a new thing to get wrong.** It
takes no clicks, but it is still *there*: it renders, it is composited, and it is
in front of the region being captured. Its transparent middle must be genuinely
transparent or it will appear in recordings. ADR 0011's frame could not have that
bug, because nothing was over the hole.

That is a real regression in safety and it is the reason to keep the expanded-crop
check from #24: grab the rect, grab it expanded, and require frame colour on all
four edges of one and none of the other.

**One more asynchronous property to sequence.** `ignoresMouseEvents` does not
take effect within the turn it is set, which is how it was nearly recorded as
non-functional. Anything reading back window state after setting it must pump
first.

## Consequences

[ADR 0011](0011-why-the-macos-frame-is-more-than-one-window.md)'s composition is
superseded. Its measurements are not: GTK still does not inherit alpha
hit-testing, `addChildWindow` still propagates move and not resize, and the
`setFrame:` trap is still real for anyone attaching child windows.

The five-window frame built in #24 is superseded before it merged. `layout.rs`
loses the four-strip arithmetic and its overlap tests, and gains a much smaller
job: one border rect and one chrome rect.

## What would falsify this

`ignoresMouseEvents` failing to survive something GTK does, because GDK's macOS
backend manages that property for its own input-region handling and could reset
it. That would be a frame which is click-through until the user resizes it and
then quietly is not — a bug that only appears in use.

**Resize was measured and it survives:** the flag still reads `true` afterwards
and the hit test still passes through. Restacking, theme changes and a
fullscreen transition were not tested, so the concern is narrowed rather than
closed. Anything that makes GDK recompute its input region is worth re-checking,
and the check is two commands.

A macOS header that genuinely diverges from the shared chrome would also
falsify the arrangement, but that is [ADR 0014](0014-the-chrome-is-shared-the-window-model-is-not.md)'s
question rather than this one's.
