#!/usr/bin/env bash
#
# Gather what a CI run produced but nothing looks at, into one directory.
#
#   scripts/collect-evidence.sh DEST [EXTRA ...]
#
# WHY THIS EXISTS. Every check in the workflow asserts on state, geometry or
# exit status. None of them looks at an image, and two shipped bugs went through
# green pull requests for exactly that reason: the macOS status bar on the wrong
# side of the frame, and a window shadow baked into the top of every macOS
# recording while the capture rect was provably correct. Both were visible in a
# picture and invisible to every assertion.
#
# CI already photographed the Linux UI for that reason. What it threw away was
# everything else: the self-test PNG — the one the self-test's own comment calls
# the half that matters — and every file the five journeys produced. The
# journeys assert that a recording completed and is decodable. Whether it is a
# recording OF ANYTHING is not something they can tell you.
#
# Nothing here is an assertion, and deliberately so. There is no reference to
# diff against — the two platforms are different window models by design
# (ADR 0015, ADR 0016) and a runner's desktop is not a user's — so a pixel
# comparison would be red forever. This is evidence for a person.
#
# ON THE RULE IN AGENTS.md. "The self-test PNG is a picture of your screen.
# Never attach it to a pull request, an issue, or a commit." That rule is about
# YOUR screen. A CI runner's desktop belongs to nobody — the same distinction
# that lets the journeys record there at all. So this uploads from CI, and the
# rule stands unchanged for anything produced on your own machine.
#
# WHAT WOULD HAPPEN IF THIS WERE DELETED. Nothing would fail. The next bug that
# is only visible in a picture would ship through a green run, which is how the
# two above arrived.

set -euo pipefail
cd "$(dirname "${BASH_SOURCE[0]}")/.."

DEST="${1:?usage: collect-evidence.sh DEST [EXTRA ...]}"
shift || true
mkdir -p "$DEST"

took=0
take() {
  [ -s "$1" ] || return 0
  cp "$1" "$DEST/$(basename "$1")" 2>/dev/null || return 0
  echo "  $(basename "$1")  ($(wc -c <"$1" | tr -d ' ') bytes)"
  took=$((took + 1))
}

echo "collect-evidence: into $DEST"

# Whatever the caller already made — the UI photograph, usually.
for extra in "$@"; do
  take "$extra"
done

# The self-test's PNG, whose path `selftest.sh` fixes.
take /tmp/glimpse-selftest.png

# What the journeys wrote. The output directory is the platform's, resolved at
# runtime by `default_output_dir` — XDG's videos directory, then ~/Movies on
# macOS, then $HOME — and there is no environment variable to pin it, so look
# where it can be rather than guessing one. Non-recursive: these land directly
# in it, and $HOME on a runner is full of things that are not ours.
#
# Only what THIS run made. A runner's home is empty, but yours is not: the first
# version of this collected every glimpse recording in ~/Movies going back
# months. Evidence for a run has to be from that run — the same reason
# `selftest.sh` refuses a PNG it did not write itself — and on your machine the
# difference is between one file and your archive.
MINUTES="${EVIDENCE_MAX_AGE_MIN:-60}"
cutoff=$(python3 -c "import datetime,sys; print((datetime.datetime.now()-datetime.timedelta(minutes=int(sys.argv[1]))).strftime('%Y-%m-%d %H:%M:%S'))" "$MINUTES")
echo "  (only files newer than $cutoff)"

for dir in "${XDG_VIDEOS_DIR:-}" "$HOME/Videos" "$HOME/Movies" "$HOME"; do
  [ -n "$dir" ] && [ -d "$dir" ] || continue
  while IFS= read -r f; do
    take "$f"
  done < <(find "$dir" -maxdepth 1 -type f -newermt "$cutoff" \
    \( -name 'glimpse*.gif' -o -name 'glimpse*.mp4' -o -name 'glimpse*.png' \) 2>/dev/null)
done

if [ "$took" -eq 0 ]; then
  # Not a failure. The journeys have their own verdicts and this runs with
  # `if: always()`, so an early failure legitimately leaves nothing to collect —
  # and a collector that failed the build would then be reporting the first
  # failure twice, under the wrong name.
  echo "  nothing to collect"
fi
echo "collect-evidence: $took file(s)"
