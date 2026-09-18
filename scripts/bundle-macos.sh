#!/usr/bin/env bash
#
# Build Glimpse.app: the binary, the GTK dylibs it needs, and an identity.
#
#   scripts/bundle-macos.sh            # -> target/Glimpse.app
#   scripts/bundle-macos.sh --skip-run # build and check paths, do not launch it
#
# WHY A BUNDLE AT ALL. [ADR 0013](../docs/adr/0013-macos-ships-an-app-bundle.md).
# The deciding argument is not the dylibs, which could be shipped in a tarball
# with `@executable_path` install names. It is Screen Recording permission: macOS
# grants it to the *responsible process*, so a bare binary launched from a
# terminal inherits the terminal's grant, and launched from Finder has no
# identity for a grant to persist against at all. A screen recorder whose
# permission story is "grant it to whatever launched me" is not shippable.
#
# WHAT IT HAS TO REWRITE. Homebrew's dylibs carry absolute install names
# (/opt/homebrew/...) and the binary has no LC_RPATH, so there is no search path
# to redirect and every name has to be changed one at a time. Measured against
# the real release binary: 12 direct, 39 in the transitive closure, 26.9 MB, 29
# packages.
#
# Eight of the 39 are X11 client libraries, on a Quartz build. They come from
# Homebrew's cairo, which links them whether or not the X11 backend is built, and
# it is not. They are unreachable and they ship anyway; at 1.6 MB they are not
# worth engineering around. Finding libX11 inside a macOS app bundle looks like a
# misconfiguration and it is not one.
#
# IT VERIFIES ITSELF. ADR 0013 closed with "what is still unverified is that
# rewriting all 39 install names actually produces a working bundle", and a build
# script that produces a bundle nobody launched would leave that sentence true.
# So this refuses to finish unless no Homebrew path survives anywhere in the
# bundle AND the app it just built comes up and reports a capture rect.

set -euo pipefail
cd "$(dirname "${BASH_SOURCE[0]}")/.."

SKIP_RUN=0
[ "${1:-}" = "--skip-run" ] && SKIP_RUN=1

if [ "$(uname -s)" != "Darwin" ]; then
  echo "bundle-macos: macOS only." >&2
  exit 1
fi

