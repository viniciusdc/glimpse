#!/usr/bin/env bash
#
# Every user journey, on macOS.
#
#   scripts/journeys-macos.sh              # all of them
#   scripts/journeys-macos.sh record       # just one
#
# WHY THIS EXISTS. `make journeys` drives all five through `smoke.sh`, which runs
# them under `headless.sh` — Xvfb, X11 only. macOS therefore ran **no journeys at
# all**: its CI job checks that the binary comes up and reports a capture rect,
# and stops there. Everything past that point — Record actually recording, Stop
# actually stopping, cancel preserving the capture, retry re-encoding it — was
# verified on one platform and asserted on the other.
#
# That gap has already cost something. Issue #45 was a recording that could not
# be stopped, and the first thing that reproduced it was driving a journey by
# hand.
#
# WHY IT CANNOT RUN IN CI. avfoundation needs Screen Recording permission, which
# is granted to an application by a human at a system dialog. A GitHub runner has
# no one to grant it, so every journey would fail for a reason that is not a
# product bug — which is worse than not running them, because a red check nobody
# can act on is a check people learn to ignore.
#
# WHY IT USES YOUR SCREEN. There is no Xvfb on macOS. The journeys put a window
# up, record a region of your desktop and write a file. AGENTS.md says not to run
# this on somebody else's machine, and it means it.
#
# WHAT WOULD HAPPEN IF THE THING THIS TESTS WERE BROKEN. Nothing, until a user
# found it — which is exactly how #45 arrived.

set -euo pipefail
cd "$(dirname "${BASH_SOURCE[0]}")/.."

if [ "$(uname -s)" != "Darwin" ]; then
  echo "journeys-macos: this is the macOS half. Use 'make journeys' on Linux." >&2
  exit 1
fi

command -v ffmpeg >/dev/null 2>&1 || {
  echo "journeys-macos: ffmpeg is not installed" >&2
  exit 1
}

ALL=(record record-mp4 snapshot cancel-encode retry)
want=("$@")
[ ${#want[@]} -eq 0 ] && want=("${ALL[@]}")

# A stray capture from an earlier run holds the screen device and makes the next
# recording fail in a way that looks like a product bug — issue #45. The app
# sweeps these at start-up now, but a journey harness that begins from an unknown
# state cannot tell its own failures from inherited ones.
if pgrep -f 'ffmpeg.*avfoundation' >/dev/null 2>&1; then
  echo "journeys-macos: an ffmpeg is already capturing the screen." >&2
  echo "  That will make these fail for a reason that is not Glimpse's." >&2
  echo "  Clear it first:  pkill -f 'ffmpeg.*avfoundation'" >&2
  exit 1
fi

cargo build --locked -q

pass=0
fail=0
for mode in "${want[@]}"; do
  log=$(mktemp -t "glimpse-journey-$mode.XXXXXX")
  printf '%-14s ' "$mode"

  # No timeout(1) on macOS, so the watchdog is here: a journey that hangs must
  # end as a failure rather than as a build that never finishes.
  GLIMPSE_SELFTEST="$mode" ./target/debug/glimpse >"$log" 2>&1 &
  pid=$!
  waited=0
  while kill -0 "$pid" 2>/dev/null && [ "$waited" -lt 60 ]; do
    sleep 1
    waited=$((waited + 1))
  done
  if kill -0 "$pid" 2>/dev/null; then
    kill -9 "$pid" 2>/dev/null || true
    wait "$pid" 2>/dev/null || true
    echo "FAIL — did not finish within 60s"
    sed 's/^/    /' "$log" | tail -12
    fail=$((fail + 1))
    rm -f "$log"
    continue
  fi
  wait "$pid" 2>/dev/null || true

  # The SAME verdicts `smoke.sh` uses, from one file, so the two platforms
  # cannot drift into asserting different things. The first version of this
  # script had its own copy written from memory: it looked for `snapshot:` in a
  # log that says `status: saved`, and reported a working journey as a failure.
  # Weaker is the dangerous direction, and it landed on the platform with less
  # coverage to begin with.
  if scripts/journey-verdict.sh "$mode" "$log" >/dev/null 2>&1; then
    echo "ok"
    pass=$((pass + 1))
  else
    echo "FAIL"
    scripts/journey-verdict.sh "$mode" "$log" 2>&1 | sed 's/^/    /' || true
    fail=$((fail + 1))
  fi
  rm -f "$log"

  # Between journeys, not only at the end: a leaked capture would make the NEXT
  # journey fail and send whoever reads this looking in the wrong place.
  if pgrep -f 'ffmpeg.*avfoundation' >/dev/null 2>&1; then
    echo "  WARNING: a capture survived this journey. That is issue #45."
    pkill -f 'ffmpeg.*avfoundation' || true
  fi
done

echo
echo "journeys-macos: $pass passed, $fail failed"
exit $((fail > 0))
