# 0011 — Why the macOS frame is more than one window

- **Status:** PROPOSED
- **Date:** 2026-08-27
- **Relates to:** [ADR 0006](0006-the-header-is-the-chrome.md),
  [ADR 0010](0010-capture-providers-and-a-platform-free-core.md),
  [issue #1](https://github.com/viniciusdc/glimpse/issues/1)

## Context

[Issue #1](https://github.com/viniciusdc/glimpse/issues/1) proposes composing the
macOS frame from four border windows around an uncovered middle, and gives this
reason:

> GTK4's macOS backend hard-codes input shapes off … This is not a flag to flip —
> AppKit has **no regional input shape at all**. `NSWindow.ignoresMouseEvents` is
> a whole-window property.

and rules out the obvious alternative:

> Not a transparent window in the middle, either — a transparent window would
> still be returned by the window server's hit test, and would still swallow the
> first click that focuses it.

The second claim is false, and the first is true about `ignoresMouseEvents` while
being the wrong diagnosis. The composition it recommends is nonetheless correct.
This record exists because a right answer resting on a wrong reason will be
undone by the next person who checks the reason.

## What was measured

Everything below was read back from the window server with
`windowNumberAtPoint:belowWindowWithWindowNumber:`, never inferred from pointer
position — the same discipline as [ADR 0000](0000-x11-framing-window-spike.md),
for the same reason. Every check tests both directions: points inside the hole
that must **not** belong to us, and points on the border that must. Without the
second half, a frame that failed to render at all passes clean.

**A plain `NSWindow` with a transparent middle IS click-through.** One borderless
window, `setOpaque(false)`, `clearColor` background, opaque border drawn as
subviews, no `ignoresMouseEvents` anywhere. All five hole points returned another
application's window; all four border points returned ours. macOS hit-tests a
non-opaque window per pixel against its alpha channel, which is the equivalent of
an X11 input shape by another name.

This is not novel. LICEcap has shipped exactly this since 2011 — `clearColor`,
`setOpaque:NO`, `SWELL_SetWindowShadow(false)`, no `ignoresMouseEvents`, with the
capture taken separately via `CGDisplayCreateImageForRect`.

**A GTK4 window with the same visual result is NOT click-through.** Same test,
same both-directions checking, against `GdkMacosDisplay`. The border points pass,
so the window rendered. All five hole points came back as ours.

**And the pixels are not the problem.** `screencapture -l` on the GTK window,
which preserves alpha, gives `rgba=00000000` in the hole and `rgba=4080f5ff` on
the border. GTK renders the hole at **alpha 0**. The window server has exactly the
information it needs and does not act on it.

Six candidate causes were eliminated:

| tried | result |
|---|---|
| `NSWindow.isOpaque` | already `false` on GTK's window |
| `gdk_surface_set_opaque_region(empty)` | no change |
| `setBackgroundColor(clearColor)` | no change; GTK's is already `gray … 0 1e-05` |
| layer-backing (`wantsLayer`) | not the cause — the raw AppKit window still passes with it on |
| `GSK_RENDERER=opengl` (`GskGLRenderer`) | no change |
| `GSK_RENDERER=cairo` (`GskCairoRenderer`) | no change |

The renderer rows exist because the layer-backing row does not eliminate what it
appears to. `setWantsLayer(true)` on a raw `NSWindow` installs a plain `CALayer`;
GSK's GL renderer installs a GPU-backed layer, which is a different object and
could plausibly present "opaque" as its shape regardless of what was drawn into
it. Cairo is the path that draws into an image surface instead, and is therefore
the one most likely to present the alpha AppKit hit-tests against.

It does not. Renderer selection was confirmed to take effect rather than assumed
— `GSK_DEBUG=renderer` reports `Using renderer 'GskCairoRenderer' for surface
'GdkMacosToplevelSurface'` — because a silent fallback to GL would have produced
a passing-looking elimination that eliminated nothing.

The mechanism was not identified.

For the record, this build offers `cairo` and `opengl`; `broadway` and `vulkan`
are disabled at build time, and the list moves between releases, so
`GSK_RENDERER=help` is the authoritative version of this question on any given
machine.

## Decision

**On macOS the frame is composed of several windows, because GTK cannot make a
covered region click-through — not because AppKit lacks input shapes.**

There are three ways to stop the middle of the frame taking clicks, and macOS
with GTK has none of them:

| platform | mechanism | available |
|---|---|---|
| X11 + GTK | punch the input region (`ShapeInput`) | yes — what ships today |
| macOS + raw AppKit | window server hit-tests per-pixel alpha | yes |
| macOS + GTK | neither | **no** |

So: if the middle cannot be made click-through, do not put a window there.

**The chrome stays a GTK window, and keeps ADR 0006's shape.** The header is its
own window sitting above the hole, carrying the size readout, the split button,
the format chip and the menu — the bulk of the existing UI, unchanged. Four thin
windows draw the border. Nothing covers the hole.

**They move as one via `addChildWindow:ordered:`.** Measured: moving only the
parent by (137, −83) moved every child by exactly that delta, read back from the
window server, with a non-child window as a control that the same check correctly
flagged as not following.

## Because

**Keeping GTK keeps one UI source.** The alternative — a raw AppKit frontend in
LICEcap's shape — buys a genuinely simpler window model and loses every widget,
every stylesheet token ported verbatim from the design document, and all the
session wiring. The composition differs per platform; the interface does not.

**Issue #1's model was right for a reason it did not state.** Four strips work
because there is no window over the hole, which sidesteps the toolkit question
entirely. That is worth writing down, because the stated reason invites someone
to "fix" it by reaching for a transparent window that will not behave.

**ADR 0006 does not need superseding.** Its decisions — undecorated window, header
as chrome and drag handle, self-drawn resize grips — were never wrong; they were
scoped to a window model that was not stated because there was only one. The
header having "nowhere to live" was a consequence of the four-window sketch, and
it has somewhere to live again as soon as it is a window of its own.

## Consequences

`addChildWindow` solves *move* and not *resize*, now measured. A parent can
change size with no child noticing, so the strips have to be repositioned and
resized by hand on every frame resize.

**And there is a trap in how that is written.** Propagation depends on which call
is made, not on the state of the relationship:

| call on the parent | children follow? |
|---|---|
| `setFrameOrigin:` | yes, by exactly the delta |
| `setFrame:display:` — size only | n/a, nothing to follow |
| `setFrame:display:` — size **and** origin | **no**, not even the origin component |

The obvious way to write a resize is `setFrame:`, because size and origin change
together. Do that and the strips silently stop tracking — which is the shear this
composition was accused of, arriving through a door nobody was watching.

That reading was confirmed rather than assumed. Children failing to follow a
`setFrame:` has two very different explanations — the call does not propagate, or
the call severed the parent/child link — and they need different fixes. A
`setFrameOrigin:` issued immediately afterwards on the same windows moved all
four by exactly its delta, so the relationship was intact the whole time and the
difference is purely the API.

**So: never let a `setFrame:` on the parent carry an origin change.** Decompose
it — `setFrameOrigin:` for the move, so AppKit does the lockstep, then size-only
frames for each strip.

`performWindowDragWithEvent:` (macOS 10.11+) hands a drag to the window server and
returns immediately, which is the native equivalent of the `begin_resize` handoff
ADR 0006 needed `EventSequenceState::Denied` to make work. The X11 workaround
should not be ported without checking whether the platform already does it.

Five windows have to be laid out and kept consistent where X11 has one. The
X11 frontend keeps its single shaped window; this composition is macOS-only, and
lives behind the crate boundary ADR 0010 established.

## What would falsify this

Finding the GTK knob. If a GTK4 surface on macOS can be made to inherit the
window server's alpha hit test, the whole composition collapses back to one
window and this record should be superseded rather than amended. Six candidates
have been eliminated; the search was not exhaustive.

The renderer deserves specific mention, because it is the candidate most likely
to be tried next by someone who has not read this far. It is not a fixed property
of the build — `GSK_RENDERER` changes it at run time, today, without recompiling
anything. Both renderers this build ships were measured and neither helps, but a
future GTK4 release adding or changing one could move the answer in either
direction, silently and without any change on our side. The measurement is two
commands and worth repeating before building on it.

## Where measurement stops being cheap

Everything above is geometry read back from the window server, which is exact and
costs nothing. Whether a hand-written four-strip resize **shears visibly** during
a drag is not answerable that way: it needs a real pointer dragging a real frame,
and a frame that does not exist yet. That question is deferred to the frontend
rather than guessed at here, and it is the one place in this record where the
next answer will cost more than a spike.
