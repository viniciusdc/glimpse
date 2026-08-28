#!/usr/bin/env bash
#
# Record end to end, off-screen, and turn the result into an exit status.
#
#   scripts/smoke.sh record         record -> GIF
#   scripts/smoke.sh record-mp4     record -> MP4
#   scripts/smoke.sh snapshot       press Snapshot, commit a PNG
#   scripts/smoke.sh cancel-encode  cancel mid-encode, preserve, reap
#   scripts/smoke.sh retry          re-encode a preserved capture
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

# `headless.sh` exits 97 when it refused and the command never ran. Distinguishing
# that from the command's own failure is the whole reason it has a reserved
# status: reporting "the app exited 1" about an app that was never started is the
# same defect this file exists to prevent, one layer up.
readonly RUNNER_REFUSED=97


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

if (( status == RUNNER_REFUSED )); then
  fail "the off-screen runner refused; the app was never started" \
       "its reason is printed above"
fi
(( status == 0 )) || fail "the app exited $status"

grep -q '\[smoke\]' "$log" || fail "the smoke harness never ran"

# Per-journey assertions. `Recording` then `Completed` is the record path and
# means nothing for the others: a snapshot never enters the session machine at
# all, and cancel-encode is a success precisely when it does NOT complete.
# Asserting the record shape everywhere would have made three journeys either
# permanently red or trivially green.
want() {
  grep -q "$1" "$log" || fail "$2" "$(grep '\[smoke\]' "$log" || true)"
}

case "$mode" in
  record|record-mp4)
    # Arming is the half a wrong button silently skips.
    want '\[smoke\] state: Recording'        "Record did not arm a recording"
    # And finishing is the half a broken encode silently skips.
    want '\[smoke\] final state: Completed'  "the recording did not complete"
    echo
    echo "smoke ($mode): armed a real recording and completed it."
    ;;

  snapshot)
    # A snapshot is deliberately not a session (ADR 0009), so there is no state
    # to assert. What matters is that it committed a file: `saved <path>` is the
    # success text and anything else in that slot is the error.
    want '\[smoke\] pressing Snapshot'       "the Snapshot button was never pressed"
    want '\[smoke\] status: saved '          "the snapshot did not commit a file"
    echo
    echo "smoke ($mode): pressed Snapshot and committed a file."
    ;;

  cancel-encode)
    # ADR 0002's durability guarantee: cancelling mid-encode must reach
    # Cancelled and must not leave an ffmpeg behind. Checking the state alone
    # would pass while a child kept running into a deleted directory.
    want '\[smoke\] state before cancel: Encoding' \
         "the cancel did not land during an encode, so nothing was tested"
    want '\[smoke\] state after cancel:  Cancelled' \
         "cancelling mid-encode did not reach Cancelled"
    if [[ "$(grep -c '\[smoke\] ffmpeg alive: 0' "$log")" -lt 1 ]]; then
      fail "an ffmpeg survived the cancel" "$(grep 'ffmpeg alive' "$log" || true)"
    fi
    echo
    echo "smoke ($mode): cancelled mid-encode, reached Cancelled, reaped the child."
    ;;

  retry)
    # The other half of ADR 0002: a preserved capture is re-encodable without
    # recording again. `retry visible: true` is the UI half, `after retry:
    # Completed` is the one that proves it actually re-encoded.
    want '\[smoke\] retry visible: true'     "no retry was offered for a preserved capture"
    want '\[smoke\] after retry: Completed'  "the retry did not produce a finished encode"
    echo
    echo "smoke ($mode): re-encoded a preserved capture without recording again."
    ;;

  *)
    fail "unknown journey '$mode'" \
         "add its assertions here rather than letting it pass unchecked"
    ;;
esac
