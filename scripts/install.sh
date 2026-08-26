#!/bin/sh
#
# Install Glimpse from a GitHub release.
#
#   curl -fsSL https://raw.githubusercontent.com/viniciusdc/glimpse/main/scripts/install.sh | sh
#   curl -fsSL .../install.sh | GLIMPSE_VERSION=v0.1.0 sh
#   curl -fsSL .../install.sh | INSTALL_DIR=/usr/local/bin sh
#
# You are about to pipe a script from the internet into a shell. Read it first;
# it is short, and that advice is worth as much here as anywhere else.
#
# What it does, and what it refuses to do:
#
#   * Verifies the SHA-256 of the download against the checksum published beside
#     it, and REFUSES TO INSTALL on a mismatch or a missing checksum. This is the
#     point of the script. A tarball that fails this check is deleted, not
#     "installed with a warning".
#   * Extracts into a temporary directory and copies out only the one file it
#     expects, so a tarball containing paths like ../../.bashrc cannot place
#     anything anywhere.
#   * Installs to ~/.local/bin by default and never calls sudo on its own. If you
#     point INSTALL_DIR somewhere privileged, you elevate it yourself.
#   * Refuses to run on anything but Linux x86_64, because that is the only
#     binary published. Glimpse is an X11 application by design; there is no
#     macOS or Windows build to fall back to.
#
# It does not verify a signature, because the releases are not signed. The
# checksum protects against a corrupted or truncated download and against a
# tampered artifact *if* the checksum itself is trustworthy — and both come from
# the same host, so a compromise of that host defeats both. Say so rather than
# imply more.

set -eu

REPO="viniciusdc/glimpse"
BIN="glimpse"
: "${GLIMPSE_VERSION:=latest}"
: "${INSTALL_DIR:=${HOME}/.local/bin}"

die() { printf '\033[31merror:\033[0m %s\n' "$*" >&2; exit 1; }
say() { printf '\033[36m==>\033[0m %s\n' "$*"; }

need() { command -v "$1" >/dev/null 2>&1 || die "$1 is required but not installed"; }
need tar
need install

if command -v curl >/dev/null 2>&1; then
  # --proto '=https' refuses a redirect to plain http; -f fails on 404 rather
  # than saving an error page and calling it a tarball.
  fetch() { curl -fsSL --proto '=https' --tlsv1.2 "$1" -o "$2"; }
  fetch_stdout() { curl -fsSL --proto '=https' --tlsv1.2 "$1"; }
elif command -v wget >/dev/null 2>&1; then
  fetch() { wget -qO "$2" "$1"; }
  fetch_stdout() { wget -qO- "$1"; }
else
  die "neither curl nor wget is available"
fi

if command -v sha256sum >/dev/null 2>&1; then
  sum() { sha256sum "$1" | cut -d' ' -f1; }
elif command -v shasum >/dev/null 2>&1; then
  sum() { shasum -a 256 "$1" | cut -d' ' -f1; }
else
  die "no sha256sum or shasum available; refusing to install unverified"
fi

# ---------------------------------------------------------------- platform --
os=$(uname -s)
arch=$(uname -m)
[ "$os" = "Linux" ] || die "Glimpse is Linux-only (found $os). It is an X11 application by design; see docs/faq.md"
[ "$arch" = "x86_64" ] || die "no published binary for $arch (only x86_64). Build from source: https://github.com/$REPO"

# ------------------------------------------------------------------ version --
if [ "$GLIMPSE_VERSION" = "latest" ]; then
  say "resolving the latest release"
  # No jq dependency: pull the tag out of the redirect-free JSON with sed.
  GLIMPSE_VERSION=$(fetch_stdout "https://api.github.com/repos/$REPO/releases/latest" \
    | sed -n 's/.*"tag_name": *"\([^"]*\)".*/\1/p' | head -n1)
  [ -n "$GLIMPSE_VERSION" ] || die "could not determine the latest release; pass GLIMPSE_VERSION=vX.Y.Z"
fi

archive="${BIN}-${GLIMPSE_VERSION}-linux-x86_64.tar.gz"
base="https://github.com/$REPO/releases/download/$GLIMPSE_VERSION"

tmp=$(mktemp -d) || die "could not create a temporary directory"
trap 'rm -rf "$tmp"' EXIT INT TERM

# ----------------------------------------------------------------- download --
say "downloading $archive"
fetch "$base/$archive" "$tmp/$archive" || die "download failed — does $GLIMPSE_VERSION exist?"
fetch "$base/$archive.sha256" "$tmp/$archive.sha256" \
  || die "no checksum published for $archive; refusing to install unverified"

# ------------------------------------------------------------------- verify --
expected=$(cut -d' ' -f1 <"$tmp/$archive.sha256")
actual=$(sum "$tmp/$archive")
[ -n "$expected" ] || die "the published checksum is empty; refusing to install"
if [ "$expected" != "$actual" ]; then
  rm -f "$tmp/$archive"
  die "checksum mismatch for $archive
  expected $expected
  actual   $actual
The download has been deleted. Do not install it."
fi
say "checksum verified"

# ------------------------------------------------------------------ extract --
# Into the temporary directory, then copy out only the file expected by name —
# so nothing in the archive decides where anything lands.
tar -xzf "$tmp/$archive" -C "$tmp"
extracted="$tmp/${BIN}-${GLIMPSE_VERSION}-linux-x86_64/$BIN"
[ -f "$extracted" ] || die "the archive did not contain $BIN where expected"

mkdir -p "$INSTALL_DIR"
install -m 0755 "$extracted" "$INSTALL_DIR/$BIN"
say "installed $INSTALL_DIR/$BIN"

# A desktop entry, if the archive carried one and there is somewhere to put it.
desktop="$tmp/${BIN}-${GLIMPSE_VERSION}-linux-x86_64/${BIN}.desktop"
apps="${XDG_DATA_HOME:-$HOME/.local/share}/applications"
if [ -f "$desktop" ]; then
  mkdir -p "$apps"
  install -m 0644 "$desktop" "$apps/${BIN}.desktop"
  say "installed $apps/${BIN}.desktop"
fi

# ------------------------------------------------------- runtime complaints --
# Better to say what is missing now than to let it fail confusingly at launch.
missing=""
[ "${XDG_SESSION_TYPE:-}" = "wayland" ] && missing="$missing\n  - you are on Wayland; Glimpse needs an X11 session and will refuse to start"
command -v ffmpeg >/dev/null 2>&1 || missing="$missing\n  - ffmpeg is not installed; recording will not work without it"
pkg-config --exists gtk4 2>/dev/null || ldconfig -p 2>/dev/null | grep -q libgtk-4 \
  || missing="$missing\n  - GTK 4 does not appear to be installed"
case ":$PATH:" in
  *":$INSTALL_DIR:"*) ;;
  *) missing="$missing\n  - $INSTALL_DIR is not on your PATH" ;;
esac

if [ -n "$missing" ]; then
  printf '\n\033[33mbefore it will run:\033[0m'
  printf "$missing\n"
fi

printf '\n%s %s installed. Run: %s\n' "$BIN" "$GLIMPSE_VERSION" "$BIN"
