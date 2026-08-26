# Releasing

A release is one Linux x86_64 tarball attached to a GitHub release, plus its
checksum. There is no macOS or Windows build and there will not be — see the
[FAQ](faq.md#are-there-macos-or-windows-builds).

## Before you tag

```sh
make check       # docs, formatting, clippy, tests
make smoke       # record → GIF and record → MP4, off-screen
```

Then the things no gate can check:

- **Look at the app.** `make run`, record something, open the file. The suite
  cannot tell you the output looks wrong, only that one was produced.
- **If the interface changed, regenerate the animation** — `make demo`, then
  look at `docs/assets/demo.gif`. A README showing an interface that no longer
  exists is worse than one showing none.
- **Read the README as a stranger would.** Structural drift is caught by
  `make docs-check`; prose that has quietly stopped being true is not.

## Cutting it

The version lives in `Cargo.toml` and the tag is that version with a `v` in
front. They must agree — the release refuses to build otherwise, because a tag
and a crate version that disagree ship an artifact named one thing built from a
crate that calls itself another, and `glimpse --version` then contradicts the
file the user downloaded.

```sh
# 1. bump the crate version
$EDITOR Cargo.toml            # version = "0.2.0"
cargo build                   # updates Cargo.lock; commit both

git commit -am "Release 0.2.0"
git push

# 2. wait for CI to be green on main — the release builds from the tag, so a
#    broken main becomes a broken release

# 3. tag and push it
git tag -a v0.2.0 -m "0.2.0"
git push origin v0.2.0
```

Pushing the tag starts the `Release` workflow. It runs the **whole suite against
the exact tree being shipped** before building anything, then produces:

```
glimpse-v0.2.0-linux-x86_64.tar.gz          binary, desktop entry, README, LICENSE, NOTICE
glimpse-v0.2.0-linux-x86_64.tar.gz.sha256
```

and attaches both to the GitHub release for that tag.

## Afterwards, verify it like a user

Do not assume the artifact is good because the workflow was green.

```sh
curl -fsSL https://raw.githubusercontent.com/viniciusdc/glimpse/main/scripts/install.sh \
  | GLIMPSE_VERSION=v0.2.0 INSTALL_DIR=/tmp/verify sh
/tmp/verify/glimpse --version      # must print 0.2.0
```

That exercises the same path a stranger takes, including the checksum
verification, and catches the failure mode that matters most here: the installer
and the release workflow disagreeing about what the archive is called. They agree
today — `scripts/install.sh` builds the same name `release.yml` writes — but
nothing enforces it, so **changing the archive name in one file breaks installs
until the other is changed too.**

## Versioning

Semver, and pre-1.0 while the interface is still moving: breaking changes bump
the minor, everything else bumps the patch. A settings file written by an older
version must keep loading — `Config` fills in unknown fields with defaults and
never fails, and that is a promise rather than an implementation detail.

## Release notes

Say what changed for someone using it, not what changed in the tree. The commit
log already holds the second kind.

Worth calling out explicitly:

- anything that changes where files are written, or what they are called
- anything that changes the settings file's meaning
- known problems, in the release notes rather than only in an ADR

## If a release is wrong

Do not retag. A tag that has been pushed and downloaded should keep meaning what
it meant. Fix forward: bump the patch version, tag again, and if the bad artifact
is actively harmful, delete its release on GitHub so `latest` stops resolving to
it — the installer asks the API for `releases/latest`, so removing it is enough
to stop new installs.
