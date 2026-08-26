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

Simple screen recorder for a region of your screen, with an easy to use interface.

> The design was inspired by the now-deprecated
> [Peek](https://github.com/phw/peek), which got all the fundamentals right but
> unfortunately could not be maintained any longer. While looking for a
> replacement I found that none of the available tools were quite like what I
> liked about it, so I decided to build Glimpse — an independent implementation
> in Rust on GTK4 that solves a similar problem, and contains none of Peek's code.

## About

Glimpse makes it easy to create short screencasts of a screen area. You place the
Glimpse window over the part of the screen you want to record, press **Record**,
and get an animated GIF or an MP4. The hole in the middle of the window *is* the
recording area — it is transparent and clicks pass straight through it, so you can
keep using the application underneath while you line the frame up and while you
record.

<div align="center">
  <img src="docs/assets/demo.gif" alt="Glimpse: framing a region, recording it, and saving a GIF" width="800"/>
</div>

<p align="center">
  <sub>An illustration, not a screen capture — drawn by
  <a href="scripts/make-demo.py"><code>scripts/make-demo.py</code></a> and assembled with
  Glimpse's own encoding pipeline. See the FAQ for why it is not a real recording.</sub>
</p>

It was built for the cases where a screenshot is not enough and a real screencast
is too much: showing a UI interaction in a pull request, attaching a reproduction
to a bug report, or demonstrating a feature in a README.

Glimpse is **not** a general purpose screencast application. It records one region
of one screen, silently, to one file. There is no audio, no webcam, no editing, no
streaming, and no full-desktop or multi-monitor capture. If you need those, use
OBS.

Glimpse runs on **X11 only**. See the FAQ for why.

## Requirements

### Runtime

- An X11 session
- GTK4 >= 4.10
- FFmpeg >= 6 (developed against 6.1)

### Building

- Rust, stable
- `libgtk-4-dev` and `pkg-config`

## Installation

There are no distribution packages yet. Build it from source:

```sh
sudo apt install libgtk-4-dev ffmpeg     # or your distribution's equivalent
git clone https://github.com/viniciusdc/glimpse.git
cd glimpse
make install                             # into ~/.local, with a desktop entry
```

`make install PREFIX=/usr/local` installs system-wide, and `make uninstall`
removes it. `make check-reqs` reports anything missing before a build finds out.

To run it without installing:

```sh
cargo run
```

## Usage

1. Launch Glimpse. Move and resize the window until the hole covers what you want
   to record — drag the header to move it, drag any edge or corner to resize.
2. Pick **GIF** or **MP4** from the chip in the header.
3. Press **Record**. Press **Stop** when you are done.
4. The file is written to your videos folder as `glimpse.gif` or `glimpse.mp4`.
   An existing file is never overwritten — you get `glimpse-1.gif`, and so on.

Use the menu in the header to pick where recordings are saved, and to choose a
light, dark, or system-matching theme. Both are remembered.

The header shows the exact pixel size of the recording area, and a timer while
recording. The status line at the bottom tells you where the finished file went.

Recording is currently fixed at 15 frames per second with the mouse cursor drawn.
Both live in the settings file and can be edited by hand, but have no interface
yet — see [`docs/roadmap.md`](docs/roadmap.md).

## Frequently asked questions

### Can I click things inside the recording area while recording?

Yes. The recording area is a real hole: Glimpse sets an X input shape so the
middle of the window does not accept pointer events at all, and they go to
whatever is underneath. This does not depend on your window manager or on
stacking order.

### Where does my recording go?

Into your videos folder — `XDG_VIDEOS_DIR` if you have one, otherwise your home
directory — as `glimpse.gif` or `glimpse.mp4`. Change it with **Save recordings
to…** in the header menu. If that name is taken Glimpse counts up —
`glimpse-1.gif`, `glimpse-2.gif` — rather than overwriting a file you might still
want. The status line names the file it just wrote, and **Show in folder** opens
it.

### Why is my GIF so large?

Because it is a GIF. Every frame is a full image with a 256-colour palette, and
there is no motion compensation, so file size scales with how much of the screen
changes. Recording a smaller area, or something with less motion, helps most.

If the destination accepts video, choose **MP4** instead. It is meaningfully
smaller — though by less than is often claimed: on a mostly-static capture it came
out about 1.5× smaller, not ten times.

### Why use GIF at all then?

Because it plays inline, automatically, silently and everywhere — in issue
trackers, pull requests, chat clients and documentation, with no player controls
and no click to start. That is the entire reason the format survives, and it is
why it is the default here.

### My recording stopped by itself and said the frame moved. Why?

Glimpse records a fixed rectangle of the screen. If the window is moved after
recording starts — dragged, or moved by the window manager — everything captured
after the move is of the wrong region, while the resulting file still looks
perfectly plausible. Rather than hand you a wrong recording, Glimpse stops and
tells you. The captured video up to that point is kept, and the status line says
where.

Resizing is disabled while recording for the same reason.

### Encoding failed. Did I lose the recording?

No. The captured video is preserved and the status line gives you its path. Only
the conversion failed, so you can retry from that file with ffmpeg directly.

### Where are my settings stored?

`~/.config/glimpse/config.toml`. It holds the theme, the output format and
folder, and the framerate and cursor setting. It is written whenever you change
something rather than at exit, so a preference survives even if Glimpse is killed.
If the file is unreadable Glimpse says so and starts with defaults rather than
refusing to run.

### Can I record audio, or my webcam, or the whole desktop?

No, and none of these are planned. Glimpse records one silent region. See *About*.

### Why no Wayland support?

Not an omission — the idea does not survive the transition. Glimpse works by being
a window that knows where it is on screen and declares its own capture rectangle.
Under Wayland the compositor mediates screen capture: an application asks the
portal, and the *user* picks what gets shared. A framing window cannot choose its
own region, so a Wayland version would be a different application with a different
interaction, not a port of this one.

Glimpse checks which display backend GTK actually chose at startup and exits with
an explanation rather than running and misbehaving. Note that having `DISPLAY` set
is not enough to be on X11 — under Wayland, XWayland usually answers it too.

### Is that animation a real recording?

No, and it says so under the image. It is drawn frame by frame by
[`scripts/make-demo.py`](scripts/make-demo.py) — though it is assembled into a GIF
by Glimpse's own pipeline, an ffv1 intermediate and then `palettegen`/`paletteuse`,
so the file itself is produced exactly the way a real recording would be.

There is no real screen capture in this README for two reasons. The middle of the
Glimpse window is transparent, so any genuine capture of it also publishes
whatever happened to be behind it. And the headless X server used for automated
testing has no compositor, so on it the transparency would not composite and the
hole — the one thing worth showing — would come out black.

Run `make demo` to regenerate the animation.

## Contributing

Bug reports and pull requests are welcome.
[`CONTRIBUTING.md`](CONTRIBUTING.md) covers what a pull request has to clear;
[`docs/development.md`](docs/development.md) covers setting up.

```sh
make            # list every target
make check      # the gate: docs, formatting, clippy, tests
make headless   # run it on a private X server, off your screen
make smoke      # record → GIF and record → MP4, off your screen
```

Glimpse is a screen recorder, so testing it naturally means opening windows and
grabbing the display on the machine you are using. `make headless` and `make
smoke` run it against an `Xvfb` instead.

Decisions — including the ones that were reversed and why — are recorded in
[`docs/adr/`](docs/adr/).

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
  config.rs             Persisted settings: theme, format, output folder
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
  config.rs             Defaults, round-trip, and surviving a corrupt file
  encode.rs             Collision policy, argument shape, real GIF and MP4 encodes
data/
  glimpse.desktop       Desktop entry, installed by `make install`
scripts/
  headless.sh           Runs Glimpse on a private X server, off your screen
  make-demo.py          Draws the README animation, frame by frame
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
  - [0008](docs/adr/0008-settings-and-themes.md) — Settings, and what the theme is allowed to change
<!-- END GENERATED adr-index -->

## Licence

Apache-2.0 — see [LICENSE](LICENSE) and [NOTICE](NOTICE).

Peek is GPL-3.0-or-later and remains so; it is prior art and an acknowledged
influence on the product design, and none of its code, resources or assets are
present here. See [ADR 0003](docs/adr/0003-apache-2-0.md) for the reasoning and
for the constraint it places on future work.
