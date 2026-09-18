# Releasing

A release is two artifacts attached to a GitHub release, each with its checksum:

| platform | artifact | shape |
|---|---|---|
| Linux x86_64 | `glimpse-<version>-linux-x86_64.tar.gz` | a bare binary |
| macOS arm64 | `glimpse-<version>-macos-arm64.zip` | `Glimpse.app` |

They are different shapes on purpose. GTK comes from the distribution on Linux
and a binary on the PATH is what that platform expects; macOS carries its own GTK
inside a bundle, because Screen Recording permission attaches to an application
identity and a bare binary has none ([ADR 0013](adr/0013-macos-ships-an-app-bundle.md)).

There is no Windows build. Nobody has measured anything there, so read its
absence as unexamined rather than settled — see the
[FAQ](faq.md#are-there-macos-or-windows-builds).

The macOS bundle is **ad-hoc signed, not notarized**. Downloaded through a
browser it is quarantined and macOS calls it damaged; fetched with `curl`, as
`install.sh` does, it is not. That asymmetry is a trap for whoever writes the
install instructions, and it is recorded rather than solved.

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

On macOS the installer puts `Glimpse.app` in `~/Applications` rather than a
binary on the PATH, so verify it there — and **open it from Finder at least
once**, because that is what gives Screen Recording permission something to
attach to that is not your terminal.

That exercises the same path a stranger takes, including the checksum
verification.

The failure mode it used to be the only guard against — the installer and the
release workflow disagreeing about what an archive is called — now fails the
build instead. `scripts/check-release-names.sh` asserts that both files name the
same platforms with the same extensions and build the name the same way, and
`make check` runs it. Adding macOS turned one such coupling into four, which is
what finally made an assertion cheaper than the prose warning that used to be
here.

What that check *cannot* tell you is whether the names are right — only that the
two files agree. Both being wrong together still passes, and the only cure is the
download above.

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
