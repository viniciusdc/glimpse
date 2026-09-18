#!/usr/bin/env bash
#
# The installer and the release workflow must agree on what an artifact is called.
#
#   scripts/check-release-names.sh
#
# WHY THIS EXISTS. `scripts/install.sh` *constructs* the archive name that
# `.github/workflows/release.yml` *writes*, and neither file can see the other.
# `docs/releasing.md` has flagged that for as long as there has been one
# platform, and the fix it names is "a check that asserts the names match, not a
# second hardcoded string" — because a second copy is the thing that drifts.
#
# Adding macOS turned one coupling into four: two platforms, each with an archive
# name and an extension, and a third file (`docs/install.md`) telling people what
# to expect. A mismatch does not fail anything at build time. It fails at the
# moment a user runs the installer against a real release, which is the worst
# possible place to find out and the one place nobody is watching.
#
# WHAT IT CANNOT CHECK. That the names are *right* — only that the two files say
# the same thing. Both being wrong together passes, and the only cure for that is
# a release somebody downloads. `docs/releasing.md` covers it.
#
# WHAT WOULD HAPPEN IF THE THING THIS TESTS WERE BROKEN. `install.sh` would fetch
# a URL that 404s, having already told the user it was downloading. The failure
# arrives on someone else's machine, in a script that is piped into a shell.

set -euo pipefail
cd "$(dirname "${BASH_SOURCE[0]}")/.."

INSTALL=scripts/install.sh
RELEASE=.github/workflows/release.yml

fail=0
note() {
  echo "check-release-names: $1" >&2
  fail=1
}

# The platforms Glimpse publishes, and the archive shape each uses. This list is
# the thing being asserted about; if a platform is added it goes here first and
# the check then demands both files learn about it.
#
# Linux is a tarball because GTK comes from the distribution and a binary on the
# PATH is what the platform expects. macOS is a zip because it ships an .app
# bundle, and `ditto`/`unzip` preserve what is inside one (ADR 0013).
PLATFORMS="linux-x86_64:tar.gz macos-arm64:zip"

for entry in $PLATFORMS; do
  platform="${entry%%:*}"
  ext="${entry##*:}"

  grep -q -- "$platform" "$INSTALL" ||
    note "$INSTALL never mentions '$platform'; the installer cannot fetch it"
  grep -q -- "$platform" "$RELEASE" ||
    note "$RELEASE never builds '$platform'; the installer would fetch a 404"

  # The extension has to match too. An installer that appends .tar.gz to a name
  # the workflow published as .zip is a 404 that looks like a missing release.
  grep -q -- "$ext" "$INSTALL" ||
    note "$INSTALL never mentions the '$ext' extension used by $platform"
  grep -q -- "$ext" "$RELEASE" ||
    note "$RELEASE never produces a '$ext' for $platform"
done

# Both files build the name from the same three parts in the same order:
# <bin>-<version>-<platform>. Asserting the shape catches a reordering that the
# per-part greps above would each pass individually.
grep -q 'glimpse-\${version}-' "$RELEASE" ||
  note "$RELEASE no longer names artifacts glimpse-<version>-<platform>"
grep -q '\${BIN}-\${GLIMPSE_VERSION}-\${platform}' "$INSTALL" ||
  note "$INSTALL no longer builds the name as <bin>-<version>-<platform>"

# And a checksum for every artifact, because `install.sh` refuses to install
# without one. A release that publishes an archive and no `.sha256` is a release
# nobody can install through the documented path.
grep -q 'sha256' "$RELEASE" ||
  note "$RELEASE publishes no checksums, and install.sh refuses to install without one"

if [ "$fail" -ne 0 ]; then
  echo "check-release-names: the installer and the release workflow disagree." >&2
  echo "  Fix both, or the mismatch surfaces when a user runs the installer." >&2
  exit 1
fi

echo "  release names: install.sh and release.yml agree on ${PLATFORMS// /, }"
