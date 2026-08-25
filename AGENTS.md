# Working agreement

For coding agents, and for humans who would rather not rediscover these.

## Read first

- [`docs/adr/0000-x11-framing-window-spike.md`](docs/adr/0000-x11-framing-window-spike.md)
  — the two verification failures that shaped this codebase.
- [`docs/architecture.md`](docs/architecture.md) — the conversion chain.

## The rules that already cost something

**Numbers agreeing with numbers is not verification.** The computed capture rect
once matched `xwininfo` to the pixel and was still wrong by a 3px border. Grab
the rectangle and *look at the image*: `make selftest`.

**Never test click-through by moving the pointer.** `XQueryPointer`'s child field
is geometric and blind to input shape. It reports the framing window whether or
not click-through works. Read the shape back from the server —
`x11probe::input_shape`, or `cargo run --example root_geometry -- <xid>`.

**The capture target paints nothing.** `compute_bounds` returns a widget's border
box. The frame is painted by the parent so the target's bounds are already
correct. If you find yourself adding an inset constant to compensate for a
border, you have reintroduced a fixed bug — move the painting instead.

**Never derive DPI from monitor physical size.** This machine's monitor reports
1mm × 1mm. Only the integer scale factor is trusted.

**`ui::window_xid` is a deliberate choke point.** `GdkX11Surface::xid` is
deprecated since GTK 4.18 with no replacement. Keep every use of it in that one
function so the eventual fallback is a single edit.

## Scope discipline

v0.1 is GIF-only, X11-only, ffmpeg-only. Wayland is not a missing backend, it is
a different interaction model — see
[ADR 0002](docs/adr/0002-ffmpeg-pipeline-and-session-model.md). If a change wants
a `CaptureBackend` trait before a second backend actually exists, it is early.

## Before opening a PR

```sh
make check
```

Anything touching `geometry.rs` or the widget hierarchy also needs `make
selftest` **and an eyeball on the resulting PNG**. The test suite cannot see that
one.
