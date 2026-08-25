<div align="center">

# glimpse

**An animated GIF screen recorder with a framing window.**

[![Check](https://github.com/viniciusdc/glimpse/actions/workflows/check.yml/badge.svg)](https://github.com/viniciusdc/glimpse/actions/workflows/check.yml)
[![Status](https://img.shields.io/badge/status-v0.1%20%C2%B7%20framing%20window-f0883e)](docs/roadmap.md)
[![Rust](https://img.shields.io/badge/rust-stable-000000?logo=rust&logoColor=white)](Cargo.toml)
[![GTK](https://img.shields.io/badge/gtk-4.x-4a86cf?logo=gnome&logoColor=white)](https://gtk-rs.org)
[![License: Apache-2.0](https://img.shields.io/badge/license-Apache--2.0-blue.svg)](LICENSE)
[![Platform](https://img.shields.io/badge/platform-Linux%20%C2%B7%20X11-lightgrey?logo=linux&logoColor=white)](#building)

</div>

You place a window over the thing you want to record, and the hole in the middle
*is* the capture region. Press record, get a GIF.

The idea is not new: [Peek](https://github.com/phw/peek) established it, and
Glimpse keeps its product design wholesale because that design is right. What
Glimpse does not keep is any of Peek's code — this is an independent
implementation in Rust on GTK4, where Peek is Vala on GTK3.

## Why

Peek works. The reasons to rewrite it are narrow and worth stating plainly rather
than dressing up:

- **A conversion chain that is typed.** Turning a widget rectangle into
  root-screen pixels is the one calculation this product cannot get wrong, and it
  is exactly where logical and device coordinates get mixed. Each stage here is a
  distinct type.
- **A verification story, written down.** Two of the first three bugs found in
  this codebase were confidently verified as *absent* by methods that could not
  have detected them. What replaced those methods is in
  [ADR 0000](docs/adr/0000-x11-framing-window-spike.md) and enforced by
  `make selftest`.
- **A path to Wayland that is honest about being a different product.** Peek's
  Wayland support is its weakest area. Glimpse does not pretend a compositor that
  mediates selection can host a window that chooses its own capture rectangle —
  see [ADR 0002](docs/adr/0002-ffmpeg-pipeline-and-session-model.md).

What it does *not* claim: better GIF quality. v0.1 uses the same ffmpeg
`palettegen`/`paletteuse` path Peek does, and says so.

## Status

**v0.1 — the framing window works; capture does not exist yet.**

Running today: a transparent, click-through framing window whose input region is
re-punched whenever the layout settles; `capture_rect` running the full
conversion chain with clipping to the root window; and a `lock()`/`unlock()`
contract that snapshots the rect and disables resizing. Note the precise claim:
`lock()` does **not** stop a window manager from *moving* the frame, so movement
is detected by `geometry_drifted()` rather than assumed away.

The toolkit question is settled and it was settled by evidence, not preference.
GTK4 removed the window-positioning APIs this product is built on, and the escape
hatch back to raw X11 is deprecated as of GTK 4.18 **with no replacement**. Rather
than assume, a throwaway spike answered three questions; all three passed, and
[ADR 0000](docs/adr/0000-x11-framing-window-spike.md) records both the verdict
and the deprecation that is *not* thereby repealed.

The session lifecycle exists and is fully tested — a pure state machine that maps
events to effects, so policies like "a failed encode must not cost the recording"
and "a frame that moves mid-recording aborts" are pinned by tests that need no
display. The Record button works: it drives the machine, records off the UI thread, and
aborts if the frame moves mid-recording. What is missing is the last step — GIF
encoding — so a recording currently ends preserved on disk rather than converted. Order and reasoning in [`docs/roadmap.md`](docs/roadmap.md).

> There is no screenshot in this README, deliberately. The middle of the window is
> transparent, so any capture of it publishes whatever happened to be behind it.

## Building

Requires stable Rust, an **X11 session**, and GTK4 development headers.

```sh
sudo apt install libgtk-4-dev ffmpeg
cargo run
```

`make check-reqs` reports what is missing, and `make` on its own lists every
target. To install it properly:

```sh
make install                    # ~/.local/bin plus a desktop entry
make install PREFIX=/usr/local  # or wherever
```

**Wayland is not supported, and not by omission.** Glimpse checks the GDK backend
at startup and exits with an explanation rather than running and misbehaving.
Connecting to an X server is not sufficient evidence — under Wayland, XWayland
usually answers on `$DISPLAY` while GTK selects its own backend, so the check is
on the backend GTK actually chose.

`ffmpeg` is a runtime dependency, not a build one — the framing window runs
without it, but nothing will record.

## Verifying it works

```sh
make selftest
```

Reads the input shape back **from the X server** — the only sound way to confirm
click-through — then grabs the computed rectangle to
`/tmp/glimpse-selftest.png`. Open that image. Any part of Glimpse's own interface
in it means the rectangle is wrong, whatever the numbers said.

That last sentence is the whole lesson of this project so far: during the spike
the computed rectangle matched `xwininfo` to the pixel and was still wrong by the
3px width of the frame border.

## Project layout

<!-- BEGIN GENERATED layout (verified by scripts/sync-docs.sh; edit descriptions freely, paths are checked) -->
```
AGENTS.md               Working agreement — the failure modes already hit here
Makefile                make check is the gate; make selftest is the one CI can't run
src/
  lib.rs                Library surface, so the logic is testable without a display
  main.rs               The binary: application entry and shutdown ownership
  x11probe.rs           The X11 boundary — origin, root size, input-shape readback
  geometry.rs           WidgetRect → SurfaceRect → RootPixelRect, with clipping
  session.rs            The recording lifecycle: pure state machine, no I/O
  capture.rs            The ffmpeg recorder: owns the child, reaps on every path
  worker.rs             Runs the recorder off the UI thread; dropping it reaps
  ui.rs                 The framing window: hole, input region, lock/unlock
examples/
  root_geometry.rs      Query X with no GTK window involved
  framing_window.rs     The smallest useful framing window
  record.rs             Record a fixed region, no GTK window involved
tests/
  geometry.rs           Clipping — the part of the chain testable without a display
  session.rs            Lifecycle policy: drift, retry, cancellation, shutdown
  capture.rs            ffmpeg arguments and workspace ownership
data/
  glimpse.desktop       Desktop entry, installed by `make install`
scripts/
  sync-docs.sh          Regenerates the ADR index; fails the build on doc drift
docs/
  adr/                  Decision records, append-only
  architecture.md       The stack, the modules, the conversion chain
  development.md        Setting up, the gates, how to verify geometry changes
  roadmap.md            What comes next, in dependency order
```
<!-- END GENERATED layout -->

## Further reading

- [`docs/architecture.md`](docs/architecture.md) — the conversion chain and why each stage is its own type
- [`docs/development.md`](docs/development.md) — setting up, the gates, verifying geometry and click-through
- [`docs/roadmap.md`](docs/roadmap.md) — capture, encoding, the session state machine
- [`CONTRIBUTING.md`](CONTRIBUTING.md) — what a pull request has to clear
- [`AGENTS.md`](AGENTS.md) — working agreement for coding agents
- [`docs/adr/`](docs/adr/) — decision records:
<!-- BEGIN GENERATED adr-index (regenerate with `make docs-sync`) -->
  - [0000](docs/adr/0000-x11-framing-window-spike.md) — The X11 framing-window spike
  - [0001](docs/adr/0001-rust-and-gtk4.md) — Rust and GTK4, rewriting Peek
  - [0002](docs/adr/0002-ffmpeg-pipeline-and-session-model.md) — An ffmpeg-only pipeline, and an explicit session model
  - [0003](docs/adr/0003-apache-2-0.md) — Apache-2.0, and what that requires of the capture implementation
  - [0004](docs/adr/0004-review-corrections-and-the-lifecycle-spine.md) — Review corrections, and a lifecycle spine before capture
<!-- END GENERATED adr-index -->

## Licence

Apache-2.0 — see [LICENSE](LICENSE) and [NOTICE](NOTICE).

Peek is GPL-3.0-or-later and remains so; it is prior art and an acknowledged
influence on the product design, and none of its code, resources or assets are
present here. See [ADR 0003](docs/adr/0003-apache-2-0.md) for the reasoning and
for the constraint it places on future work.
