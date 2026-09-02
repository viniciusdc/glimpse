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

# Every crate's sources, not one path. This gate used to read
# `crates/glimpse-x11/src/ui.rs`, and when the chrome moved to `glimpse-ui` the
# journeys went with it. The grep then matched nothing, `implemented` collapsed
# to the self-test alone, and an empty set is trivially a subset of anything — so
# this reported "every journey has a caller" while looking at a file that had
# none. The check written to stop journeys being orphaned had been orphaned
# itself.
SOURCES=$(ls crates/*/src/*.rs src/*.rs 2>/dev/null)

# Modes the app answers to, by string compare.
string_modes=$(grep -ohE 'mode == "[a-z0-9-]+"' $SOURCES | sed 's/.*"\(.*\)"/\1/' | sort -u)

# Finding none means the journeys moved again, or stopped being written this way.
# Either way the comparison below becomes vacuous, so refuse rather than pass:
# that is exactly how this went unnoticed the first time.
if [[ -z "$string_modes" ]]; then
  echo "no journey modes found in the workspace sources." >&2
  echo >&2
  echo "This greps for 'mode == \"...\"'. Matching nothing makes every" >&2
  echo "comparison below vacuous and this check would pass having verified" >&2
  echo "nothing — which is what happened when the chrome moved to glimpse-ui." >&2
  exit 1
fi

# `1` is the geometry self-test, matched separately because it is tested with
# `is_ok()` rather than a string compare.
implemented=$(printf '%s\n1\n' "$string_modes" | sort -u)

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

# Every journey must end through the one helper that honours
# GLIMPSE_SELFTEST_HOLD. The decision was copied at five journey endings before
# it was collapsed, and a sixth journey quitting on its own would be invisible:
# the app just vanishes before it can be looked at, which is the failure the hold
# was added to prevent. Located by the helper rather than by path, so this does
# not go blind the next time the file moves.
holder=$(grep -l 'fn finish_journey' $SOURCES || true)
if [[ -z "$holder" ]]; then
  echo "finish_journey not found; journeys have no single place to end" >&2
  exit 1
fi
quits=$(grep -cE '\.quit\(\)' "$holder")
if [[ "$quits" != "1" ]]; then
  echo >&2
  echo "$holder quits in $quits places; it must quit in exactly one." >&2
  echo "A journey that quits on its own ignores GLIMPSE_SELFTEST_HOLD, and the" >&2
  echo "symptom is an app that vanishes before anyone can look at it." >&2
  exit 1
fi
echo "  every journey ends through finish_journey"
