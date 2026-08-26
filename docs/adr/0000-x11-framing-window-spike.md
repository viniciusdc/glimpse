# 0000 — The X11 framing-window spike

- **Status:** ACCEPTED (spike closed, code deleted)
- **Date:** 2026-08-25

## Context

Glimpse's defining operation is turning a widget rectangle into root-screen
pixels. Everything else — capture, encoding, output — is downstream of getting
that rectangle right.

GTK4 is hostile to this by design. It deliberately removed the window-positioning
APIs, on the position that an application should not know where it is on screen,
and `gdk_x11_surface_get_xid` — the escape hatch back to raw X11 — is
**deprecated as of GTK 4.18 with no documented replacement**. Choosing GTK4 for
an application whose window position is part of its data model was called a
category error during review, and that objection was correct on the facts.

So the toolkit choice was not assumed. A throwaway spike was written to answer
three questions before any product code existed:

- **Q1** Can we compute the root-pixel rect of an inner widget, accurately, on
  X11 under gala?
- **Q2** Does `gdk_surface_set_input_region` give a real click-through hole?
- **Q3** Does the scale-factor path survive a monitor whose EDID reports a
  physical size of 1mm × 1mm? (One on the development machine does.)

## Decision

**GTK4 is viable. Proceed.** All three questions passed on GTK 4.14.5 / X11 / gala.

| | Result | Evidence |
|---|---|---|
| Q1 | PASS | `TranslateCoordinates` → `(0,62)`; `xwininfo` independently → `Absolute upper-left 0, 62` |
| Q2 | PASS | Server-side `ShapeGetRectangles(INPUT)` → `700x49+0+0  24x350+0+49  24x350+676+49  700x51+0+399` — four bands around the hole |
| Q3 | PASS | `scale_factor` = 1; the bogus physical size is never consulted |

## Because — the two findings that justified running it at all

Both intermediate results were wrong, in opposite directions. Neither error was
reachable by comparing numbers against numbers, and both shaped the code that
replaced the spike.

**1. Q2 first reported FAIL, and the failure was an artefact of the test.**
Click-through was checked by parking the pointer in the hole and asking which
window sat underneath; the answer was "ours". A control probe on the frame
border returned "ours" as well — proving `XQueryPointer`'s child field is
computed geometrically and is **blind to the input shape**, so that method
cannot detect click-through at all. The only trustworthy evidence is reading the
shape back from the X server.

**2. Q1 reported PASS, and the pass was shallow.** The computed rect agreed with
`xwininfo` exactly. It was still wrong. It surfaced only when the captured PNG
was *looked at*: a 3px border of Glimpse's own chrome on all four edges, because
`compute_bounds` returns the widget's **border box**.

The general lesson, now a standing rule: **cross-checking a number against
another number is necessary and never sufficient. Grab the rectangle and look at
the image.**

## What survived into the product

- `x11probe::input_shape` — server-side shape readback, the only sound way to
  verify an input region.
- The border-box bug is fixed **structurally, not numerically**. The spike
  patched it with `inset = border_px * scale`, a magic number that would have to
  track the stylesheet forever. In the product the frame border is painted by the
  *parent* widget and the capture target paints nothing, so `compute_bounds`
  excludes the border by construction and the bug class ceases to exist. No inset
  constant appears anywhere in the codebase.
- `GLIMPSE_SELFTEST=1` runs both surviving checks — shape readback and
  grab-and-inspect — against the real binary.
- The input region is re-punched from a tick callback on bounds change, not from
  `connect_map`. `connect_map` fires before allocation, which is how Q2 first
  produced a spurious "window not sized yet".

## Costs accepted

The spike proves GTK4 works **today, on 4.14.5**. It does not repeal the
deprecation. On GTK ≥ 4.18 the call begins warning, and if
it is ever removed the fallbacks are GTK3 or a framing window owned directly
through `x11rb`. `ui::window_xid` is the single choke point for that reason —
when it breaks, exactly one function changes.

The spike code itself is deleted. This record is what it was for.
