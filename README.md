<div align="center">
  <picture>
    <source media="(prefers-color-scheme: dark)"  srcset="docs/assets/banner-dark.svg">
    <source media="(prefers-color-scheme: light)" srcset="docs/assets/banner-light.svg">
    <img src="docs/assets/banner-dark.svg" alt="glimpse — the hole is the capture region" width="860"/>
  </picture>
</div>

<br/>

<div align="center">

**A screen recorder with a framing window. GIF or MP4.**

[![Check](https://github.com/viniciusdc/glimpse/actions/workflows/check.yml/badge.svg)](https://github.com/viniciusdc/glimpse/actions/workflows/check.yml)
[![Status](https://img.shields.io/badge/status-v0.1%20%C2%B7%20record%20%E2%86%92%20gif%20%7C%20mp4-f0883e)](docs/roadmap.md)
[![Rust](https://img.shields.io/badge/rust-stable-000000?logo=rust&logoColor=white)](Cargo.toml)
[![GTK](https://img.shields.io/badge/gtk-4.x-4a86cf?logo=gnome&logoColor=white)](https://gtk-rs.org)
[![License: Apache-2.0](https://img.shields.io/badge/license-Apache--2.0-blue.svg)](LICENSE)
[![Platform](https://img.shields.io/badge/platform-Linux%20%C2%B7%20X11-lightgrey?logo=linux&logoColor=white)](#building)

</div>

Place the window over what you want to record. The hole in the middle **is** the
capture region — press Record, get a GIF or an MP4.

The design is [Peek](https://github.com/phw/peek)'s, which got it right. Glimpse
is an independent implementation in Rust on GTK4 and contains none of Peek's code.

## What it does

- **Frames a region by being a window.** The centre is transparent and
  click-through, so you can keep using whatever is underneath while you line it up.
- **GIF or MP4**, picked from the header. Both go through a lossless intermediate,
  so the format choice costs nothing at record time.
- **Refuses to record the wrong thing.** `x11grab` captures a fixed rectangle, so
  if the frame gets moved mid-recording the result would look plausible and be
  wrong. Glimpse detects the move and aborts instead.
- **Does not lose a recording to a failed encode.** The captured video is kept and
  the status line says where it is.
- **Does not leave ffmpeg running.** The child is reaped on every exit path, and
  killed by the kernel if Glimpse dies without getting the chance.

## Status

**v0.1 — recording works end to end.** What is missing is application furniture
rather than the product: output selection and persisted settings. Framerate and
cursor capture are currently fixed at 15fps with the cursor drawn, and output goes
to `~/glimpse.gif` or `~/glimpse.mp4`, disambiguated rather than overwritten.
[`docs/roadmap.md`](docs/roadmap.md) has the order and the reasoning.

Known limits, all deliberate and recorded:

- **X11 only, by design rather than omission.** The framing-window idea does not
  survive a compositor that mediates selection, so a Wayland build would be a
  different interaction, not a port ([ADR 0002](docs/adr/0002-ffmpeg-pipeline-and-session-model.md)).
- **An encode in progress cannot be cancelled**; quitting waits for it
  ([ADR 0005](docs/adr/0005-gif-encoding-and-the-atomic-commit.md)).
- **GIF quality matches Peek's**, since it is the same ffmpeg palette path. MP4 is
  smaller, but by less than folklore suggests — 1.5× on a mostly-static capture,
  measured ([ADR 0007](docs/adr/0007-gif-and-mp4.md)).

Why things are built the way they are, including the decisions that were reversed,
is in [`docs/adr/`](docs/adr/).

## Building

Requires stable Rust, an **X11 session**, and GTK4 development headers.

```sh
sudo apt install libgtk-4-dev ffmpeg
cargo run
```

`make check-reqs` reports what is missing, and `make` on its own lists every
target. `make headless` runs Glimpse on a private X server if you would rather it
did not appear on yours. To install it properly:

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

There is no screenshot of the app in this README on purpose: the middle of the
window is transparent, so any capture of it publishes whatever happened to be
behind it.

Glimpse draws its own window chrome, including its own resize edges, because GTK
provides none once the titlebar is replaced. If resizing misbehaves on your
compositor, `GLIMPSE_DECORATIONS=server` hands the frame back to the window
manager. [`docs/development.md`](docs/development.md#environment-variables) lists
every variable the binary reads.

## Development

```sh
make            # list every target
make check      # the gate: docs, formatting, clippy, tests
make headless   # run it on a private X server, off your screen
make smoke      # record → GIF and record → MP4, off your screen
```

Glimpse is a screen recorder, so testing it naturally means opening windows and
grabbing the display on the machine you are using. `make headless` and `make
smoke` run it against an `Xvfb` instead.

[`docs/development.md`](docs/development.md) covers the rest: verifying geometry
changes, checking click-through, and every environment variable the binary reads.

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
  encode.rs             GIF and MP4 encoding, and the atomic commit
  ui.rs                 The framing window: hole, input region, lock/unlock
examples/
  root_geometry.rs      Query X with no GTK window involved
  framing_window.rs     The smallest useful framing window
  record.rs             Record a fixed region, no GTK window involved
tests/
  geometry.rs           Clipping — the part of the chain testable without a display
  session.rs            Lifecycle policy: drift, retry, cancellation, shutdown
  capture.rs            ffmpeg arguments and workspace ownership
  encode.rs             Collision policy, argument shape, real GIF and MP4 encodes
data/
  glimpse.desktop       Desktop entry, installed by `make install`
scripts/
  headless.sh           Runs Glimpse on a private X server, off your screen
  sync-docs.sh          Regenerates the ADR index; fails the build on doc drift
docs/
  adr/                  Decision records, append-only
  assets/               README banner, light and dark
  architecture.md       The stack, the modules, the conversion chain
  development.md        Setting up, the gates, how to verify geometry changes
  roadmap.md            What comes next, in dependency order
```
<!-- END GENERATED layout -->

## Further reading

- [`docs/architecture.md`](docs/architecture.md) — the stack, the conversion chain, the session lifecycle
- [`docs/development.md`](docs/development.md) — setting up, the gates, verifying geometry and click-through
- [`docs/roadmap.md`](docs/roadmap.md) — what is done, and what comes next in dependency order
- [`CONTRIBUTING.md`](CONTRIBUTING.md) — what a pull request has to clear
- [`AGENTS.md`](AGENTS.md) — working agreement for coding agents
- [`docs/adr/`](docs/adr/) — decision records:
<!-- BEGIN GENERATED adr-index (regenerate with `make docs-sync`) -->
  - [0000](docs/adr/0000-x11-framing-window-spike.md) — The X11 framing-window spike
  - [0001](docs/adr/0001-rust-and-gtk4.md) — Rust and GTK4 as the stack
  - [0002](docs/adr/0002-ffmpeg-pipeline-and-session-model.md) — An ffmpeg-only pipeline, and an explicit session model
  - [0003](docs/adr/0003-apache-2-0.md) — Apache-2.0, and what that requires of the capture implementation
  - [0004](docs/adr/0004-review-corrections-and-the-lifecycle-spine.md) — Review corrections, and a lifecycle spine before capture
  - [0005](docs/adr/0005-gif-encoding-and-the-atomic-commit.md) — GIF encoding, and how the output is committed
  - [0006](docs/adr/0006-the-header-is-the-chrome.md) — The header bar is the window chrome
  - [0007](docs/adr/0007-gif-and-mp4.md) — GIF and MP4 as the initial output formats
<!-- END GENERATED adr-index -->

## Licence

Apache-2.0 — see [LICENSE](LICENSE) and [NOTICE](NOTICE).

Peek is GPL-3.0-or-later and remains so; it is prior art and an acknowledged
influence on the product design, and none of its code, resources or assets are
present here. See [ADR 0003](docs/adr/0003-apache-2-0.md) for the reasoning and
for the constraint it places on future work.
