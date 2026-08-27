#!/usr/bin/env bash
#
# Keep the docs honest about the code.
#
#   scripts/sync-docs.sh           regenerate what is generated, report drift
#   scripts/sync-docs.sh --check   change nothing, exit non-zero on any drift
#
# The --check form runs in `make check` and in CI, because documentation drift is
# not caught by the compiler and was the single largest category of finding in
# this project's first review: three published claims did not match the code.
#
# What is GENERATED (do not hand-edit between the markers):
#   * the ADR index in README.md — titles come from each ADR's own heading
#
# What is VERIFIED (hand-written, mechanically checked):
#   * every path in the README layout block exists
#   * every Rust source under src/ and crates/*/ appears in that block
#   * every `make <target>` mentioned in any .md is a real target
#   * every relative link in the .md files resolves to a real file
#   * every image referenced by a doc exists AND is tracked by git
#
# Deliberately not piped through head/tail: a pipeline reports the pager's exit
# status, so a failure would look like a success.

set -euo pipefail
cd "$(dirname "${BASH_SOURCE[0]}")/.."

CHECK=0
[[ "${1:-}" == "--check" ]] && CHECK=1

drift=0
note() { printf '  %s\n' "$1"; }
fail() { printf 'DRIFT: %s\n' "$1"; drift=1; }

