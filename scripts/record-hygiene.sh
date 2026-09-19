#!/usr/bin/env bash
#
# Press Record for real, and check that the attempt leaves nothing behind.
#
#   scripts/record-hygiene.sh
#
# WHY THIS EXISTS. It was written when a macOS CI runner was believed unable to
# record — the capture probe said so on every build, reading a device index that
# does not exist on a runner (#56). So it asserts only what is true **whichever
# way the attempt goes**:
#
#   * the app exits rather than hanging
#   * no ffmpeg survives it
#   * no workspace is left in the temp directory
#
# Three, not four. The state the session ends in is reported and not asserted:
# see the note further down about the check that had to be walked back.
#
# They hold whether the recording succeeds or is refused — a machine without
# the permission takes the refusal path, which no journey reaches. A check whose
# verdict depended on which machine it ran on would be worth very little, so this
# one does not have one. That still earns its place now the journeys run in CI:
# they check each journey did what it says, and this checks the process table
# and the temp directory after the app is gone.
#
# WHY THESE. They are issue #45, itemised. An orphaned ffmpeg held the capture
# device and broke the NEXT recording, and workspaces accumulated 28 deep. Both
# are lines below, and neither was covered by anything on macOS.
#
# It found one the first time CI ran it: `app.quit()` does not emit
# `close-request`, so quitting mid-recording never told the session to stand
# down and ffmpeg outlived the process. Linux hides that behind
# `die_with_parent`; macOS has no analogue.
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

# REPORTED, NOT ASSERTED, and it was asserted in the first version of this file.
#
# The state the journey prints is the state three seconds after Stop, and how
# long an encode takes is a property of the machine: `Completed` here,
# `Encoding` on a loaded runner, `Failed` where there is no capture device. Then
# the process exits and everything below is checked. Demanding a terminal state
# at that instant is precisely the machine-dependent verdict this check was
# written to avoid having — the mistake is easy to make twice.
#
# Nothing is lost. "Stuck in Stopping" was a symptom of #45, and a session stuck
# anywhere has a live ffmpeg under it, which the next check does assert on.
echo "report: $(grep -oE 'final state: .*' "$log" | head -1 || echo 'no final state logged')"

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
