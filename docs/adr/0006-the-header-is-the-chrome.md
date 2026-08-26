# 0006 — The header bar is the window chrome

- **Status:** ACCEPTED
- **Date:** 2026-08-25
- **Source:** the `Glimpse Screen Recording UI` design document

## Context

The UI was designed as a document before being built, and it has no title bar:
a 44px header carries the size readout, the primary action, a format chip and a
menu, and everything below it is the capture hole. This is Peek's shape too — the
header bar is the entire chrome — and it is right for this product, because every
pixel of window that is not hole is pixels the user cannot record.

Implementing it means `set_decorated(false)`.

## Decision

The window is **undecorated**, and the header doubles as the drag handle via
`gtk::WindowHandle`. `GLIMPSE_DECORATIONS=server` restores window-manager
decorations.

## Because

A framing window that cannot be *moved* is useless, so the drag handle is not
optional — an undecorated window with no `WindowHandle` would be a broken
product, not a styling regression.

The design's tokens are ported verbatim into the CSS rather than approximated, so
the app and the mock cannot drift apart on colour. The four visual states map
directly onto the session machine: idle, recording (red border, pulsing dot,
elapsed timer), saved (blue dot, path, "Show in folder"), aborted (amber border
and the preserved-recording path).

## Glimpse draws its own resize edges

The first attempt shipped `set_decorated(false)` and assumed GTK would still
provide resize edges. **It does not**, and the window could not be resized at all
— found in use, then reproduced by simulating an edge drag with
`xdotool` and watching the geometry not move.

`set_titlebar(custom)` was tried next, on the theory that keeping client-side
decorations keeps their invisible resize borders. On gala it reported
`_GTK_FRAME_EXTENTS = 0,0,0,0` — no margin at all — and resize stayed broken.

So Glimpse installs its own grips: 8px edge strips and 16px corners in a
`GtkOverlay`, each with the matching resize cursor, handing the drag to the
compositor through `Toplevel::begin_resize`.

**The part that made it work was releasing the gesture.** `begin_resize` alone did
nothing: GTK's implicit pointer grab from the click gesture kept holding the
pointer, so the compositor's resize grab never started. Setting the sequence to
`EventSequenceState::Denied` straight after the call fixes it.

Verified by simulated drags — east 760→850, south 520→590, south-east corner
760×590→810×630 — against a control run with `GLIMPSE_DECORATIONS=server` proving
the test method detects a resize that is known to work. The control mattered: an
earlier run of the same test produced a **false negative**, because the window had
not been activated and the first synthetic click was consumed giving it focus.

## Costs accepted
- Eight overlay widgets exist purely to be invisible and catch drags. They cost
  nothing visually, but they are exactly the kind of thing a later refactor
  deletes without noticing the window stops resizing.
  `GLIMPSE_DECORATIONS=server` remains the escape hatch.
- Custom chrome means Glimpse no longer inherits the desktop's window controls or
  its theming for the title area. On a framing window that is the point, but it is
  still a divergence from the platform.
- Going undecorated makes GTK's surface transform potentially non-zero, which
  turned a latent bug into a live one — the input region and the capture rect had
  to apply the same transform. That was already fixed before this change landed
  (ADR 0004), and the self-test confirms the hole still lands correctly.
