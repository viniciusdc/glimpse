#!/usr/bin/env bash
#
# Run a command against a private X server, so development never touches the
# developer's screen, pointer or window stack.
#
#   scripts/headless.sh cargo run
#   scripts/headless.sh env GLIMPSE_SELFTEST=record cargo run
#
# WHAT THIS CANNOT CHECK. Xvfb has no window manager and no compositor:
#
#   * Moving and resizing go through the window manager — `begin_resize` sends
#     _NET_WM_MOVERESIZE and there is nobody to receive it. Resize must be
#     verified on a real session.
#   * Transparency is not composited, so the capture hole shows the root window
#     rather than what is behind Glimpse. Geometry and the input region are still
#     verified exactly, because both are checked against the X server, not by eye.
#
# Everything else — geometry, the input-region hole, recording, encoding, cleanup
# — behaves the same as on a real display.

set -euo pipefail
cd "$(dirname "${BASH_SOURCE[0]}")/.."

: "${HEADLESS_DISPLAY:=:99}"
: "${HEADLESS_SIZE:=1920x1080x24}"

if [[ $# -eq 0 ]]; then
  echo "usage: scripts/headless.sh <command...>" >&2
  exit 2
fi

# Both tools have to be present, and `xdpyinfo` for a sharper reason than the
# obvious one.
#
# The clobber check below is an `if` on xdpyinfo's exit status. A missing
# xdpyinfo exits 127, so the branch is not taken and the guard silently passes —
# it would then start an X server on top of a live display, having reported that
# the display was free. A safety check that cannot run must refuse rather than
# shrug; the same failure shape as a liveness probe that always answers "alive".
#
# They are separate packages on Debian (`xvfb` and `x11-utils`), so having one
# without the other is an ordinary state rather than a corner case. And without
# this, a missing Xvfb surfaced six seconds later as "Xvfb did not come up",
# which describes a server that failed to start rather than one that was never
# installed.
missing=()
command -v Xvfb     >/dev/null 2>&1 || missing+=(Xvfb)
command -v xdpyinfo >/dev/null 2>&1 || missing+=(xdpyinfo)
if (( ${#missing[@]} )); then
  echo "scripts/headless.sh needs, and cannot find: ${missing[*]}" >&2
  echo "  Debian/Ubuntu: sudo apt-get install xvfb x11-utils" >&2
  echo "Without these it cannot run Glimpse off-screen, and cannot verify that" >&2
  echo "$HEADLESS_DISPLAY is free before starting a server on it." >&2
  exit 1
fi

# Refuse to clobber a live display — :99 is conventional for this, but check.
if xdpyinfo -display "$HEADLESS_DISPLAY" >/dev/null 2>&1; then
  echo "$HEADLESS_DISPLAY is already in use; set HEADLESS_DISPLAY to something else" >&2
  exit 1
fi

Xvfb "$HEADLESS_DISPLAY" -screen 0 "$HEADLESS_SIZE" -nolisten tcp >/dev/null 2>&1 &
xvfb_pid=$!
cleanup() {
  kill "$xvfb_pid" 2>/dev/null || true
  wait "$xvfb_pid" 2>/dev/null || true
}
trap cleanup EXIT INT TERM

# Wait for it to accept connections rather than sleeping a guessed interval.
for _ in $(seq 1 50); do
  xdpyinfo -display "$HEADLESS_DISPLAY" >/dev/null 2>&1 && break
  sleep 0.1
done
if ! xdpyinfo -display "$HEADLESS_DISPLAY" >/dev/null 2>&1; then
  echo "Xvfb did not come up on $HEADLESS_DISPLAY" >&2
  exit 1
fi

echo "headless: $HEADLESS_DISPLAY ($HEADLESS_SIZE)"
DISPLAY="$HEADLESS_DISPLAY" "$@"