# ---------------------------------------------------------------- ADR index --
generate_adr_index() {
  local f n title
  for f in docs/adr/*.md; do
    n=$(basename "$f" .md)
    n=${n%%-*}
    # The title line is `# 0000 — Something`; keep the part after the number.
    title=$(sed -n 's/^# [0-9]\{4\} — //p' "$f")
    [[ -z "$title" ]] && title="(untitled — ADR is missing its '# NNNN — Title' heading)"
    printf '  - [%s](%s) — %s\n' "$n" "$f" "$title"
  done
}

sync_block() {
  local file=$1 name=$2 content=$3 current
  current=$(awk -v n="$name" '
    $0 ~ "<!-- BEGIN GENERATED "n {inb=1; next}
    $0 ~ "<!-- END GENERATED "n {inb=0}
    inb {print}
  ' "$file")

  if [[ "$current" == "$content" ]]; then
    note "$name: up to date"
    return
  fi

  if (( CHECK )); then
    fail "$name in $file is stale — run 'make docs-sync'"
    return
  fi

  GLIMPSE_BLOCK="$content" python3 -c '
import os, re, sys, pathlib
path, name = sys.argv[1], sys.argv[2]
body = os.environ["GLIMPSE_BLOCK"].rstrip("\n")
p = pathlib.Path(path); s = p.read_text()
pat = re.compile(r"(<!-- BEGIN GENERATED " + re.escape(name) + r"[^\n]*-->\n).*?(<!-- END GENERATED " + re.escape(name) + r" -->)", re.S)
s2, n = pat.subn(lambda m: m.group(1) + body + "\n" + m.group(2), s)
if n == 0:
    sys.exit("markers for %s not found in %s" % (name, path))
p.write_text(s2)
' "$file" "$name"
  note "$name: regenerated"
}

# ----------------------------------------------------------------- verifiers --
# Every path the README layout block names, with tree prefixes resolved.
#
# The block is a tree: an unindented entry ending in / opens a directory, and
# indented entries below it are relative to that directory. Both the
# does-it-exist check and the is-it-listed check read this, so they cannot
# disagree about what the block says.
layout_paths() {
  awk '/<!-- BEGIN GENERATED layout/{inb=1; next} /<!-- END GENERATED layout/{inb=0} inb' README.md \
    | awk '
        /^```/ { next }
        /^[^[:space:]]/ { if ($1 ~ /\/$/) { prefix = $1; next } else { prefix = "" ; print $1; next } }
        /^[[:space:]]/  { if (NF) print prefix $1 }
      '
}

verify_layout_paths() {
  local missing=0 p
  while read -r p; do
    [[ -z "$p" || "$p" == '```' ]] && continue
    if [[ ! -e "$p" && ! -e "${p%/}" ]]; then
      fail "README layout names '$p', which does not exist"
      missing=1
    fi
  done < <(layout_paths)

  (( missing )) || note "layout paths: all exist"
}

# Every Rust source in the workspace must appear in the layout block.
#
# Matched on the FULL resolved path, not on a basename: with one crate per
# platform there are now several `src/geometry.rs`, and a basename match would
# let one of them go missing while its namesake covered for it.
verify_sources_are_documented() {
  local f undocumented=0 listed
  listed=$(layout_paths)
  while read -r f; do
    [[ -z "$f" ]] && continue
    if ! printf '%s\n' "$listed" | grep -qxF "$f"; then
      fail "$f is not mentioned in the README layout block"
      undocumented=1
    fi
  done < <(find src crates -name '*.rs' -not -path '*/target/*' 2>/dev/null | sort)
  (( undocumented )) || note "source coverage: every module is listed"
}

verify_make_targets() {
  local t bad=0
  while read -r t; do
    [[ -z "$t" ]] && continue
    if ! grep -qE "^${t}:" Makefile; then
      fail "docs reference 'make $t', which is not a target"
      bad=1
    fi
  # Only inside backticks — otherwise English prose ("make it clear") is matched
  # as a target, which is how the first version of this check embarrassed itself.
  done < <(grep -rhoE '`make [a-z][a-z-]+`' -- *.md docs/*.md docs/adr/*.md 2>/dev/null \
             | tr -d '`' | awk '{print $2}' | sort -u)
  (( bad )) || note "make targets: every documented target exists"
}

verify_assets_are_tracked() {
  # A file can exist locally, satisfy every path check, and still be missing from
  # the repository because .gitignore swallowed it — which ships a broken image to
  # everyone but the author. This caught exactly that: docs/assets/demo.gif was
  # matched by a bare `*.gif` rule.
  local f asset bad=0
  for f in README.md CONTRIBUTING.md AGENTS.md docs/*.md docs/adr/*.md; do
    [[ -e "$f" ]] || continue
    while read -r asset; do
      [[ -z "$asset" || "$asset" == http* ]] && continue
      asset="$(dirname "$f")/$asset"
      asset="${asset#./}"
      if [[ ! -e "$asset" ]]; then
        fail "$f references '$asset', which does not exist"
        bad=1
      elif ! git ls-files --error-unmatch "$asset" >/dev/null 2>&1; then
        fail "$asset exists but is NOT tracked by git — check .gitignore"
        bad=1
      fi
    done < <(grep -oE '(src|srcset)="[^"]+"' "$f" | sed 's/^[a-z]*="//; s/"$//')
  done
  (( bad )) || note "referenced assets: all exist and are tracked"
}

verify_relative_links() {
  local f link target bad=0
  for f in README.md AGENTS.md CONTRIBUTING.md docs/*.md docs/adr/*.md; do
    while read -r link; do
      [[ -z "$link" || "$link" == http* || "$link" == '#'* ]] && continue
      link=${link%%#*}
      target="$(dirname "$f")/$link"
      if [[ ! -e "$target" ]]; then
        fail "$f links to '$link', which does not resolve"
        bad=1
      fi
    done < <(grep -oE '\]\([^)]+\)' "$f" | sed 's/^](//; s/)$//')
  done
  (( bad )) || note "relative links: all resolve"
}

# ---------------------------------------------------------------------- run --
printf 'Docs sync%s\n' "$( ((CHECK)) && printf ' (check only)')"
sync_block README.md adr-index "$(generate_adr_index)"
verify_layout_paths
verify_sources_are_documented
verify_make_targets
verify_assets_are_tracked
verify_relative_links

if (( drift )); then
  printf '\nDocumentation is out of date with the code.\n'
  (( CHECK )) && printf "Run 'make docs-sync', then fix whatever it could not generate.\n"
  exit 1
fi
printf 'Docs are consistent with the code.\n'
