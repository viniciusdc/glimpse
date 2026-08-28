#!/usr/bin/env bash
#
# Every user journey the app implements must be driven by something.
#
# WHY THIS EXISTS. `GLIMPSE_SELFTEST=<mode>` drives the app through a journey by
# pressing the same buttons a user would. Five modes were implemented. Two were
# driven by `make`. The other three — `snapshot`, `cancel-encode` and `retry` —
# were reachable, written deliberately, and run by nothing at all.
#
# They are also the three that matter most. `cancel-encode` and `retry` are the
# durability guarantees ADR 0002 was written for: cancelling mid-encode must
# preserve the recording, and a preserved capture must be re-encodable without
# recording again. The pure state machine covers those as *policy*, instantly and
# thoroughly. What nothing covered is that the UI is wired to that policy, which
# is the entire reason those journeys exist.
#
# An orphaned journey is worse than a missing one. It looks like coverage in the
# source, it passes review, and it never runs. The same shape as a media suite
# that skips while the badge stays green.
#
# WHAT WOULD HAPPEN IF THE THING THIS TESTS WERE BROKEN. Delete this check and
# the next journey somebody adds joins the other three silently, because nothing
# else in the build has any opinion about whether a mode has a caller.

set -euo pipefail
cd "$(dirname "${BASH_SOURCE[0]}")/.."

UI=crates/glimpse-x11/src/ui.rs

# Modes the app answers to. `1` is the geometry self-test and is matched
# separately because it is tested with `is_ok()` rather than a string compare.
implemented=$(
  {
    grep -oE 'mode == "[a-z0-9-]+"' "$UI" | sed 's/.*"\(.*\)"/\1/'
    echo 1
  } | sort -u
)

# Modes something actually drives: a make target, or a script invoked by one.
driven=$(
  {
    grep -ohE 'smoke\.sh [a-z0-9-]+' Makefile | awk '{print $2}'
    grep -ohE 'GLIMPSE_SELFTEST=[a-z0-9-]+' Makefile scripts/*.sh | cut -d= -f2
  } | sort -u
)

orphans=$(comm -23 <(printf '%s\n' "$implemented") <(printf '%s\n' "$driven"))

printf 'Journeys\n'
printf '  implemented: %s\n' "$(echo "$implemented" | tr '\n' ' ')"
printf '  driven     : %s\n' "$(echo "$driven" | tr '\n' ' ')"

if [[ -n "$orphans" ]]; then
  echo
  echo "ORPHANED: these journeys exist and nothing runs them:" >&2
  for m in $orphans; do
    echo "  GLIMPSE_SELFTEST=$m" >&2
  done
  echo >&2
  echo "Add a make target that drives it, or delete the journey. A journey that" >&2
  echo "nobody runs is not coverage, it is code that looks like coverage." >&2
  exit 1
fi

echo "  every journey has a caller"
