#!/usr/bin/env bash
#
# Photograph a region of the running macOS app, then always close it.
#
#   scripts/shot-macos.sh OUT.png [LEFT_FRAC RIGHT_FRAC]
#   scripts/shot-macos.sh OUT.png --full        # the whole screen, uncropped
#
# THE SCALE IS READ, NOT ASSUMED. The app reports the capture rect in device
# pixels and `screencapture` takes points, so something has to convert. This
# hardcoded `2.0` — right on every Retina Mac, wrong on a 1x display and on a CI
# runner, where it would photograph the wrong region and the picture would read
# as a UI bug rather than a maths bug. Glimpse now prints `backing scale` beside
# the rect, from the same NSScreen the rect came from, and this reads that.
#
# --full stays, because a runner's desktop has nothing worth cropping away and a
# whole-screen grab cannot be wrong about geometry at all.
#
# WHY THIS EXISTS. Looking at the macOS UI means launching the real app on a
# real desktop — there is no Xvfb on macOS, so every window check runs on the
# screen somebody is using (AGENTS.md). Launching and capturing as separate
# steps leaves the app up whenever the capture step fails, or whenever whoever
# is driving forgets, and the person whose screen it is ends up closing windows
# by hand. The trap below is the whole point of the file: the app is killed on
# every exit path, including a failed capture and a Ctrl-C.
#
# It locates the window from the app's own `capture rect` line rather than from
# a hardcoded position, because GTK asks the window manager for a position and
# does not always get the one it asked for.
#
# The PNG is a picture of your screen. Never attach it to a pull request, an
# issue or a commit.
set -euo pipefail

OUT="${1:?usage: shot-macos.sh OUT.png [LEFT_FRAC RIGHT_FRAC]}"
LEFT_FRAC="${2:-0}"
RIGHT_FRAC="${3:-1}"
BIN="target/release/glimpse"
LOG="$(mktemp -t glimpse-shot)"

[ -x "$BIN" ] || { echo "build it first: cargo build --release" >&2; exit 1; }

"$BIN" >"$LOG" 2>&1 &
PID=$!
# Every exit path. A capture that fails must not leave a window on the screen.
trap 'kill "$PID" 2>/dev/null || true; wait "$PID" 2>/dev/null || true' EXIT

for _ in $(seq 1 40); do
  # Both lines, because the crop needs both and they are printed one after the
  # other: waiting only for the rect can win the race for it and lose the scale.
  grep -q 'capture rect' "$LOG" && grep -q 'backing scale' "$LOG" && break
  kill -0 "$PID" 2>/dev/null || break
  sleep 0.5
done

if ! grep -q 'capture rect' "$LOG"; then
  echo "the app never reported a capture rect:" >&2
  cat "$LOG" >&2
  exit 1
fi

if [ "$LEFT_FRAC" = "--full" ]; then
  screencapture -x "$OUT"
  echo "$OUT: the whole screen"
  exit 0
fi

python3 - "$LOG" "$OUT" "$LEFT_FRAC" "$RIGHT_FRAC" <<'PY'
import re, subprocess, sys
log, out, lf, rf = sys.argv[1], sys.argv[2], float(sys.argv[3]), float(sys.argv[4])
text = open(log).read()
m = re.search(r'capture rect (\d+)x(\d+) at (\d+),(\d+)', text)
w, h, x, y = (int(v) for v in m.groups())

# Device pixels to points, at the scale THIS screen reported.
#
# This was `s = 2.0`. Correct on every Retina Mac, so it never failed for anyone
# who ran it — and silently wrong on a 1x display or a CI runner, where it would
# crop a region twice the size of the right one and the photograph would look
# like a broken UI rather than broken arithmetic.
#
# Refused rather than defaulted when the line is missing: falling back to 2.0
# would restore exactly the bug, and would do it on the machines where it is
# wrong, which are the ones least likely to notice.
sm = re.search(r'backing scale ([0-9]+(?:\.[0-9]+)?)', text)
if not sm:
    sys.exit(
        "the app did not report a backing scale, so device pixels cannot be "
        "converted to points. Rebuild: this needs a Glimpse that prints it."
    )
s = float(sm.group(1))
hx, hy, hw, hh = x / s, y / s, w / s, h / s
# The window is the hole plus its 3pt border, the header above and the status
# bar below. Generous margins so a shadow or a rounded corner is not clipped.
left, top = hx - 3 - 12, hy - 49 - 12
width, height = hw + 6 + 24, hh + 49 + 40 + 24
left += width * lf
width *= (rf - lf)
subprocess.run(['screencapture', '-x', f'-R{left},{top},{width},{height}', out], check=True)
print(f'{out}: {width:.0f}x{height:.0f} points at {left:.0f},{top:.0f} (backing scale {s:g})')
PY
