# 0017 — Click-through is a mode, not a window

- **Status:** PROPOSED
- **Date:** 2026-09-08
- **Supersedes:** the compositions in
  [ADR 0015](0015-the-frame-is-two-windows.md) and
  [ADR 0016](0016-the-chrome-is-above-and-below.md). Every measurement in both
  stands; the conclusion drawn from ADR 0015's does not.
- **Relates to:** [ADR 0006](0006-the-header-is-the-chrome.md),
  [ADR 0011](0011-why-the-macos-frame-is-more-than-one-window.md),
  [ADR 0014](0014-the-chrome-is-shared-the-window-model-is-not.md),
  [issue #10](https://github.com/viniciusdc/glimpse/issues/10)

## Context

macOS runs the same chrome Linux runs ([ADR 0014](0014-the-chrome-is-shared-the-window-model-is-not.md))
and arranges it in **three** windows: chrome above, click-through frame in the
middle, status and sheet below. That arrangement exists because of one belief,
held since [ADR 0011](0011-why-the-macos-frame-is-more-than-one-window.md): the
middle of a GTK window cannot be made click-through on macOS, therefore the
chrome and the hole cannot share a window.

The belief is correct. The conclusion does not follow, because it assumes the
hole must be click-through **at all times**. It only has to be click-through
while the user is interacting with the application being recorded.

## What was measured

All of it on 2026-09-08, on one machine, in the sessions recorded by
`crates/glimpse-macos/examples/one_window_hit_test.rs` and
`crates/glimpse-macos/examples/single_window_frame.rs`. Hit tests are read back
from the window server with `windowNumberAtPoint:belowWindowWithWindowNumber:`,
never inferred from pointer position, and both directions are checked on every
pass.

### A single GTK window still cannot punch a hole

ADR 0011 eliminated six candidate causes. Four more are eliminated here, and one
of them is the mechanism the GTK documentation prescribes:

| tried | result |
|---|---|
| `gdk_surface_set_input_region(window minus hole)` | **no change** — `gdk_display_supports_input_shapes()` is `false` on the Quartz backend, and the call succeeds silently |
| `CALayer.opaque = NO` across the whole layer tree | no change, and the flag was verified to survive the redraw it triggered |
| whole `NSView` tree opaque = NO, including `NSThemeFrame` | no change |
| `styleMask = Borderless` | no change; verified to read back as `0x0` |

Two controls make those readable. **The pixels are correct**: `screencapture -l`
on the GTK window, which preserves alpha, gives `rgba=(0,0,0,0)` at three points
inside the hole and `255` alpha on the header and border. **Raw AppKit still
works**: a hand-built borderless `NSWindow` — `clearColor`, `setOpaque:NO`,
opaque strips, no `ignoresMouseEvents` — built in the same process minutes later
passes all nine points. The failure is GTK's, not the machine's and not the OS
version's.

This also settles a dispute the records left open. Issue #1 claimed *"GTK4's
macOS backend hard-codes input shapes off … AppKit has no regional input shape
at all"*, and ADR 0011 rejected the whole sentence as the wrong diagnosis. Split
in two: the first half is now **measured true**, directly, by asking GDK. The
second half is false, and the control window above is the proof.

### The flag makes the whole window pass clicks, chrome included

ADR 0015 measured this on a window with nothing in it. Measured again on a window
carrying the full structure — header, rule, bordered frame around a transparent
hole, status bar:

```
IDLE      (flag off)   hole centre=ours     hole top-left=ours     header=ours     status=ours
RECORDING (flag on)    hole centre=through  hole top-left=through  header=through  status=through
```

Four points, both directions, one window. The mode switch does exactly what the
design needs and nothing more.

### The window's own shadow does not reach an interior hole

The reason to care is [PR #40](https://github.com/viniciusdc/glimpse/pull/40):
the chrome's shadow was baked into the top 40 device pixels of every macOS
recording, with a correct capture rect and every geometry check green, because a
shadow is a gradient and the expanded-crop test looks for a colour. A single
non-opaque window derives its shadow from its alpha shape, so an interior hole
was a plausible route for the same bug.

Recorded twice over an identical backdrop, through `AvfCapture` → `GrabCommand`
→ `Recorder` — the shipping path, not `screencapture` — sampling rows inward
from the hole's top edge:

```
  row   with-shadow  without   delta
    0      181.00    181.00    +0.00
    3      181.00    181.00    +0.00
   10      181.00    181.00    +0.00
   30      181.00    181.00    +0.00
  120      181.00    181.00    +0.00
```

**Two controls, and the first version of this probe failed one of them.** It put
a background on the shell, which filled the hole; every row read the shell colour,
both captures agreed exactly, and the table above would have been a confident
false negative. So:

- *The hole is genuinely transparent* — the deepest sampled row reads 181, the
  backdrop's value, not the shell's 235.
- *A shadow is genuinely being drawn* — toggling it darkens the backdrop
  immediately outside the window's left edge by **−70.08**. Without this,
  "+0.00 in the hole" is equally consistent with no shadow existing at all.

A single window therefore keeps its drop shadow, and the assembly gets back the
elevation three stacked windows had to give up.

### Dimming the window dims the chrome and not the hole

The recording cue is `alphaValue`, and the risk is that it composites the hole
too and tints every macOS recording. Arithmetic says it cannot — the hole is
already at alpha 0, and 0 × 0.8 is 0 — but this project has a record of
compositing arguments being wrong in ways only a capture shows.

```
  header  alpha 1.0 = 232.83   alpha 0.8 = 221.87   delta -10.97
  hole    alpha 1.0 = 181.00   alpha 0.8 = 181.00   delta  +0.00
```

The header band is the control: it sits over the same backdrop, so its value
moving toward the backdrop's 181 is what proves `alphaValue` took effect at all.
Without it, "the hole did not change" is equally consistent with the call doing
nothing.

### GDK gives a resizable window the Resizable style mask

`decorated(false) resizable(true)` produces style mask `0x800f` — Titled,
Closable, Miniaturizable, **Resizable**, FullSizeContentView. GDK builds a titled
AppKit window regardless of what GTK asks for; ADR 0011's composition never had
reason to look.

## Decision

**One window on macOS, with Linux's structure, and `ignoresMouseEvents` toggled
as an application mode.**

```
header
rule / progress
frame → hole          the capture region
status bar
sheet
```

The same widget tree X11 builds, through the shared chrome's existing
`Hole::InChrome` branch.

- **Idle:** the window takes clicks. The chrome works. The hole does **not** pass
  clicks through, and that is accepted rather than worked around — see Costs.
- **Recording:** `ignoresMouseEvents = true`. The whole window takes none, so the
  user can touch the application being recorded.
- **Passthrough is also user-controllable**, not only recording-controlled, so
  the frame can be positioned over a live application without blocking it.

**The mode is visible, and the chrome tells the truth about it.** Two changes,
both macOS-only, both while passthrough is on:

- **The window dims to `alphaValue = 0.8`.** A window that silently stopped
  accepting clicks would read as a frozen application. The dim is what makes
  "not interactive" a thing you can see. Measured above: it does not reach the
  hole.
- **The Stop button is replaced by the shortcut that actually stops the
  recording.** Not disabled, not greyed — replaced, in the same place, by the key
  combination.

**Stopping** is an `NSStatusItem` in the menu bar and a configurable global
hotkey. The menu bar item is primary and needs no permission of any kind.

## Because

**The timing hazard ADR 0015 records does not apply to a mode.** That record
measured `ignoresMouseEvents` as asynchronous — it does not take effect within
the turn it is set, and was very nearly written off as non-functional because of
it. A design that toggles the flag per pointer-crossing would race every click
against the window server. A design that toggles it on a state transition toggles
it once, at a moment when the user has just pressed Record and the session is
arming anyway. There is no click in flight to lose.

**Three windows cost more than they buy.** They exist to keep the chrome off a
hole that the flag can clear anyway. What they add: a manual re-anchor whenever
GTK resizes the status window, a seam at every join, no shadow anywhere, and
state CSS that silently stops applying because `.state-*` lives on the chrome
window's shell and the other two windows never see it.

**It removes a whole bug class rather than fixing an instance.** `Frame::capture_rect`
answers from a `Layout` computed once in `Frame::new` and never recomputed, so
after a drag macOS reports — and records — the rectangle the frame *started* at,
and `geometry_drifted` cannot see it because both sides read the same stale
value. In one window the rect comes from `compute_bounds` on the hole widget on
every call, exactly as X11 does it, and the staleness has nowhere to live.

**It is the precondition for resize.** Issue #10 is open because the frame takes
no clicks and so cannot be grabbed by its edge. One window is what GTK's own
resize edges need, and the window already carries the Resizable style mask.

**Swapping the Stop button for the shortcut is [ADR 0012](0012-a-setting-a-backend-cannot-honour.md)'s
rule, not a nicety.** That record decides the Capture-pointer switch is not shown
on macOS because avfoundation ignores it, and calls a control that flips,
persists and changes nothing "the same failure as a file that lies about its
contents". A Stop button rendered on top of a window that takes no clicks is that
same control: it is visible, it looks live, and pressing it does nothing. The
platform cannot honour it, so it is not offered. What goes in its place is the
thing that does work.

**And the hint has to be generated from the registered hotkey, never from a
literal.** The shortcut is configurable, so a hardcoded "⌘⇧S" in the chrome would
be a label that disagrees with the binding the moment anyone changes it — the
same lie one layer down. `RegisterEventHotKey` can also *fail*, because another
application already owns the combination. When it does, the chrome must name the
menu bar item instead. A hint is only worth showing if it is rendered from state
that is known to be live.

## Costs

**While idle, the hole is not click-through — accepted.** On X11 the input shape
is punched the whole time, so a user can position the frame over an application
and keep clicking into that application to set up the shot. On macOS they cannot,
unless they turn passthrough on first.

This is a real divergence between the two platforms and it is taken deliberately.
It is not a regression a macOS user experiences as one: there is no prior macOS
behaviour for them to lose, and the passthrough toggle is in the chrome where the
setup workflow needs it. The cost lands on whoever maintains two frontends that
now behave differently over the hole, which is a cost this record pays rather
than passes on silently.

**The Stop button is unreachable while recording.** By construction, not by
accident, which is why it is replaced rather than left in place. The menu bar
item is the primary stop path and needs no permission of any kind. The hotkey is
secondary and must use Carbon's `RegisterEventHotKey`:
`NSEvent.addGlobalMonitorForEventsMatchingMask` requires Accessibility, and
`AXIsProcessTrusted` was measured `false` on a developer's own machine. Shipping
a second permission prompt on top of Screen Recording to make stopping work is
not acceptable.

**The shared chrome grows a fifth platform hook.** `PlatformHooks` is documented
as a seam where every entry exists because a specific line needs it, and nothing
was added for symmetry. This adds one: what to render where the action button
goes while passthrough is on. X11 answers "the button"; macOS answers with the
live binding, or with the menu bar when there is no live binding. That is a real
addition to the seam and it should be justified in the diff that makes it, not
assumed by this record.

**Keyboard shortcuts stop arriving while passthrough is on.** `ignoresMouseEvents`
is about mouse events, but a window that cannot be clicked cannot take focus, and
key events go to the focused window. Esc and Print Screen work today because the
window has focus. They will not during recording, which is the same reason the
hotkey must be global.

## What would falsify this

- **A click landing wrong at the transition.** If the asynchronous flag turns out
  to matter even once per state change — a click posted in the window between
  `setIgnoresMouseEvents` and the server acting on it — the mode is not safe and
  the three windows come back. Measure the lag before wiring the transition.
- **`begin_resize` not implemented on the Quartz backend.** The Resizable style
  mask says AppKit is willing; it does not say GDK forwards a GTK-initiated
  resize. Untested — it needs a real pointer drag, and synthesised drags require
  Accessibility. `GLIMPSE_PROBE_HOLD=1 cargo run -p glimpse-macos --example
  single_window_frame` puts a grip on screen for a human to drag. If it does
  nothing, resize is still open, though no worse than today.
- **Sheet reflow moving the hole.** In one window the sheet appearing grows the
  window and GTK reflows; if that moves the hole while a recording is running,
  `geometry_drifted` must catch it. On X11 it already does. Verify rather than
  assume.
- **A dim that hides the recording border.** `.state-recording .glimpse-frame`
  turns the border red, and that border is how a user sees what is being
  recorded. At `alphaValue = 0.8` it is also 20% dimmer. If the red stops reading
  as red against a busy desktop, the cue and the mode indicator are fighting each
  other and the dim belongs on the chrome widgets rather than on the window.
  Decided by looking at it, not by a number.

## Consequences

Deleted: the three-window assembly in `app.rs`, `attach_strips`, `settle_status`,
the seam handling, and most of `layout.rs`.

Added: an `NSStatusItem`, a Carbon hotkey registration, the hotkey itself in
`config.toml` ([ADR 0008](0008-settings-and-themes.md)), a passthrough mode on the
session state, and a fifth `PlatformHooks` entry so the shared chrome can render
the stop path in the action button's place.

ADR 0015's central measurement is not weakened by this record — it is what makes
it possible. What changes is that whole-window click-through is treated as a
capability of the application rather than a property of one decorative window.
