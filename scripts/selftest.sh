#!/usr/bin/env bash
#
# Run the geometry self-test and turn its report into an exit status.
#
#   scripts/selftest.sh              on the current display
#   scripts/selftest.sh --headless   on a private X server
#
# WHY THIS EXISTS. The self-test is the only check that can see a misplaced
# capture rectangle — the suite provably cannot (ADR 0000). It printed a report
# and always exited 0, which left three ways for a broken run to read as a good
# one:
#
#   * The grab fails and the PREVIOUS run's PNG is still on disk. `make selftest`
#     then says "now open /tmp/glimpse-selftest.png" and you inspect a correct
#     image from an earlier run. This is the worst of the three: the artifact is
#     genuine, it is simply answering a question nobody asked.
#   * The shape verdict says FAIL and the process exits 0 anyway, so anything
#     reading the status — a script, CI, a `&&` chain — sees success.
#   * The app exits before printing anything at all, which is indistinguishable
#     from a pass to everything except a human reading the scrollback.
#
# All three are the house failure mode: not a wrong answer, but a check that
# cannot tell you it did not run. `make check` already refuses a media suite that
# skips, for the same reason.
#
# WHAT WOULD HAPPEN IF THE THING THIS TESTS WERE BROKEN. Delete the PNG-freshness
# check and a failed grab passes silently on any machine that has ever run the
# self-test successfully. Delete the verdict check and a run that never reached
# the self-test passes everywhere.

set -euo pipefail
cd "$(dirname "${BASH_SOURCE[0]}")/.."

# `headless.sh` exits 97 when it refused and the command never ran. Distinguishing
# that from the command's own failure is the whole reason it has a reserved
# status: reporting "the app exited 1" about an app that was never started is the
# same defect this file exists to prevent, one layer up.
readonly RUNNER_REFUSED=97


PNG=/tmp/glimpse-selftest.png
runner=()
if [[ ${1-} == --headless ]]; then
  runner=(scripts/headless.sh)
  shift
fi

# Remove it FIRST. Everything below can then treat the file's existence as
# evidence that this run produced it, rather than trusting a timestamp
# comparison against a clock the grab does not share.
rm -f "$PNG"

log=$(mktemp -t glimpse-selftest.XXXXXX)
trap 'rm -f "$log"' EXIT

status=0
# `${runner[@]+...}` rather than `${runner[@]}`: expanding an EMPTY array under
# `set -u` is an error in bash 3.2, which is what macOS ships. It does not fail
# loudly either. The whole pipeline is skipped — `tee` never runs, so the log is
# never created — and `status` keeps its initial 0, so the check below passes on
# a command that never executed. The report-presence check then catches it and
# blames the app for exiting early, when the app was never started.
"${runner[@]+"${runner[@]}"}" env GLIMPSE_SELFTEST=1 "$@" 2>&1 | tee "$log" || status=$?

# First argument is the reason; any further arguments are context lines, printed
# one per line. Building the message with command substitution ate the newlines,
# which is how the first version of this produced "...did not succeedgrab :
# FAILED".
fail() {
  echo >&2
  echo "SELF-TEST FAILED: $1" >&2
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

# The report itself. Its absence means the run died before the self-test ran —
# the case that otherwise looks exactly like success.
grep -q '=== glimpse self-test ===' "$log" \
  || fail "no self-test report was printed; the app exited before running it"

# The self-test writes its verdicts as prose. Read them rather than re-deriving
# them here, so this script cannot disagree with the report the human is reading.
grep -q 'input shape  : PASS' "$log" \
  || fail "the input-region check did not pass" "$(grep 'input shape' "$log" || true)"

grep -q 'grab         : wrote' "$log" \
  || fail "the grab did not succeed" "$(grep 'grab  ' "$log" || true)"

[[ -s $PNG ]] || fail "$PNG was not written by this run"

echo
echo "Self-test passed its mechanical checks, and $PNG is from THIS run."
echo
echo "That is not the whole check. Open it and look: any Glimpse chrome in the"
echo "image means the capture rect is wrong, whatever the numbers above said."
echo "A rect that agreed with xwininfo to the pixel was once wrong by a 3px"
echo "border, and only the image showed it."
