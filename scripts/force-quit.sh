#!/usr/bin/env bash
#
# Force-quit Glimpse mid-recording. Nothing may survive it.
#
#   scripts/force-quit.sh
#
# WHY THIS EXISTS. `SIGKILL` is the one signal no process can handle for itself,
# so it is the one exit path the application cannot cover from the inside. Every
# other way out is handled: the window's close request, the application's
# `shutdown` signal — Cmd-Q, the menu, a journey ending — and `SIGINT`/`SIGTERM`.
#
# Linux has the kernel do it: `PR_SET_PDEATHSIG` takes ffmpeg down with its
# parent whatever kills the parent. macOS has no equivalent, and what that cost
# was measured before anything was built ([ADR 0019](../docs/adr/0019-a-recording-outlives-a-killed-glimpse.md)):
# the orphan kept writing at about 5 MB/s — 18 GB/hour — holding the screen
# capture device until the disk filled or Glimpse was started again.
#
# WHAT IT CHECKS. That the guard process actually guards, through the real
# binary, with a real recording, killed the real way. `glimpse-macos`'s unit
# tests drive `reap::watch` directly with two `sleep` processes, which is where
# the kqueue logic is pinned down; they cannot show that Glimpse spawns the guard
# at all, points it at the right pids, or that the guard survives the parent it
# is watching. This is the end-to-end half.
#
# WHAT WOULD HAPPEN IF THE THING THIS TESTS WERE BROKEN. A user who force-quits
# an app that looks stuck — which is exactly the user who force-quits — fills
# their disk at 18 GB/hour, and finds out when something else fails. The evidence
# reads as "my disk is full", not "Glimpse leaked a process".

set -euo pipefail
cd "$(dirname "${BASH_SOURCE[0]}")/.."

TMP="${TMPDIR:-/tmp}"

capture_pids() {
  pgrep -f 'ffmpeg.*(avfoundation|x11grab)' 2>/dev/null || true
}

# An inherited orphan would be reported as this run's, and would also hold the
# capture device and make the recording fail for a reason that is not the code's.
if [ -n "$(capture_pids)" ]; then
  echo "force-quit: a capture is already running; clear it first" >&2
  pgrep -fl ffmpeg | sed 's/^/  /' >&2
  exit 1
fi

BIN="${GLIMPSE_BIN:-}"
if [ -z "$BIN" ]; then
  cargo build --locked -q
  BIN=./target/debug/glimpse
fi
[ -x "$BIN" ] || { echo "force-quit: no binary at $BIN" >&2; exit 1; }

log=$(mktemp -t fq-log.XXXXXX)
GLIMPSE_SELFTEST=record "$BIN" >"$log" 2>&1 &
app=$!

# Wait for a recording to actually be running. Killing before ffmpeg exists
# would pass while testing nothing, which is the failure mode of every fixed
# sleep in this repo's history.
for _ in $(seq 1 300); do
  grep -q 'state: Recording' "$log" && [ -n "$(capture_pids)" ] && break
  kill -0 "$app" 2>/dev/null || break
  sleep 0.1
done

before=$(capture_pids)
if [ -z "$before" ]; then
  echo "force-quit: no capture ever started, so nothing was tested" >&2
  sed 's/^/  /' "$log" >&2
  kill -9 "$app" 2>/dev/null || true
  rm -f "$log"
  exit 1
fi
echo "force-quit: recording under pid(s): $(echo "$before" | tr '\n' ' ')"

# The real thing. Not SIGTERM, which the app handles; not `app.quit()`, which it
# also handles now. This is Force Quit.
kill -9 "$app"
wait "$app" 2>/dev/null || true
echo "force-quit: SIGKILLed Glimpse (pid $app)"

# Bounded wait rather than a flat sleep: the guard's latency is the kernel's
# notification latency plus a process wake-up, and hardcoding a guess for that
# is how a check ends up measuring the machine instead of the code.
gone=0
for _ in $(seq 1 100); do
  [ -z "$(capture_pids)" ] && { gone=1; break; }
  sleep 0.1
done

fail=0
if [ "$gone" -eq 1 ]; then
  echo "ok: the capture died with the application"
else
  echo "FAIL: a capture outlived a force-quit." >&2
  pgrep -fl ffmpeg | sed 's/^/      /' >&2
  echo "      It holds the capture device and writes ~18 GB/hour until the" >&2
  echo "      disk fills or Glimpse is started again (ADR 0019)." >&2
  pkill -9 -f 'ffmpeg.*(avfoundation|x11grab)' 2>/dev/null || true
  fail=1
fi

# The guard must not become the thing it was built to prevent.
if pgrep -f 'glimpse.*--reap' >/dev/null 2>&1; then
  echo "FAIL: a --reap guard outlived the recording it was guarding." >&2
  pgrep -fl 'glimpse.*--reap' | sed 's/^/      /' >&2
  pkill -9 -f 'glimpse.*--reap' 2>/dev/null || true
  fail=1
else
  echo "ok: no guard left behind"
fi

# The workspace is expected to survive: nothing finalises it, and the start-up
# sweep is what collects it. Reported rather than asserted, because deleting it
# is not this path's job — and left on disk, because the sweep is the next thing
# that should be seen doing its work.
left=$(find "$TMP" -maxdepth 1 -type d -name 'glimpse-*' 2>/dev/null | wc -l | tr -d ' ')
echo "report: $left workspace(s) left for the start-up sweep"

rm -f "$log"
if [ "$fail" -eq 0 ]; then
  echo
  echo "PASS: nothing survived the force-quit."
fi
exit "$fail"
