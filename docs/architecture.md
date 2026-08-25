# Architecture

Glimpse is a **framing window**: a window whose middle is transparent and
click-through, which you place over whatever you want to record. That hole *is*
the capture region. Everything else follows from getting its rectangle right.

## The stack

| Layer | Choice | Why |
|---|---|---|
| Language | Rust 2021 | [ADR 0001](adr/0001-rust-and-gtk4.md) |
| Toolkit | GTK4 via `gtk4-rs` | [ADR 0001](adr/0001-rust-and-gtk4.md), validated by [ADR 0000](adr/0000-x11-framing-window-spike.md) |
| Positioning | `x11rb` direct to the X server | GTK4 will not tell an app where it is |
| Capture | `ffmpeg -f x11grab` | [ADR 0002](adr/0002-ffmpeg-pipeline-and-session-model.md) |
| Encoding | `ffmpeg palettegen/paletteuse` | [ADR 0002](adr/0002-ffmpeg-pipeline-and-session-model.md) |

## Modules

```
src/lib.rs        Library surface, so geometry is testable without a display
src/main.rs       The binary: application entry only
src/x11probe.rs   Direct X queries — window origin, root size, input-shape readback
src/geometry.rs   The widget → root-pixel conversion chain, with clipping
src/ui.rs         The framing window: hole, input region, lock/unlock
```

## The conversion chain

The one calculation the product cannot get wrong:

```
WidgetRect      compute_bounds, logical widget coordinates
   ↓            surface_transform
SurfaceRect     native surface coordinates, still logical
   ↓            × integer scale_factor
device pixels
   ↓            TranslateCoordinates(xid → root)
RootPixelRect   absolute, what x11grab is handed
   ↓            clipped_to(root)
capture rect
```

Each stage is a distinct type. Mixing logical and device coordinates is the
easiest mistake to make here, and the type system is cheap insurance against it.

Two rules are load-bearing, both learned the hard way in
[ADR 0000](adr/0000-x11-framing-window-spike.md):

- **The capture target paints nothing.** `compute_bounds` returns a widget's
  *border box*, so a border on the capture widget would be recorded in every GIF.
  The frame is painted by the parent instead, which removes the bug class rather
  than compensating for it. There is no inset constant anywhere in this codebase,
  and there should never be one.
- **Never derive DPI from monitor physical size.** The development machine's
  monitor reports 1mm × 1mm. Only the integer scale factor is consulted.

## Verifying geometry

Numbers agreeing with other numbers is not evidence — during the spike the
computed rect matched `xwininfo` exactly while being wrong by the border width.

```sh
GLIMPSE_SELFTEST=1 cargo run
```

Runs the two checks that do work: the input shape is read back **from the X
server**, and the computed rect is grabbed to `/tmp/glimpse-selftest.png` for
inspection. Any Glimpse chrome visible in that image means the rect is wrong.

## What is not here yet

Capture and encoding. The `lock()`/`unlock()` contract that guards them already
exists — it snapshots the rect and freezes the window, so the frame cannot drift
out from under a running recorder. See [`roadmap.md`](roadmap.md).