missing=()
command -v otool            >/dev/null 2>&1 || missing+=(otool)
command -v install_name_tool >/dev/null 2>&1 || missing+=(install_name_tool)
command -v codesign          >/dev/null 2>&1 || missing+=(codesign)
if [ ${#missing[@]} -ne 0 ]; then
  echo "bundle-macos: missing ${missing[*]} — install the Xcode command line tools." >&2
  exit 1
fi

# Homebrew's prefix differs by architecture: /opt/homebrew on Apple Silicon,
# /usr/local on Intel. Asked rather than hardcoded, so an Intel build does not
# silently bundle nothing and ship a binary that cannot start.
BREW_PREFIX="$(brew --prefix 2>/dev/null || echo /opt/homebrew)"
echo "bundle-macos: homebrew prefix $BREW_PREFIX"

VERSION=$(sed -n 's/^version = "\(.*\)"/\1/p' Cargo.toml | head -1)
APP="target/Glimpse.app"
CONTENTS="$APP/Contents"
FRAMEWORKS="$CONTENTS/Frameworks"
RESOURCES="$CONTENTS/Resources"

cargo build --locked --release
rm -rf "$APP"
mkdir -p "$CONTENTS/MacOS" "$FRAMEWORKS" "$RESOURCES"
cp target/release/glimpse "$CONTENTS/MacOS/glimpse"

# ---- the transitive closure -------------------------------------------------
#
# Breadth-first over `otool -L`, because the direct list is a third of the real
# answer.
#
# **Not every dependency is an absolute path.** An early version of this matched
# only names beginning with the Homebrew prefix, which silently skipped
# `@rpath/...` entries — and Homebrew's libwebp names `@rpath/libsharpyuv.0.dylib`
# that way. The library was never copied, the reference was never rewritten, and
# the bundle died on `dyld: Library not loaded` the first time it ran somewhere
# that build had not been done. It did not fail on the machine that wrote this,
# because that machine's gdk-pixbuf does not pull webp in at all: 39 dylibs here,
# 40 on a CI runner. A closure that only understands one spelling of "depends on"
# is a closure that is right until the next machine.
#
# So: take every dependency that is not part of the OS, resolve it — through
# LC_RPATH and @loader_path if that is how it is written — and bundle whatever it
# resolves to.

# Dependencies worth bundling: everything except the OS's own, which are present
# on any Mac and must NOT be copied.
deps_raw() {
  otool -L "$1" | tail -n +2 | awk '{print $1}' |
    grep -v '^/usr/lib/' | grep -v '^/System/' || true
}

# The rpath entries a Mach-O carries, which is how `@rpath/...` is resolved.
rpaths_of() {
  otool -l "$1" | awk '/LC_RPATH/{want=1} want && $1=="path" {print $2; want=0}'
}

# A dependency string, turned into a real file on disk. Empty if it cannot be
# resolved, which the caller treats as fatal rather than skipping: a dependency
# nobody can find is the bug this function exists to stop shipping.
resolve_dep() {
  local dep="$1" owner="$2" dir base rp cand
  dir=$(dirname "$owner")
  case "$dep" in
    @rpath/*)
      base="${dep#@rpath/}"
      while IFS= read -r rp; do
        cand="${rp//@loader_path/$dir}"
        cand="${cand//@executable_path/$dir}"
        [ -f "$cand/$base" ] && { /usr/bin/python3 -c "import os,sys;print(os.path.realpath(sys.argv[1]))" "$cand/$base"; return; }
      done < <(rpaths_of "$owner")
      # Homebrew keeps everything under one prefix, so this is a sound last
      # resort when a library names an rpath it does not itself carry.
      [ -f "$BREW_PREFIX/lib/$base" ] && { /usr/bin/python3 -c "import os,sys;print(os.path.realpath(sys.argv[1]))" "$BREW_PREFIX/lib/$base"; return; }
      ;;
    @loader_path/*)
      cand="${dep/@loader_path/$dir}"
      [ -f "$cand" ] && /usr/bin/python3 -c "import os,sys;print(os.path.realpath(sys.argv[1]))" "$cand"
      ;;
    /*)
      [ -f "$dep" ] && /usr/bin/python3 -c "import os,sys;print(os.path.realpath(sys.argv[1]))" "$dep"
      ;;
  esac
}

seen_file=$(mktemp -t glimpse-bundle-seen.XXXXXX)
queue_file=$(mktemp -t glimpse-bundle-queue.XXXXXX)
trap 'rm -f "$seen_file" "$queue_file"' EXIT

# The queue carries "owner<TAB>dependency", because resolving `@rpath` needs to
# know which library said it.
while IFS= read -r d; do
  printf '%s\t%s\n' "$CONTENTS/MacOS/glimpse" "$d" >> "$queue_file"
done < <(deps_raw "$CONTENTS/MacOS/glimpse")

while [ -s "$queue_file" ]; do
  line=$(head -1 "$queue_file")
  sed -i '' '1d' "$queue_file"
  owner="${line%%	*}"
  dep="${line#*	}"

  real=$(resolve_dep "$dep" "$owner")
  if [ -z "$real" ]; then
    echo "bundle-macos: cannot resolve '$dep' needed by $(basename "$owner")" >&2
    echo "  A dependency nobody can find is the bug this check exists to stop shipping." >&2
    exit 1
  fi

  base=$(basename "$real")
  grep -qx "$base" "$seen_file" 2>/dev/null && continue
  echo "$base" >> "$seen_file"
  cp "$real" "$FRAMEWORKS/$base"
  chmod u+w "$FRAMEWORKS/$base"
  while IFS= read -r d; do
    printf '%s\t%s\n' "$real" "$d" >> "$queue_file"
  done < <(deps_raw "$real")
done

count=$(wc -l < "$seen_file" | tr -d ' ')
size=$(du -sh "$FRAMEWORKS" | awk '{print $1}')
echo "bundle-macos: bundled $count dylibs ($size)"

# ---- rewrite every install name --------------------------------------------
#
# Two rewrites per library: its own id, and every dependency it names. Missing
# either leaves a dylib that resolves through Homebrew on the build machine and
# fails on anyone else's, which is the failure mode that looks like success.
#
# Rewriting is keyed on the ORIGINAL dependency string, whatever its spelling,
# and points at the basename the copy was made under.
rewrite() {
  local file="$1" dep real base
  while IFS= read -r dep; do
    real=$(resolve_dep "$dep" "$file")
    [ -z "$real" ] && continue
    base=$(basename "$real")
    install_name_tool -change "$dep" "@executable_path/../Frameworks/$base" "$file" 2>/dev/null || {
      echo "bundle-macos: could not rewrite $dep in $(basename "$file")" >&2
      exit 1
    }
  done < <(deps_raw "$file")
}

rewrite "$CONTENTS/MacOS/glimpse"
while IFS= read -r base; do
  lib="$FRAMEWORKS/$base"
  install_name_tool -id "@executable_path/../Frameworks/$base" "$lib"
  rewrite "$lib"
done < "$seen_file"

# ---- re-sign everything install_name_tool touched ---------------------------
#
# **Mandatory on Apple Silicon, and it is not about Gatekeeper.** Homebrew ships
# arm64 dylibs with an ad-hoc signature, `install_name_tool` invalidates it by
# editing the load commands, and the kernel then refuses to map the image at all.
# The failure gives you nothing to go on: the process dies on SIGKILL with no
# output and no diagnostic, which is exactly what the first build of this script
# produced — `--version` exited 137 having printed nothing.
#
# ADR 0013 discusses signing only as a Gatekeeper problem for a *downloaded*
# bundle. This is a different requirement and it applies to a bundle that never
# leaves the machine that built it.
#
# `--sign -` is ad-hoc: no certificate, no Apple Developer account. It satisfies
# the kernel. It does not satisfy Gatekeeper, which stays a separate cost.
echo "bundle-macos: re-signing $count dylibs and the binary"
while IFS= read -r base; do
  codesign --force --sign - --timestamp=none "$FRAMEWORKS/$base" 2>/dev/null || {
    echo "bundle-macos: could not sign $base" >&2
    exit 1
  }
done < "$seen_file"
codesign --force --sign - --timestamp=none "$CONTENTS/MacOS/glimpse"

# ---- the runtime data GTK needs and dylibs do not carry ---------------------
#
# A GTK application is not only its libraries. Without the compiled GSettings
# schemas it aborts at start-up on a missing schema; without the pixbuf loader
# cache it cannot decode an image; without an icon theme the symbolic icons in
# the header render as nothing. None of these is a dylib, so none of them is
# reachable from `otool`, and all three are invisible until the bundle runs
# somewhere that has no Homebrew.
mkdir -p "$RESOURCES/share/glib-2.0/schemas" "$RESOURCES/share/icons"
cp "$BREW_PREFIX/share/glib-2.0/schemas/gschemas.compiled" \
   "$RESOURCES/share/glib-2.0/schemas/" 2>/dev/null || {
  echo "bundle-macos: no compiled GSettings schemas at $BREW_PREFIX/share/glib-2.0/schemas" >&2
  echo "  GTK aborts at start-up without them. Try: glib-compile-schemas $BREW_PREFIX/share/glib-2.0/schemas" >&2
  exit 1
}
# Adwaita is its own keg and Homebrew does not link it into the main prefix, so
# looking in $BREW_PREFIX/share/icons finds only hicolor and concludes the theme
# is absent. Ask for the package's own prefix.
ADWAITA=""
for candidate in \
  "$(brew --prefix adwaita-icon-theme 2>/dev/null || true)/share/icons/Adwaita" \
  "$BREW_PREFIX/share/icons/Adwaita"; do
  [ -d "$candidate" ] && { ADWAITA="$candidate"; break; }
done
if [ -n "$ADWAITA" ]; then
  cp -R "$ADWAITA" "$RESOURCES/share/icons/"
  echo "bundle-macos: icon theme from $ADWAITA"
else
  # Not fatal, but not silent either: the header's menu button and the record
  # bullet are symbolic icons, and a bundle whose chrome renders blank looks
  # broken in a way that has nothing to do with recording.
  echo "bundle-macos: WARNING no Adwaita icon theme; header icons will be blank" >&2
  echo "  brew install adwaita-icon-theme" >&2
fi
# hicolor is the fallback theme every GTK icon lookup ends at. Without it GTK
# warns on every missing icon rather than falling back quietly.
[ -d "$BREW_PREFIX/share/icons/hicolor" ] &&
  cp -R "$BREW_PREFIX/share/icons/hicolor" "$RESOURCES/share/icons/" || true

# ---- Info.plist -------------------------------------------------------------
#
# CFBundleIdentifier is the whole point: it is the identity Screen Recording is
# granted to, and it has to match what the app is launched as for the grant to
# persist across restarts.
cat > "$CONTENTS/Info.plist" <<PLIST
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
	<key>CFBundleName</key><string>Glimpse</string>
	<key>CFBundleDisplayName</key><string>Glimpse</string>
	<key>CFBundleIdentifier</key><string>com.vinicius.glimpse</string>
	<key>CFBundleExecutable</key><string>glimpse</string>
	<key>CFBundleVersion</key><string>$VERSION</string>
	<key>CFBundleShortVersionString</key><string>$VERSION</string>
	<key>CFBundlePackageType</key><string>APPL</string>
	<key>LSMinimumSystemVersion</key><string>12.0</string>
	<key>NSHighResolutionCapable</key><true/>
</dict>
</plist>
PLIST

# ---- verify -----------------------------------------------------------------
#
# A bundle that still names Homebrew works perfectly on the machine that built it
# and nowhere else. Checking every Mach-O in the bundle, not just the binary.
echo "bundle-macos: checking for surviving $BREW_PREFIX and @rpath references"
leaked=0
while IFS= read -r f; do
  if otool -L "$f" 2>/dev/null | tail -n +2 | grep -q "^	$BREW_PREFIX"; then
    echo "  LEAKED: $f" >&2
    otool -L "$f" | grep "$BREW_PREFIX" | sed 's/^/      /' >&2
    leaked=1
  fi
  # An unresolved `@rpath/` is the same defect wearing a different spelling, and
  # it is the one that shipped: libwebp names @rpath/libsharpyuv.0.dylib, the
  # closure walk did not understand it, and the bundle died on `dyld: Library
  # not loaded` the first time it ran on a machine that had not built it.
  # Checking here turns that into a build failure naming the library.
  if otool -L "$f" 2>/dev/null | tail -n +2 | grep -q "^	@rpath/"; then
    echo "  UNRESOLVED @rpath: $f" >&2
    otool -L "$f" | grep "@rpath/" | sed 's/^/      /' >&2
    leaked=1
  fi
done < <(find "$APP" -type f \( -perm -u+x -o -name '*.dylib' \))
if [ "$leaked" -ne 0 ]; then
  echo "bundle-macos: the bundle still resolves through Homebrew and will not run elsewhere." >&2
  exit 1
fi
echo "bundle-macos: no Homebrew paths survive"

if [ "$SKIP_RUN" -eq 1 ]; then
  echo "bundle-macos: built $APP (not launched: --skip-run)"
  exit 0
fi

# And it has to actually start. ADR 0013 left "rewriting all 39 install names
# produces a working bundle" unverified, and a script that stops at the path
# check would leave it that way.
log=$(mktemp -t glimpse-bundle-run.XXXXXX)
"$CONTENTS/MacOS/glimpse" >"$log" 2>&1 &
pid=$!
trap 'kill "$pid" 2>/dev/null || true; rm -f "$seen_file" "$queue_file" "$log"' EXIT
for _ in $(seq 1 60); do
  grep -q 'capture rect' "$log" && break
  kill -0 "$pid" 2>/dev/null || break
  sleep 0.5
done
kill "$pid" 2>/dev/null || true
wait "$pid" 2>/dev/null || true

if ! grep -q 'capture rect' "$log"; then
  echo "bundle-macos: the bundled app did not start." >&2
  sed 's/^/  /' "$log" >&2
  exit 1
fi
echo "bundle-macos: it runs — $(grep 'capture rect' "$log" | head -1)"
echo "bundle-macos: built $APP"
