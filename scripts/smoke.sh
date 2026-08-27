#!/usr/bin/env bash
#
# Record end to end, off-screen, and turn the result into an exit status.
#
#   scripts/smoke.sh record       record -> GIF
#   scripts/smoke.sh record-mp4   record -> MP4
#
# WHY THIS EXISTS. The smoke test drives Record and Stop through the same code
# path the button uses, prints the states it passed through, and always exited 0
# — including when it never recorded anything.
#
# That was not hypothetical. The split button remembers whether you last chose
# Record or Snapshot and persists it to config.toml (ADR 0009). The record smoke
# branch did not state the mode, so with `mode = "snapshot"` persisted it pressed
# Snapshot instead. Snapshot does not go through the session machine, so the
# state stayed Idle, no recording happened, and `make smoke` reported success.
# The harness had stopped testing recording and could not tell anyone.
#
# The mode bug is fixed in ui.rs. This exists because the class is not: the smoke
# test reads state out of a UI carrying persisted user settings, so it will
# always have more ways to do nothing than to do the wrong thing. `make check`
# already refuses a media suite that skips, for exactly this reason.
#
# WHAT WOULD HAPPEN IF THE THING THIS TESTS WERE BROKEN. Delete the Recording
# check and any future variant of "the button did something else" passes green.
# Delete the Completed check and a recording that fails to encode passes green.

set -euo pipefail
cd "$(dirname "${BASH_SOURCE[0]}")/.."

mode=${1:-record}
shift || true

log=$(mktemp -t glimpse-smoke.XXXXXX)
trap 'rm -f "$log"' EXIT

status=0
scripts/headless.sh env "GLIMPSE_SELFTEST=$mode" "$@" 2>&1 | tee "$log" || status=$?

fail() {
  echo >&2
  echo "SMOKE FAILED ($mode): $1" >&2
  shift
  for line in "$@"; do
    [[ -n $line ]] && echo "  $line" >&2
  done
  exit 1
}

(( status == 0 )) || fail "the app exited $status"

grep -q '\[smoke\]' "$log" || fail "the smoke harness never ran"

# Arming is the half a wrong button silently skips.
grep -q '\[smoke\] state: Recording' "$log" \
  || fail "Record did not arm a recording" "$(grep '\[smoke\] state:' "$log" || true)"

# And finishing is the half a broken encode silently skips.
grep -q '\[smoke\] final state: Completed' "$log" \
  || fail "the recording did not complete" "$(grep '\[smoke\] final state:' "$log" || true)"

echo
echo "smoke ($mode): armed a real recording and completed it."
