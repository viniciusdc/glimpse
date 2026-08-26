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

**ffmpeg flags come from ffmpeg's documentation, never from Peek's source.**
Capture and encoding are the next thing to be written, and Peek's flag sequences
are short and easy to reproduce from memory after reading them. Doing so would
quietly break the Apache-2.0 basis of this project. Cite the doc in a comment
where a flag choice is non-obvious. See
[ADR 0003](docs/adr/0003-apache-2-0.md).

**Never derive DPI from monitor physical size.** A monitor on the development
machine reports its physical size as 1mm × 1mm, and monitors that lie about this
are common. Only the integer scale factor is trusted.

**`ui::window_xid` is a deliberate choke point.** `GdkX11Surface::xid` is
deprecated since GTK 4.18 with no replacement. Keep every use of it in that one
function so the eventual fallback is a single edit.

**The self-test PNG is a picture of your screen.** `make selftest`
grabs whatever the framing window was over. Never attach it to a pull request,
an issue, or a commit. The README has no screenshot for the same reason.

**Do not test on someone else's screen.** This is a screen recorder: exercising it
means opening windows, warping the pointer and grabbing the display, on a machine
somebody is trying to use. `make smoke` and `make headless` run it against a
private `Xvfb` instead. Only resize, move and transparency genuinely need a real
session — see [`docs/development.md`](docs/development.md#working-off-screen).

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

And when the image shows something suspicious, *establish* what it is rather than
assuming. A red band in a grab once looked exactly like the frame border bleeding
in; changing the border colour and re-grabbing proved it was content in the page
underneath. Visual inspection is a smoke test, not an oracle — see
[ADR 0004](docs/adr/0004-review-corrections-and-the-lifecycle-spine.md).
