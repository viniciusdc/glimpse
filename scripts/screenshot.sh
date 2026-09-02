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
#     passed — including the expanded-crop test written to catch chrome bleeding
#     into a grab, which looks for frame colour and cannot see a gradient.
#
# Neither was findable by asserting harder. Both were obvious in a picture.
#
# This is the X11 half. It runs the real application on a private X server and
# grabs the WHOLE screen — whole, not a crop, because the layout bug above
# survived being looked at three times and every look was a crop chosen by
# whoever was debugging. A crop cannot show you that the thing you framed is in
# the wrong place.
#
# macOS has no equivalent because it has no Xvfb. See AGENTS.md.
#
# NOT a comparison and NOT a pixel diff. The two platforms are different window
# models by design (ADR 0006, ADR 0015, ADR 0016), so a diff would be red forever
# and mean nothing when it was not. This produces evidence for a person.
#
# IT DOES, HOWEVER, CHECK ITSELF. The first version of this script shipped a
# 1920x1080 black rectangle and reported success: it ran `cargo run --release`,
# which in CI starts a fresh build, and the liveness check confirmed *cargo* was
# alive rather than the app. A tool whose job is to make failures visible must
# not have an invisible failure of its own, so this now builds before it times
# anything, waits for a real mapped window, and refuses an image with nothing in
# it.

set -euo pipefail
cd "$(dirname "${BASH_SOURCE[0]}")/.."

: "${HEADLESS_SIZE:=1920x1080x24}"
# How long to wait for a window to be mapped. Not a settle time — the loop below
# polls for the window and stops as soon as it exists.
: "${WINDOW_TIMEOUT:=45}"

# ---------------------------------------------------------------- inner half --
# Re-exec of this same script inside the headless display: one file, and no
# temporary script written at run time.
if [[ "${1:-}" == "--inside" ]]; then
  out="$2"; size="$3"; timeout="$4"

  ./target/release/glimpse &
  app=$!
  trap 'kill $app 2>/dev/null' EXIT

  # Poll for a mapped top-level window rather than sleeping a guessed interval.
  # `xwininfo` comes from x11-utils, which `headless.sh` already requires for
  # `xdpyinfo`, so this adds no dependency.
  mapped=0
  for _ in $(seq 1 "$timeout"); do
    if ! kill -0 "$app" 2>/dev/null; then
      echo "screenshot: the app exited before a window appeared" >&2
      exit 1
    fi
    if xwininfo -root -children 2>/dev/null | grep -qiE '"(glimpse|Glimpse)"'; then
      mapped=1
      break
    fi
    sleep 1
  done
  if (( ! mapped )); then
    echo "screenshot: no glimpse window after ${timeout}s" >&2
    xwininfo -root -children 2>/dev/null | head -20 >&2
    exit 1
  fi

  # Mapped is not the same as painted: GTK maps first and draws on the next
  # frame, and a grab between the two is the black rectangle this script exists
  # not to produce.
  sleep 2

  ffmpeg -hide_banner -loglevel error -f x11grab -video_size "$size" -i "$DISPLAY" \
    -frames:v 1 -c:v png -update 1 -y "$out"
  exit $?
fi

# ---------------------------------------------------------------- outer half --
OUT="${1:-glimpse-linux.png}"
size_wh="${HEADLESS_SIZE%x*}"     # 1920x1080x24 -> 1920x1080

for tool in ffmpeg xwininfo; do
  command -v "$tool" >/dev/null 2>&1 || {
    echo "screenshot: $tool is required" >&2
    exit 1
  }
done

# Built here, outside the timing window. `cargo run` inside it was the original
# bug: the build counted against the wait, and the grab landed mid-compile.
echo "screenshot: building the binary first, so the wait measures startup only"
"${CARGO:-cargo}" build --release -q

scripts/headless.sh "$0" --inside "$OUT" "$size_wh" "$WINDOW_TIMEOUT"

[[ -s "$OUT" ]] || { echo "screenshot: produced no image" >&2; exit 1; }

# The self-check. An all-black frame is what a mistimed grab looks like, and it
# is indistinguishable from a working screenshot by file size alone — the black
# one was 6 KB and looked plausible.
#
# Counting distinct colours rather than comparing to a reference: a window with
# text and buttons in it has hundreds, an empty root window has one or two.
colours=$(ffmpeg -v error -i "$OUT" -f rawvideo -pix_fmt rgb24 - 2>/dev/null \
  | xxd -p -c3 | sort -u | wc -l | tr -d ' ')
echo "screenshot: wrote $OUT ($(wc -c < "$OUT" | tr -d ' ') bytes, $size_wh, $colours distinct colours)"

if (( colours < 20 )); then
  echo "screenshot: only $colours distinct colours — this is a blank display, not a UI" >&2
  exit 1
fi
