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
#   * every `#fragment` in a link resolves to a real heading
#   * every image referenced by a doc exists AND is tracked by git
#   * markdown hygiene: no trailing whitespace, no tabs, no skipped heading level
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

# No path may be listed twice.
#
# Neither check above notices a duplicate: a repeated entry exists, so the
# does-it-exist check passes, and it is listed, so the coverage check passes.
# `scripts/smoke.sh` and `scripts/selftest.sh` were each in the block twice with
# DIFFERENT descriptions, which is the damaging form — two descriptions of one
# file, and a reader has no way to tell which is current.
verify_no_duplicate_paths() {
  local dupes
  dupes=$(layout_paths | grep -v '^$' | sort | uniq -d)
  if [[ -n "$dupes" ]]; then
    while read -r p; do
      [[ -n "$p" ]] && fail "README layout lists '$p' more than once"
    done <<< "$dupes"
    return
  fi
  note "layout paths: no duplicates"
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


# Every `#fragment` resolves to a heading that exists.
#
# `verify_relative_links` deliberately drops the fragment before checking, so
# `docs/faq.md#anything-at-all` passes as long as faq.md exists, and a bare
# `#section` link is skipped entirely. That is the blind half: the link most
# likely to rot is the one pointing INTO a document somebody rewrote, and it is
# exactly the one nothing looked at.
#
# The slug rules match GitHub's: lowercase, drop anything that is not
# alphanumeric/space/hyphen, spaces become hyphens. Written in python because
# doing it in sed would be write-only.
verify_anchors() {
  local out
  out=$(python3 - <<'PYEOF'
import os, re, pathlib, sys

def slug(h):
    s = re.sub(r'`|\*|_|\[|\]|\(|\)', '', h.strip().lower())
    s = re.sub(r'[^a-z0-9 \-]', '', s)
    return re.sub(r'\s+', '-', s.strip())

files = sorted(pathlib.Path('.').glob('*.md')) + sorted(pathlib.Path('docs').rglob('*.md'))
headings = {}
for f in files:
    txt = f.read_text()
    seen, hs = {}, set()
    for m in re.finditer(r'^(#{1,6})\s+(.*?)\s*$', txt, re.M):
        base = slug(m.group(2))
        # GitHub disambiguates repeats with -1, -2, ...
        n = seen.get(base, 0); seen[base] = n + 1
        hs.add(base if n == 0 else f'{base}-{n}')
    headings[str(f)] = hs

bad = 0
for f in files:
    for m in re.finditer(r'\]\(([^)]+)\)', f.read_text()):
        link = m.group(1)
        if link.startswith('http') or '#' not in link:
            continue
        path, frag = link.split('#', 1)
        # normpath, not as_posix: from docs/faq.md a link to ../README.md
        # resolves to 'docs/../README.md', which matches no key and would be
        # reported as "not a scanned file" — rejecting a link that is correct.
        target = os.path.normpath(f.parent / path) if path else str(f)
        if target not in headings:
            print(f"{f} links to '{link}', whose target is not a scanned markdown file")
            bad += 1
        elif frag not in headings[target]:
            print(f"{f} links to '{link}', but '#{frag}' is not a heading there")
            bad += 1
sys.exit(1 if bad else 0)
PYEOF
  ) || { while read -r l; do [[ -n "$l" ]] && fail "$l"; done <<< "$out"; return; }
  note "link fragments: all resolve to real headings"
}

# Markdown hygiene, limited to things that are unambiguously wrong.
#
# Not a style opinion: trailing whitespace becomes an invisible <br> in some
# renderers, a leading tab indents unpredictably across renderers, and a heading
# level jump (## straight to ####) breaks the document outline that screen
# readers and verify_anchors both depend on.
#
# In python, not grep. The first version used `grep -P`, which is GNU-only: on
# macOS every call failed with "invalid option -- P" and the function still
# printed its success line, because the exit status went to the loop and not to
# the check. A hygiene check that cannot fail on the developer's own machine is
# worse than no check, since it reports the thing it never looked at.
verify_markdown_hygiene() {
  local out
  out=$(python3 - <<'PYEOF'
import pathlib, re, sys

files = sorted(pathlib.Path('.').glob('*.md')) + sorted(pathlib.Path('docs').rglob('*.md'))
bad = 0
for f in files:
    prev, fence = 0, False
    for n, line in enumerate(f.read_text().splitlines(), 1):
        if line.lstrip().startswith('```'):
            fence = not fence
            continue
        if fence:
            continue
        if line != line.rstrip():
            print(f"{f}:{n} has trailing whitespace"); bad += 1
        if line.startswith('\t'):
            print(f"{f}:{n} indents with a tab"); bad += 1
        m = re.match(r'^(#{1,6}) +\S', line)
        if m:
            lvl = len(m.group(1))
            if prev and lvl > prev + 1:
                print(f"{f}:{n} jumps from h{prev} to h{lvl}"); bad += 1
            prev = lvl
sys.exit(1 if bad else 0)
PYEOF
  ) || { while read -r l; do [[ -n "$l" ]] && fail "$l"; done <<< "$out"; return; }
  note "markdown hygiene: no trailing whitespace, tabs or heading jumps"
}

# ---------------------------------------------------------------------- run --
printf 'Docs sync%s\n' "$( ((CHECK)) && printf ' (check only)')"
sync_block README.md adr-index "$(generate_adr_index)"
verify_layout_paths
verify_no_duplicate_paths
verify_sources_are_documented
verify_make_targets
verify_assets_are_tracked
verify_relative_links
verify_anchors
verify_markdown_hygiene

if (( drift )); then
  printf '\nDocumentation is out of date with the code.\n'
  (( CHECK )) && printf "Run 'make docs-sync', then fix whatever it could not generate.\n"
  exit 1
fi
printf 'Docs are consistent with the code.\n'
