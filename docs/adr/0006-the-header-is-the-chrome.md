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

## Costs accepted

- **Edge-resizing an undecorated GTK4 window is unverified.** GTK is expected to
  provide invisible resize edges for CSD windows, but that was not tested here —
  automating a drag from an edge is not something this project can do, and the
  developer's own window manager is the only real test. This matters more than it
  usually would: **the frame's size *is* the capture region**, so if resize is
  lost the product is lost. `GLIMPSE_DECORATIONS=server` exists for exactly this
  outcome, and it is the first thing to try if resizing feels wrong.
- Custom chrome means Glimpse no longer inherits the desktop's window controls or
  its theming for the title area. On a framing window that is the point, but it is
  still a divergence from the platform.
- Going undecorated makes GTK's surface transform potentially non-zero, which
  turned a latent bug into a live one — the input region and the capture rect had
  to apply the same transform. That was already fixed before this change landed
  (ADR 0004), and the self-test confirms the hole still lands correctly.
