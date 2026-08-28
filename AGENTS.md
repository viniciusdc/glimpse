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
`x11probe::input_shape`, or
`cargo run -p glimpse-x11 --example root_geometry -- <xid>`.

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

**`x11probe::window_xid` is a deliberate choke point.** `GdkX11Surface::xid` is
deprecated since GTK 4.18 with no replacement. Keep every use of it in that one
function so the eventual fallback is a single edit.

**Nothing toolkit-shaped goes in `glimpse-core`.** No `gtk4`, no `gdk4-*`, no
`x11rb`, no `objc2` — the manifest is the enforcement, so the rule holds only for
as long as that dependency list stays short
([ADR 0010](docs/adr/0010-capture-providers-and-a-platform-free-core.md)). If core
needs something from a platform, it arrives as data on `GrabCommand`; core does
not reach for it.

**A `cfg(not(target_os = "linux"))` stub is a decision, not a placeholder.**
`process_is_alive` returned `true` off Linux, which silently reduced the whole
stale-workspace sweep to a no-op on macOS. Nothing failed, nothing warned, and
every killed session leaked a temp directory. When you gate a function by
platform, write what the other platform actually does — and if the answer is
"nothing", say so in the doc comment and say what it costs.

**The self-test PNG is a picture of your screen.** `make selftest`
grabs whatever the framing window was over. Never attach it to a pull request,
an issue, or a commit. The README has no screenshot for the same reason.

**Do not test on someone else's screen.** This is a screen recorder: exercising it
means opening windows, warping the pointer and grabbing the display, on a machine
somebody is trying to use. `make smoke` and `make headless` run it against a
private `Xvfb` instead. Only resize, move and transparency genuinely need a real
session — see [`docs/development.md`](docs/development.md#working-off-screen).

## Scope discipline

Glimpse records GIF and MP4 and snapshots PNG, through ffmpeg, and the only
frontend that exists is X11. Wayland is not a missing backend, it is a different
interaction model — see
[ADR 0002](docs/adr/0002-ffmpeg-pipeline-and-session-model.md).

macOS is a different case and is being worked towards
([ADR 0010](docs/adr/0010-capture-providers-and-a-platform-free-core.md)). The
core is split out and platform-free; the seam between core and a backend is
`GrabCommand`, plain data. `glimpse-macos` records end to end, so the reason this
paragraph used to give for not writing a `CaptureProvider` trait — that no macOS
frame had been verified yet — has expired.

**There is still no `CaptureProvider` trait, and the reason is now the durable
one:** both backends are selected at compile time, so a trait would buy no
dispatch. That is ADR 0010's own argument, and it does not weaken as macOS
matures. Do not read the expired precondition as a gate that has since opened.

What is still missing on macOS is a *frontend* — there is no window, so the binary
refuses. The window model is decided in
[ADR 0011](docs/adr/0011-why-the-macos-frame-is-more-than-one-window.md).

**Do not offer a setting the backend cannot honour.** avfoundation ignores
`-capture_cursor`, measured, so the macOS frontend does not show the Capture
pointer switch — see
[ADR 0012](docs/adr/0012-a-setting-a-backend-cannot-honour.md). A switch that
flips, persists and changes nothing is the same failure as a file that lies about
its contents.

## Before opening a PR

```sh
make check
```

Anything touching either `geometry.rs` or the widget hierarchy also needs `make
selftest` **and an eyeball on the resulting PNG**. The test suite cannot see that
one — and note that splitting the crates did not change this. The chain's first
three stages need a realized GTK window, so they are still only exercised under
`make selftest-headless`, never by `cargo test`.

Off Linux, `make check` covers `glimpse-core` and the binary only; `glimpse-x11`
cannot build against a Quartz-backend GTK. A change touching the X11 frontend is
**unverified until it has run `make check` on Linux**, whatever it did elsewhere.

And when the image shows something suspicious, *establish* what it is rather than
assuming. A red band in a grab once looked exactly like the frame border bleeding
in; changing the border colour and re-grabbing proved it was content in the page
underneath. Visual inspection is a smoke test, not an oracle — see
[ADR 0004](docs/adr/0004-review-corrections-and-the-lifecycle-spine.md).
