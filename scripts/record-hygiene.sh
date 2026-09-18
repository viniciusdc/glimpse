#!/usr/bin/env bash
#
# Press Record for real, and check that the attempt leaves nothing behind.
#
#   scripts/record-hygiene.sh
#
# WHY THIS EXISTS. The five user journeys cannot run on a macOS CI runner: they
# record, and a runner has no screen device — measured, ffmpeg exits 251 opening
# the avfoundation input and the device list comes back empty. So the wiring from
# the button through the session machine to the worker is exercised on Linux and
# asserted on macOS.
#
# This closes most of that gap without inventing a second capture path, because
# what it asserts is true **whichever way the attempt goes**:
#
#   * the app reaches a terminal state rather than sitting in Recording forever
#   * it exits rather than hanging
#   * no ffmpeg survives it
#   * no workspace is left in the temp directory
#
# On a developer machine the recording succeeds and those hold. On a runner it
# fails for want of a device and they hold too. A check whose verdict depended on
# which machine it ran on would be worth very little, so this one does not have
# one.
#
# WHY THESE FOUR. They are issue #45, itemised. An orphaned ffmpeg held the
# capture device and broke the NEXT recording; the UI sat in Stopping; workspaces
# accumulated 28 deep. Every symptom of that bug is one of the lines below, and
# none of them was covered by anything on macOS.
#
# WHAT IT DOES NOT CHECK. That a recording is any good — that is the journeys'
# job, and `scripts/journey-verdict.sh` holds their verdicts. This is about what
# is left on the machine afterwards.

set -euo pipefail
cd "$(dirname "${BASH_SOURCE[0]}")/.."

TMP="${TMPDIR:-/tmp}"

# A stray capture from an earlier run would be counted as this run's leak, and
# would also make the recording fail for a reason that is not the code's.
if pgrep -f 'ffmpeg.*avfoundation' >/dev/null 2>&1 || pgrep -f 'ffmpeg.*x11grab' >/dev/null 2>&1; then
  echo "record-hygiene: an ffmpeg is already capturing; clear it first" >&2
  pgrep -fl 'ffmpeg' | sed 's/^/  /' >&2
  exit 1
fi

# Which workspaces, not how many. The app sweeps stale ones from earlier runs at
# start-up, so a count can hold steady across a run that leaked: one swept, one
# leaked, no change. That is precisely the case worth catching, and the first
# version of this script could not see it.
#
# Directories only, and the scratch files below are deliberately not named
# `glimpse-*` — a log file counted as a workspace would mask a real leak by
# taking up its slot.
workspaces() { find "$TMP" -maxdepth 1 -type d -name 'glimpse-*' 2>/dev/null | sort; }

before=$(mktemp -t hygiene-before.XXXXXX)
workspaces >"$before"
echo "record-hygiene: $(wc -l <"$before" | tr -d ' ') glimpse workspace(s) in $TMP before"

# CI has already built a release binary by the time it gets here, and building a
# second one to run this would double the slowest step in the job.
BIN="${GLIMPSE_BIN:-}"
if [ -z "$BIN" ]; then
  cargo build --locked -q
  BIN=./target/debug/glimpse
fi
[ -x "$BIN" ] || { echo "record-hygiene: no binary at $BIN" >&2; exit 1; }

log=$(mktemp -t hygiene-log.XXXXXX)
GLIMPSE_SELFTEST=record "$BIN" >"$log" 2>&1 &
pid=$!

# No timeout(1) on macOS, so the watchdog lives here. A journey that hangs has to
# end as a failure rather than as a job that never finishes — and hanging is one
# of the four things being checked for, not an accident of the harness.
waited=0
while kill -0 "$pid" 2>/dev/null && [ "$waited" -lt 60 ]; do
  sleep 1
  waited=$((waited + 1))
done

hung=0
if kill -0 "$pid" 2>/dev/null; then
  hung=1
  kill -9 "$pid" 2>/dev/null || true
fi
wait "$pid" 2>/dev/null || true
sleep 1

echo
sed 's/^/  /' "$log"
echo

fail=0

if [ "$hung" -ne 0 ]; then
  echo "FAIL: the app did not exit within 60s." >&2
  echo "      A recording that cannot start must still let the app finish." >&2
  fail=1
fi

# A terminal state, not a particular one. `Completed` where capture works,
# `Failed` where it does not; either is the machine arriving somewhere. Sitting
# in Recording or Stopping is the bug.
if grep -qE '\[smoke\] final state: (Completed|Failed|Cancelled)' "$log"; then
  echo "ok: reached a terminal state — $(grep -oE 'final state: [A-Za-z]+' "$log" | head -1)"
else
  echo "FAIL: the session never reached a terminal state." >&2
  echo "      Recording or Stopping at the end means the UI is stuck, which is" >&2
  echo "      what issue #45 looked like from the outside." >&2
  fail=1
fi

if pgrep -f 'ffmpeg.*avfoundation' >/dev/null 2>&1 || pgrep -f 'ffmpeg.*x11grab' >/dev/null 2>&1; then
  echo "FAIL: an ffmpeg survived the attempt." >&2
  pgrep -fl 'ffmpeg' | sed 's/^/      /' >&2
  echo "      It holds the capture device and will break the next recording." >&2
  pkill -9 -f 'ffmpeg.*avfoundation' 2>/dev/null || true
  pkill -9 -f 'ffmpeg.*x11grab' 2>/dev/null || true
  fail=1
else
  echo "ok: no capture survived it"
fi

after=$(mktemp -t hygiene-after.XXXXXX)
workspaces >"$after"
leaked=$(comm -13 "$before" "$after")
if [ -n "$leaked" ]; then
  echo "FAIL: a workspace outlived the attempt." >&2
  printf '%s\n' "$leaked" | sed 's/^/      /' >&2
  echo "      These accumulate — #45 left 28 of them." >&2
  fail=1
else
  echo "ok: no workspace left behind"
fi

rm -f "$log" "$before" "$after"
if [ "$fail" -eq 0 ]; then
  echo
  echo "PASS: the attempt left nothing behind."
fi
exit "$fail"
