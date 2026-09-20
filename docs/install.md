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

## macOS

macOS records, through the same chrome Linux runs, and resizes by dragging the
window's edges like any other.

This section was headed "in progress" long after it stopped being true, for two
reasons that had both been fixed: resize, which needed no code once the frame
became one window, and the `.app` bundle — whose build instructions were already
twenty lines below the sentence saying it did not exist.

What is genuinely not done is signing and notarization: the bundle is ad-hoc
signed, so a download through a browser is quarantined and macOS calls it
damaged, while the same file fetched with `curl` runs.

```sh
brew install gtk4 pkg-config ffmpeg
cargo run
```

macOS will ask for Screen Recording permission the first time, and ffmpeg
captures nothing until it is granted. The permission is granted to the program
that launched Glimpse — your terminal — not to the binary.

### Two things behave differently from Linux

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

**The pointer-capture setting is not offered.** avfoundation ignores
`-capture_cursor` — measured, and in the OFF direction: the pointer is never
drawn. A setting a backend cannot honour is hidden on that platform rather than
shown doing nothing ([ADR 0012](adr/0012-a-setting-a-backend-cannot-honour.md)).

### Building the app bundle

```sh
make bundle-macos      # -> target/Glimpse.app
```

This is the shape macOS actually wants ([ADR 0013](adr/0013-macos-ships-an-app-bundle.md)):
the binary, the 39 GTK dylibs it needs with their load paths rewritten, the
GSettings schemas and icon theme, and a bundle identifier for Screen Recording
permission to attach to. It refuses to finish unless no Homebrew path survives
anywhere in the bundle and the app it just built starts and reports a capture
rect.

Launching `Glimpse.app` from Finder is the only way the permission can persist
against Glimpse rather than against whatever terminal started it.

### What is missing

- **A published release artifact.** The bundle builds, but nothing ships it yet
  ([issue #13](https://github.com/viniciusdc/glimpse/issues/13)), so macOS is
  still install-from-source.
- **Signing and notarization.** The bundle is ad-hoc signed, which satisfies the
  kernel but not Gatekeeper. A bundle downloaded through a browser would be
  quarantined; one fetched with `curl` is not.

The capture path can also be exercised without any window at all:

```sh
cargo run -p glimpse-macos --example record    # a real GIF, from a fixed rect
```
