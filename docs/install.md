# Installing Glimpse

Recording works on **X11 only**. macOS builds and runs the frame but has no
controls yet, so it cannot record — the last section covers what does work there.

## Requirements

### Runtime

- An X11 session
- GTK4 >= 4.10
- FFmpeg >= 6 (developed against 6.1)

### Building

- Rust, stable
- `libgtk-4-dev` and `pkg-config`

### macOS

Builds and runs the frame only. Needs `gtk4`, `pkg-config` and `ffmpeg` from
Homebrew. There is no release artifact and no `.app` bundle yet.

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

macOS is being built in the open and is **not usable as a recorder yet**. What
exists today:

```sh
brew install gtk4 pkg-config ffmpeg
cargo run                    # puts the frame on screen, prints the region, no controls
```

It draws the frame, positions it, and reports the exact rectangle it would
capture. It has no buttons, so there is no way to start a recording from it, and
`Ctrl-C` is how you quit.

The capture path underneath is real and is checked on every commit — this records
a fixed region end to end, window included or not:

```sh
cargo run -p glimpse-macos --example record    # a real GIF, from a fixed rect
cargo run -p glimpse-macos --example frame     # the frame, with its geometry read back
```

macOS will ask for Screen Recording permission the first time, and ffmpeg
captures nothing until it is granted.

What is left is the chrome and the wiring between it and the session — tracked in
the [macOS milestone](https://github.com/viniciusdc/glimpse/milestones), with the
window model settled in [ADR 0015](adr/0015-the-frame-is-two-windows.md).
