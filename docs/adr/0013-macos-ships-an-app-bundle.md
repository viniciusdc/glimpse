# 0013 — macOS ships an `.app` bundle

- **Status:** ACCEPTED. The dylib handling was unverified when this was written
  and has since been measured — see "The dylib handling, measured".
- **Date:** 2026-08-28
- **Relates to:** [ADR 0011](0011-why-the-macos-frame-is-more-than-one-window.md)

## Context

Linux ships a bare binary. `make install` puts `target/release/glimpse` on the
path and drops a `.desktop` entry beside it, and `scripts/install.sh` unpacks a
`glimpse-<version>-linux-x86_64.tar.gz` and does the same. That works because two
things are true on Linux and neither is true on macOS.

**GTK is dynamically linked.** `ldd` on the release binary resolves
`libgtk-4.so.1` from the system, which distributions ship. On macOS GTK4 comes
from Homebrew, and the prefix differs by architecture — `/opt/homebrew` on Apple
Silicon, `/usr/local` on Intel. A downloaded binary either finds the dylibs it was
built against or does not start.

**Screen Recording permission attaches to an application identity.** macOS grants
it to the *responsible process*. A bare binary launched from a terminal inherits
the terminal's grant, which is why running the spike reads as a bug in the app
rather than a permission prompt: the user grants Terminal, not Glimpse, and a
different terminal needs granting again. Launched from Finder, a bare binary has
no bundle identifier for the grant to persist against at all.

A screen recorder whose permission story is "grant it to whatever launched me" is
not shippable to anyone but its author.

## Decision

**macOS ships a `.app` bundle.** The binary lives at `Contents/MacOS/glimpse`, the
GTK dylibs it needs are bundled under `Contents/Frameworks` with load paths
rewritten to `@executable_path/../Frameworks`, and `Contents/Info.plist` carries
the bundle identifier that Screen Recording is granted to.

Linux is unchanged. The `.desktop` entry stays Linux-only, as it already is in the
`install` target.

## Because

**The permission is the deciding argument, not the dylibs.** Bundling dylibs can
be done in a plain tarball — set the install names to `@executable_path/../lib`
and ship them alongside — so that problem alone would not force a bundle. TCC
does. There is no way to give a bare binary a stable identity for a permission to
be granted to.

**It is the platform's own answer.** A GUI application on macOS is a bundle. Every
other route ends up reimplementing part of one: an identity for TCC, a place for
the dylibs, an icon, a way for Finder to launch it.

**It keeps the door open for Homebrew.** A cask installs a `.app`; a formula
installs a CLI. Deciding the bundle now does not commit to a distribution channel,
and picking the other shape would have closed the cask off.

## Costs

**Gatekeeper, and the honest version of it.** An unsigned bundle downloaded
through a *browser* is quarantined and refused, with a message that reads as
"this app is damaged". Downloaded with `curl`, quarantine is never set, so the
`install.sh` path works unsigned while clicking the release asset on GitHub does
not. That asymmetry is a trap for whoever writes the install instructions: the
documented path can work perfectly while the obvious path fails.

Signing and notarizing removes it and costs an Apple Developer account. That is a
real recurring cost and it is deferred, not solved, by this record.

**The release surface doubles, and it already had an unverified coupling.**
`release.yml` builds `Linux x86_64` only. `install.sh` *constructs* the archive
name that `release.yml` *writes*, and `docs/releasing.md` already flags that
nothing verifies the two agree. Adding macOS turns one such coupling into
several, across two architectures. The fix is a check that asserts the names
match, not a second hardcoded string.

**`glimpse --version` moves.** `docs/releasing.md` verifies a release the way a
user would, by installing it and running `glimpse --version`. Inside a bundle the
executable is at `Contents/MacOS/glimpse`, so either the verification learns that
path or the install drops a shim on `PATH`. The CLI surface exists precisely so a
release can be checked; it should not quietly stop being reachable.

**`make install` grows a second shape.** Today it installs one file and, on Linux,
one `.desktop`. A bundle is a directory tree that has to be assembled, which is
more than `install -m 755` and is the kind of thing that rots silently when only
one platform is exercised.

## The dylib handling, measured

The two commands above have now been run, against a Rust binary linked to
Homebrew GTK4 on Apple Silicon. The release binary could not be the subject:
`glimpse` on macOS links no GTK at all, because there is no frontend yet. A spike
binary that does link it stands in.

**The absolute paths are real and there is no rpath.**

```
/opt/homebrew/opt/gtk4/lib/libgtk-4.1.dylib
/opt/homebrew/opt/pango/lib/libpangocairo-1.0.0.dylib
/opt/homebrew/opt/glib/lib/libglib-2.0.0.dylib
...
otool -l | grep LC_RPATH  ->  nothing
```

So every install name has to be rewritten with `install_name_tool`. There is no
rpath to redirect and no shortcut.

**The closure is larger than the direct list, as suspected:**

| | |
|---|---|
| direct Homebrew dylibs | 12 |
| transitive closure | **39 dylibs, 25.6 MB** |
| distinct packages | 29 |

Beyond the obvious `glib`, `pango`, `cairo` and `harfbuzz`, it reaches
`freetype`, `fontconfig`, `pixman`, `libpng`, `jpeg-turbo`, `libtiff`, `pcre2`,
`zstd`, `xz`, `lzo`, `fribidi`, `libthai`, `libdatrie` and `graphite2`.

**Eight of the thirty-nine are X11 client libraries**, on a Quartz build:
`libX11`, `libxcb` and two of its extensions, `libXau`, `libXdmcp`, `libXext`,
`libXrender`. Traced rather than assumed — Homebrew's **cairo** carries all six
X11 links, `libgtk-4.dylib` itself carries none, and `pkg-config --exists
gtk4-x11` is false, so the X11 backend is not even built. They are unreachable
code that would ship anyway.

At 1.6 MB they are 6% of the bundle and not worth engineering around. They are
recorded because finding X11 libraries inside a macOS app bundle looks like a
misconfiguration, and the next person should be able to confirm it is not one
without re-deriving the trace.

**What is still unverified** is that rewriting all 39 install names actually
produces a working bundle. That needs a bundle to exist.

## What would falsify this

A static or vendored GTK build, which would remove the dylib half entirely and
leave only TCC — still enough to require the bundle, but a much smaller one.

Apple changing how Screen Recording is attributed, so that a bare binary can hold
a durable grant. That would make the bundle optional rather than forced, and this
record moot.
