# Installing Glimpse

Recording works on **X11 and macOS**. macOS is still marked in progress: it
cannot be resized and there is no `.app` bundle, and it behaves differently in
one visible way while recording. The last section covers both.

## Requirements

### Runtime

- An X11 session
- GTK4 >= 4.10
- FFmpeg >= 6 (developed against 6.1)

### Building

- Rust, stable
- `libgtk-4-dev` and `pkg-config`

### macOS

Needs `gtk4`, `pkg-config` and `ffmpeg` from Homebrew. There is no release
artifact and no `.app` bundle yet, so it is built from source.

## Installation

### From a release

```sh
curl -fsSL https://raw.githubusercontent.com/viniciusdc/glimpse/main/scripts/install.sh | sh
```

Read [the script](../scripts/install.sh) first — it is short, and that advice holds
for anything piped into a shell. It downloads the release tarball, **verifies its
SHA-256 against the published checksum and refuses to install on a mismatch**,
extracts into a temporary directory and copies out only the binary, and installs
to `~/.local/bin` without ever calling sudo.

`GLIMPSE_VERSION=v0.1.0` pins a version rather than taking the latest, and
`INSTALL_DIR=/usr/local/bin` puts it elsewhere — somewhere you would then need
permission to write.

Releases are not signed. The checksum guards against a corrupted or tampered
download only as far as the checksum itself is trustworthy, and both come from the
same host, so a compromise of that host defeats both.

### From source

There are no distribution packages yet.

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

## macOS (in progress)

macOS records, through the same chrome Linux runs. It is still marked in
progress because it cannot be resized and there is no `.app` bundle yet.

```sh
brew install gtk4 pkg-config ffmpeg
cargo run
```

macOS will ask for Screen Recording permission the first time, and ffmpeg
captures nothing until it is granted. The permission is granted to the program
that launched Glimpse — your terminal — not to the binary.

### One thing behaves differently from Linux

**While recording, the window stops accepting clicks entirely**, so you can work
in whatever is being recorded. That means the Stop button cannot be pressed. Use
the menu bar item, or the shortcut — `⌃⌥S` unless you change `stop_shortcut` in
`~/.config/glimpse/config.toml`. The button is replaced by whichever of those is
actually available while the mode is on.

The same mode is available on demand, as **Pass clicks through** in the header
menu, so the frame can be positioned over a live application without blocking
it. The same shortcut turns it back off.

Why it works this way, and the six things measured before settling on it, are in
[ADR 0017](adr/0017-click-through-is-a-mode-not-a-window.md).

### What is missing

- **An `.app` bundle**, so releases are source-only on macOS
  ([ADR 0013](adr/0013-macos-ships-an-app-bundle.md)).

The capture path can also be exercised without any window at all:

```sh
cargo run -p glimpse-macos --example record    # a real GIF, from a fixed rect
```
