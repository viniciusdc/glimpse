#!/usr/bin/env bash
#
# Did a journey pass? One definition, for every platform that runs one.
#
#   scripts/journey-verdict.sh <mode> <logfile>
#
# WHY THIS EXISTS SEPARATELY. `smoke.sh` runs the journeys under Xvfb, which is
# X11 only; `journeys-macos.sh` runs the same journeys on a real macOS screen,
# because there is no Xvfb there. Two runners, and for a while two sets of
# assertions — the macOS one written from memory, weaker, and already wrong: it
# looked for `snapshot:` in a log that says `status: saved`, and reported a
# working journey as a failure.
#
# Weaker is the dangerous direction. A second copy that asserts less passes
# everything the first would catch, on the platform with less coverage, which is
# precisely where the coverage was needed. So there is one copy and both runners
# call it.
#
# WHAT WOULD HAPPEN IF THE THING THIS TESTS WERE BROKEN. The per-journey
# reasoning below is the whole value: `Recording` then `Completed` is the record
# path and means nothing for the others — a snapshot never enters the session
# machine at all (ADR 0009), and cancel-encode is a success precisely when it
# does NOT complete. Asserting the record shape everywhere would make three
# journeys either permanently red or trivially green.

set -euo pipefail

mode=${1:?usage: journey-verdict.sh <mode> <logfile>}
log=${2:?usage: journey-verdict.sh <mode> <logfile>}

fail() {
  echo >&2
  echo "JOURNEY FAILED ($mode): $1" >&2
  shift
  for line in "$@"; do
    [[ -n $line ]] && echo "  $line" >&2
  done
  exit 1
}

# The harness itself has to have run. An app that exits 0 having pressed nothing
# is the failure `smoke.sh` was written for.
grep -q '\[smoke\]' "$log" || fail "the smoke harness never ran"

want() {
  grep -q "$1" "$log" || fail "$2" "$(grep '\[smoke\]' "$log" || true)"
}

case "$mode" in
  record|record-mp4)
    # Arming is the half a wrong button silently skips.
    want '\[smoke\] state: Recording'        "Record did not arm a recording"
    # And finishing is the half a broken encode silently skips.
    want '\[smoke\] final state: Completed'  "the recording did not complete"
    echo "journey ($mode): armed a real recording and completed it."
    ;;

  snapshot)
    # A snapshot is deliberately not a session (ADR 0009), so there is no state
    # to assert. What matters is that it committed a file: `saved <path>` is the
    # success text and anything else in that slot is the error.
    want '\[smoke\] pressing Snapshot'       "the Snapshot button was never pressed"
    want '\[smoke\] status: saved '          "the snapshot did not commit a file"
    echo "journey ($mode): pressed Snapshot and committed a file."
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
    echo "journey ($mode): cancelled mid-encode, reached Cancelled, reaped the child."
    ;;

  retry)
    # The other half of ADR 0002: a preserved capture is re-encodable without
    # recording again. `retry visible: true` is the UI half, `after retry:
    # Completed` is the one that proves it actually re-encoded.
    want '\[smoke\] retry visible: true'     "no retry was offered for a preserved capture"
    want '\[smoke\] after retry: Completed'  "the retry did not produce a finished encode"
    echo "journey ($mode): re-encoded a preserved capture without recording again."
    ;;

  *)
    # Not a default-pass. An unknown mode means a journey was added and this file
    # was not told about it, which is the orphaned-journey failure
    # `check-journeys.sh` exists for, arriving one layer down.
    fail "no verdict is defined for this journey" \
         "add one here; a journey with no assertions is not a tested journey"
    ;;
esac
