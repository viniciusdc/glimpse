#!/usr/bin/env bash
#
# Does a click aimed at the hole actually reach the window behind it?
#
#   scripts/headless.sh scripts/clickthrough.sh      # what `make clickthrough` runs
#
# WHY THIS EXISTS. Everything else checks the input region: `selftest.sh` reads
# the shape back from the X server and checks its semantics, which is a real
# check and catches a hole punched in the wrong place. What it cannot see is
# whether the shape translates into an event actually being delivered somewhere
# else. The whole product promise is that you can work in the application you are
# recording; nothing tested that a click gets there.
#
# So: a board behind, the framing window on top, one click at the middle of the
# hole and one at the header, and the board's own account of what it received.
#
# BOTH DIRECTIONS, and the second is not optional. A board that stopped
# responding — because it never mapped, because the click missed the screen,
# because xdotool silently did nothing — would report "the hole is
# click-through" for every window ever made. So the header click must NOT arrive,
# and a bare-board control click MUST.
#
# WHY THIS IS THE LINUX HALF. Synthesising a click on macOS needs Accessibility
# permission, which a developer machine does not have by default and a CI runner
# never will; `glimpse-macos/examples/click_through_blackboard.rs` is the manual
# equivalent, and it refuses to conclude anything when it cannot post events.
# Under Xvfb this needs no permission at all, so the automated half of the idea
# lives here.
#
# WHAT WOULD HAPPEN IF THE THING THIS TESTS WERE BROKEN. Stop punching the input
# region and every other check still passes: the geometry is unchanged, the grab
# is unchanged, the shape readback would fail but only if it is still asked. This
# is the one that notices the product stopped working.

set -euo pipefail
cd "$(dirname "${BASH_SOURCE[0]}")/.."

# Refuse rather than proceed, for the reason headless.sh spells out: a missing
# tool makes a check pass by not running, and that is worse than not having it.
missing=()
command -v xdotool >/dev/null 2>&1 || missing+=(xdotool)
if [ ${#missing[@]} -ne 0 ]; then
  echo "clickthrough: missing ${missing[*]}" >&2
  echo "  Debian/Ubuntu: sudo apt-get install xdotool" >&2
  exit 1
fi
if [ -z "${DISPLAY:-}" ]; then
  echo "clickthrough: no DISPLAY. Run it under scripts/headless.sh." >&2
  exit 1
fi

board_log=$(mktemp -t glimpse-board.XXXXXX)
app_log=$(mktemp -t glimpse-app.XXXXXX)
board_pid=""
app_pid=""
cleanup() {
  [ -n "$board_pid" ] && kill "$board_pid" 2>/dev/null || true
  [ -n "$app_pid" ] && kill "$app_pid" 2>/dev/null || true
  rm -f "$board_log" "$app_log"
}
trap cleanup EXIT

size=$(xdpyinfo | awk '/dimensions:/ {print $2}')
sw=${size%x*}
sh=${size#*x}
echo "clickthrough: screen ${sw}x${sh}"

cargo build --locked -q -p glimpse-x11 --example click_board
cargo build --locked -q

./target/debug/examples/click_board "$sw" "$sh" >"$board_log" 2>&1 &
board_pid=$!
for _ in $(seq 1 100); do
  grep -q 'BOARD-READY' "$board_log" && break
  sleep 0.1
done
if ! grep -q 'BOARD-READY' "$board_log"; then
  echo "clickthrough: the board never mapped; nothing was tested" >&2
  cat "$board_log" >&2
  exit 1
fi

# CONTROL ONE: the board takes clicks at all. Everything below is read against
# this, and without it a board that never responds looks like perfect
# click-through.
xdotool mousemove $((sw / 4)) $((sh - 40)) click 1
sleep 0.5
if ! grep -q 'BOARD-CLICK' "$board_log"; then
  echo "clickthrough: the board did not register a direct click." >&2
  echo "  Nothing below would mean anything, so this stops here." >&2
  cat "$board_log" >&2
  exit 1
fi
echo "clickthrough: control — the bare board takes clicks"
before=$(grep -c 'BOARD-CLICK' "$board_log")

# The app, holding after its self-test so the window stays up to be clicked, and
# reporting the rectangle to aim at. `capture rect` is the same line
# `selftest.sh` depends on.
GLIMPSE_SELFTEST=1 GLIMPSE_SELFTEST_HOLD=1 ./target/debug/glimpse >"$app_log" 2>&1 &
app_pid=$!
for _ in $(seq 1 200); do
  grep -q 'capture rect' "$app_log" && break
  sleep 0.1
done
if ! grep -q 'capture rect' "$app_log"; then
  echo "clickthrough: the app never reported a capture rect" >&2
  cat "$app_log" >&2
  exit 1
fi

read -r hw hh hx hy < <(
  sed -n 's/.*capture rect *: *\([0-9]*\)x\([0-9]*\) at \([0-9]*\),\([0-9]*\).*/\1 \2 \3 \4/p' "$app_log" | head -1
)
echo "clickthrough: hole ${hw}x${hh} at ${hx},${hy}"
if [ -z "${hy:-}" ] || [ "$hw" -le 0 ] || [ "$hh" -le 0 ]; then
  echo "clickthrough: could not read the hole rectangle" >&2
  exit 1
fi

hole_x=$((hx + hw / 2))
hole_y=$((hy + hh / 2))
# Above the hole is the frame's border, the rule, then the header. 25px clears
# the first two on any theme and stays well inside the 44px header.
head_x=$hole_x
head_y=$((hy - 25))
if [ "$head_y" -lt 0 ]; then
  echo "clickthrough: the window is too near the top edge to aim at its header" >&2
  exit 1
fi

# The measurement. Through the hole first.
xdotool mousemove "$hole_x" "$hole_y" click 1
sleep 0.6
through=$(( $(grep -c 'BOARD-CLICK' "$board_log") - before ))

# CONTROL TWO: the chrome must keep the click. If this also arrives, the window
# is not taking clicks anywhere and "the hole is click-through" is meaningless.
mid=$(grep -c 'BOARD-CLICK' "$board_log")
xdotool mousemove "$head_x" "$head_y" click 1
sleep 0.6
leaked=$(( $(grep -c 'BOARD-CLICK' "$board_log") - mid ))

echo
echo "  click at the hole   (${hole_x},${hole_y}): reached the board ${through} time(s)  [want 1]"
echo "  click at the header (${head_x},${head_y}): reached the board ${leaked} time(s)  [want 0]"
echo

fail=0
if [ "$through" -lt 1 ]; then
  echo "FAIL: a click in the hole did not reach the window behind it." >&2
  echo "      The input region is not being punched, or not where the hole is." >&2
  fail=1
fi
if [ "$leaked" -ne 0 ]; then
  echo "FAIL: a click on the header passed through to the board." >&2
  echo "      The window is taking no clicks anywhere, so the chrome is dead." >&2
  fail=1
fi
[ "$fail" -eq 0 ] && echo "PASS: the hole passes clicks and the chrome keeps them."
exit "$fail"
