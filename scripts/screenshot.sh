#!/usr/bin/env bash
#
# Photograph the framing window, off-screen, and write a PNG.
#
#   scripts/screenshot.sh [out.png]
#
# WHY THIS EXISTS. Every check this project has asserts on state, geometry or
# output. None of them looks at the result, and a whole class of bug is only
# visible that way:
#
#   * the macOS status bar sat on the wrong side of the frame through three
#     merged pull requests, with every check green on all three;
#   * the chrome's drop shadow was baked into the top 40 pixels of every macOS
#     recording, while the capture rect was correct and every geometry check
#     passed — including the expanded-crop test written to catch exactly that,
#     which looks for frame colour and cannot see a gradient.
#
# Neither was findable by asserting harder. Both were obvious in a picture.
#
# This is the X11 half. It runs the real application on a private X server and
# grabs the WHOLE screen, so the output is what a user would see rather than a
# crop chosen by whoever was debugging — which is how the layout bug above
# survived being looked at three times.
#
# macOS has no equivalent because it has no Xvfb: every macOS window check runs
# on the developer's real screen. See AGENTS.md.
#
# NOT a comparison, and deliberately not an assertion. There is no reference
# image to diff against — the two platforms are different window models by
# design (ADR 0015, ADR 0016), so a pixel diff would be red forever and mean
# nothing. This produces evidence for a person to look at.

set -euo pipefail
cd "$(dirname "${BASH_SOURCE[0]}")/.."

: "${HEADLESS_SIZE:=1920x1080x24}"
# Seconds to let GTK map, style and lay out the window before grabbing.
#
# A sleep, not a wait-for-window. `xdotool` is optional in this project, so there
# is no dependency here that can see whether a window has actually appeared, and
# claiming to wait for one would be the kind of check that reports on something
# it never looked at. What the loop below DOES verify is that the app is still
# alive — a grab of an empty display after a crash is the failure worth ruling
# out, and that one is checkable.
: "${SETTLE_SECONDS:=6}"

# ---------------------------------------------------------------- inner half --
# Re-exec of this same script, inside the headless display. One file rather than
# two, and no temporary script written at run time.
if [[ "${1:-}" == "--inside" ]]; then
  out="$2"; size="$3"; settle="$4"

  "${CARGO:-cargo}" run --release -q &
  app=$!
  trap 'kill $app 2>/dev/null' EXIT

  for _ in $(seq 1 "$settle"); do
    sleep 1
    if ! kill -0 "$app" 2>/dev/null; then
      echo "screenshot: the app exited before it could be grabbed" >&2
      exit 1
    fi
  done

  ffmpeg -hide_banner -loglevel error -f x11grab -video_size "$size" -i "$DISPLAY" \
    -frames:v 1 -c:v png -update 1 -y "$out"
  exit $?
fi

# ---------------------------------------------------------------- outer half --
OUT="${1:-glimpse-linux.png}"
size_wh="${HEADLESS_SIZE%x*}"     # 1920x1080x24 -> 1920x1080

command -v ffmpeg >/dev/null 2>&1 || {
  echo "screenshot: ffmpeg is required to grab the display" >&2
  exit 1
}

scripts/headless.sh "$0" --inside "$OUT" "$size_wh" "$SETTLE_SECONDS"

if [[ -s "$OUT" ]]; then
  echo "screenshot: wrote $OUT ($(wc -c < "$OUT") bytes, $size_wh)"
else
  echo "screenshot: produced no image" >&2
  exit 1
fi
