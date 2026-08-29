#!/usr/bin/env bash
#
# Check that external links in the docs still resolve.
#
#   scripts/check-links-external.sh                 every markdown file
#   scripts/check-links-external.sh README.md ...   only the files named
#
# Scoped to the files a change actually touches, because the alternative is
# re-checking forty URLs on every pull request to learn nothing about any of
# them. A pull request is answerable for the links it introduces or edits; it is
# not answerable for the ones it left alone, and failing it for those trains
# people to ignore the check.
#
# WHAT COUNTS AS A FAILURE, AND WHAT DOES NOT.
#
# This is the whole design. A link checker that treats every non-200 as broken is
# red every time somebody else has a bad afternoon, and a check that is red for
# reasons unrelated to the change is one people learn to skip.
#
#   404, 410            the link is wrong. FAIL — this is our bug.
#   401, 403, 405, 429  the host dislikes bots, HEAD, or our rate. NOT a verdict
#                       on the URL. Reported and passed over.
#   5xx, timeout, DNS   their outage, not our link. Reported and passed over.
#   2xx, 3xx            fine.
#
# So the only thing that turns the build red is a URL the internet positively
# says does not exist.
#
# Deliberately not parallel. Twenty URLs at one a time is a few seconds, and
# firing them concurrently at a handful of hosts is how you earn the 429 this
# script then has to forgive.

set -uo pipefail
cd "$(dirname "${BASH_SOURCE[0]}")/.."

TIMEOUT="${GLIMPSE_LINK_TIMEOUT:-12}"

files=("$@")
if [[ ${#files[@]} -eq 0 ]]; then
  # No arguments: everything. `find` rather than a glob so docs/adr/ is included
  # without naming it, and sorted so the output is stable between runs.
  while IFS= read -r f; do files+=("$f"); done < <(
    find . -name '*.md' -not -path './target/*' -not -path './.git/*' | sed 's|^\./||' | sort
  )
fi

# Nothing to do is a pass, not an error. The CI caller passes the changed-file
# list verbatim, and a pull request that touches no markdown legitimately has an
# empty list.
if [[ ${#files[@]} -eq 0 ]]; then
  echo "no markdown files to check"
  exit 0
fi

echo "checking external links in ${#files[@]} file(s)"

urls=$(
  for f in "${files[@]}"; do
    [[ -e "$f" ]] || continue
    # Markdown links, plus src=/srcset= so the README's badges and banner are
    # covered — those are the ones that rot silently, because a broken image
    # renders as nothing rather than as an error.
    grep -oE '\]\(https?://[^)]+\)' "$f" 2>/dev/null | sed 's/^](//; s/)$//'
    grep -oE '(src|srcset)="https?://[^"]+"' "$f" 2>/dev/null | sed 's/^[a-z]*="//; s/"$//'
  done | sed 's/[.,;:]$//' | sort -u
)

if [[ -z "$urls" ]]; then
  echo "  no external links in these files"
  exit 0
fi

fail=0
checked=0
forgiven=0

while IFS= read -r url; do
  [[ -z "$url" ]] && continue
  checked=$((checked + 1))

  # HEAD first. Some hosts answer HEAD with 405 or 403 while serving GET fine, so
  # anything that is not a clean answer gets one GET before we believe it.
  code=$(curl -sS -o /dev/null -w '%{http_code}' -I -L \
           --max-time "$TIMEOUT" --retry 1 --retry-delay 2 \
           -A 'glimpse-docs-link-check' "$url" 2>/dev/null)
  if [[ "$code" == "000" || "$code" == "405" || "$code" == "403" || "$code" == "401" ]]; then
    code=$(curl -sS -o /dev/null -w '%{http_code}' -L \
             --max-time "$TIMEOUT" --retry 1 --retry-delay 2 \
             -A 'glimpse-docs-link-check' "$url" 2>/dev/null)
  fi

  case "$code" in
    2*|3*)
      printf '  ok       %s\n' "$url"
      ;;
    404|410)
      printf '  BROKEN   %s  (HTTP %s)\n' "$url" "$code"
      fail=1
      ;;
    000)
      printf '  unreach  %s  (no response in %ss — treated as their outage)\n' "$url" "$TIMEOUT"
      forgiven=$((forgiven + 1))
      ;;
    *)
      printf '  skipped  %s  (HTTP %s — not a verdict on the URL)\n' "$url" "$code"
      forgiven=$((forgiven + 1))
      ;;
  esac
done <<< "$urls"

echo
printf 'checked %s url(s), %s not conclusively answered\n' "$checked" "$forgiven"

if (( fail )); then
  echo 'FAIL: at least one link returned 404 or 410'
  exit 1
fi

echo 'no broken links'
exit 0
