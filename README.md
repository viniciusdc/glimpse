<div align="center">
  <picture>
    <source media="(prefers-color-scheme: dark)"  srcset="docs/assets/banner-dark.svg">
    <source media="(prefers-color-scheme: light)" srcset="docs/assets/banner-light.svg">
    <img src="docs/assets/banner-dark.svg" alt="glimpse" width="860"/>
  </picture>
</div>

<br/>

<div align="center">

[![Check](https://github.com/viniciusdc/glimpse/actions/workflows/check.yml/badge.svg)](https://github.com/viniciusdc/glimpse/actions/workflows/check.yml)
[![Status](https://img.shields.io/badge/status-v0.1.0-f0883e)](docs/roadmap.md)
[![Records on](https://img.shields.io/badge/records%20on-Linux%20%C2%B7%20X11%20and%20macOS-lightgrey)](docs/install.md)
[![License: Apache-2.0](https://img.shields.io/badge/license-Apache--2.0-blue.svg)](LICENSE)

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
recording area — it is transparent, and clicks pass through it to the application
underneath, so you can keep working while you record. On Linux that is true of
the hole at all times; on macOS the whole window stops taking clicks while a
recording runs, and on demand otherwise, which the next section explains.

<div align="center">
  <img src="docs/assets/demo.gif" alt="Glimpse: framing a region, recording it, encoding, saving, taking a snapshot, and aborting when the frame moves" width="880"/>
</div>

<p align="center">
  <sub>An illustration, not a screen capture — drawn frame by frame by
  <a href="scripts/make-demo.py"><code>scripts/make-demo.py</code></a>, then assembled with
  Glimpse's own encoding pipeline.</sub>
</p>

It was built for the cases where a screenshot is not enough and a real screencast
is too much: showing a UI interaction in a pull request, attaching a reproduction
to a bug report, or demonstrating a feature in a README.

Glimpse is **not** a general purpose screencast application. It records one region
of one screen, silently, to one file. There is no audio, no webcam, no editing, no
streaming, and no full-desktop or multi-monitor capture. If you need those, use
OBS.

## Platforms

**Linux/X11** records, and is the older and better-covered of the two.

**macOS** records, through the same chrome, and ships as an `.app` bundle
carrying its own GTK. Two things differ. The window stops accepting clicks while
a recording runs — all of it, not just the hole — so you can work in whatever is
being recorded, and a menu bar item or a configurable shortcut stops it
([ADR 0017](docs/adr/0017-click-through-is-a-mode-not-a-window.md)). And the
pointer-capture setting is not offered, because avfoundation ignores it and a
switch that does nothing is worse than no switch
([ADR 0012](docs/adr/0012-a-setting-a-backend-cannot-honour.md)).
[`docs/install.md`](docs/install.md#macos) has the rest.

**Wayland** is out by design, and [the FAQ](docs/faq.md#why-no-wayland-support)
explains why it would be a different application rather than a missing feature.

## Install

```sh
curl -fsSL https://raw.githubusercontent.com/viniciusdc/glimpse/main/scripts/install.sh | sh
```

Read [the script](scripts/install.sh) first — it is short, and that advice holds
for anything piped into a shell.

Building from source, pinning a version, installing system-wide, and what the
checksum does and does not protect you from: [`docs/install.md`](docs/install.md).

## Usage

1. Launch Glimpse. Move and resize the window until the hole covers what you want
   to record — drag the header to move it, drag any edge or corner to resize.
2. Pick **GIF** or **MP4** from the chip in the header.
3. Press **Record**, then **Stop** — or **Esc**. On macOS the window takes no
   clicks while recording, so neither reaches it: use the menu bar item or the
   shortcut, which the button is replaced by while the mode is on. For a still
   image instead, choose **Snapshot** from the arrow beside the button, or press
   **Print Screen**; it saves a PNG immediately, with no start and stop.
4. The file is written to your videos folder — `~/Videos` on Linux, `~/Movies` on
   macOS — as `glimpse.gif` or `glimpse.mp4`.
   An existing file is never overwritten — you get `glimpse-1.gif`, and so on.

The gear in the header opens settings: frame rate, pointer capture, output
format and folder, and the theme. Everything is remembered. (Pointer capture is
Linux-only — see **Platforms** above.)

The header shows the exact pixel size of the recording area, and a timer while
recording. The status line at the bottom tells you where the finished file went.

## Documentation

| | |
|---|---|
| [Installing](docs/install.md) | Requirements, from source, macOS |
| [FAQ](docs/faq.md) | Where recordings go, why a GIF is large, why one stopped by itself |
| [Architecture](docs/architecture.md) | The stack, the conversion chain, the session lifecycle |
| [Project layout](docs/layout.md) | Every file, and what it is for |
| [Development](docs/development.md) | Setting up, the gates, verifying geometry and click-through |
| [Roadmap](docs/roadmap.md) | What is done, and what comes next in dependency order |
| [Releasing](docs/releasing.md) | Cutting a release, and verifying it like a user |
| [Decision records](docs/adr/) | Why things are the way they are, including the reversals |
| [Contributing](CONTRIBUTING.md) | What a pull request has to clear |
| [Working agreement](AGENTS.md) | For coding agents, and the failure modes already hit here |

## Contributing

Bug reports and pull requests are welcome.

```sh
make                  # list every target
make check            # the gate: docs, formatting, clippy, tests, and that no
                      # journey or release artifact is left undriven
make headless         # run it on a private X server, off your screen  (Linux)
make smoke            # record → GIF and record → MP4, off your screen (Linux)
make journeys-macos   # the same journeys on macOS, on your real screen
```

Glimpse is a screen recorder, so testing it naturally means opening windows and
grabbing the display on the machine you are using. On Linux `make headless` and
`make smoke` run it against an `Xvfb` instead. There is no `Xvfb` on macOS, so
the macOS journeys use your actual screen — which is why
[AGENTS.md](AGENTS.md) says not to run them on somebody else's.

## Licence

Apache-2.0 — see [LICENSE](LICENSE) and [NOTICE](NOTICE).
