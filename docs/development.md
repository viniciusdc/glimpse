# Development

## Setting up

Requires stable Rust, an **X11 session**, and:

```sh
sudo apt install libgtk-4-dev ffmpeg
```

`ffmpeg` is a runtime dependency, not a build one — but nothing records without
it. `make check-reqs` reports what is missing.

Wayland is not supported and this is by design, not omission — the binary says
so and exits rather than misbehaving.

## The gates

```sh
make            # list every target
make check      # docs-check, fmt, clippy, test — fastest-failing first
make test       # tests only
```

## What CI runs

Three jobs, in parallel, so the wall clock is the slowest one rather than the sum:

| job | needs | measured | why it is separate |
|---|---|---|---|
| **Docs** | nothing | 5s | No toolchain, no system libraries, and doc drift has been this project's largest category of finding |
| **Format** | rustfmt | 15s | Fails in seconds rather than behind a compile |
| **Clippy and tests** | GTK4 headers, ffmpeg | 60s | The long pole |

Those numbers are measured, and two of them are worth knowing.

**The old single job took 46–57s and skipped every ffmpeg test.** The new one takes
60s *and runs them*, so the comparable figure is "same wall clock, real encoder
coverage" rather than a slowdown.

**Queueing dominates.** In the run those timings come from, 112s elapsed between
the run being created and any job starting — nearly twice the work itself. That is
GitHub-side and not something this repository controls; it is also why a push can
appear not to have triggered CI at all when it has merely not started yet.

Getting there took two wrong turns worth recording. Splitting the job renamed it,
and the Rust cache is keyed on job name, so the first run after the split rebuilt
from cold. And installing ffmpeg unconditionally cost 109s of a 189s job — more
than clippy and the tests combined — until the step was changed to check what the
runner already has before paying for it.

**ffmpeg is installed in CI on purpose.** The media tests self-skip when it is
absent, and CI did not install it for a long time — so every encoding and
snapshot test silently skipped and those paths had no coverage at all while the
badge stayed green. The test job now asserts the media tests actually ran, because
a suite that skips is indistinguishable from a suite that passes.

[`releasing.md`](releasing.md) covers cutting one. `Release` builds a tagged Linux binary, running the full suite against the exact
tree being shipped first. **There is no macOS or Windows build**, and that is a
design consequence rather than a gap — see below.

## Why there is no macOS or Windows build

Glimpse works by being a window that knows where it is on screen and declares its
own capture rectangle. Wayland's portal model, macOS and Windows all deliberately
refuse that. It also links `gdk4-x11` and `x11rb`, and shells out to ffmpeg's
`x11grab`.

A build for those platforms would not be a port. It would be a different
application that happened to share a name — which is the same reasoning that keeps
Wayland out ([ADR 0002](adr/0002-ffmpeg-pipeline-and-session-model.md)).

The genuinely portable code — the session state machine, settings, the encoder's
argument construction and collision handling — could be compiled and tested
elsewhere by splitting the crate. That has not been done, because it would test
logic that already has no platform-specific behaviour, at the cost of a split that
exists only to serve CI.

## Regenerating the README animation

```sh
make demo
```

Draws 360 frames and assembles them through Glimpse's own pipeline. **It takes
six to seven minutes**, almost all of it ImageMagick rendering SVG — it has not
hung. `--fps` and `--scale` trade size against smoothness; the default lands at
roughly 310 KB.

The frame is deliberately tight around the window. An earlier cut showed a whole
desktop with Glimpse as one element among several, and the controls — the thing
the animation exists to show — came out too small to read. Whatever else changes,
the app should stay the subject.

## Keeping the docs honest

Documentation drift is invisible to the compiler, and it was the largest category
of finding in this project's first review — three published claims did not match
the code. So it is mechanical now:

```sh
make docs-sync   # regenerate generated sections, report anything it cannot fix
make docs-check  # change nothing, fail on drift (runs in make check and in CI)
make docs        # build the API documentation with rustdoc
```

`scripts/sync-docs.sh` **generates** the ADR index in the README from each ADR's
own heading, and **verifies** what cannot be generated without losing prose:

- every path in the README layout block exists;
- every module under `src/` appears in that block;
- every `` `make <target>` `` mentioned in any `.md` is a real target;
- every relative link in the docs resolves to a real file.

Add an ADR and `make check` fails until the index is regenerated. Rename a module
and it fails until the README agrees. That is the intent: the docs cannot quietly
fall behind the code.

## Installing

```sh
make install                   # ~/.local/bin + a desktop entry
make install PREFIX=/usr/local # or anywhere
make uninstall
```

Builds run under `nice -n 19` with `-j 2`, on the assumption that you are using
the machine for something else while they run. Override `NICE=` and `JOBS=` if
you would rather they did not.

## Working off-screen

Glimpse is a screen recorder, so testing it naturally means launching windows,
moving the pointer and grabbing the screen — on the machine someone is using.
Don't. There is a private X server for that:

```sh
make headless            # run Glimpse on its own display
make selftest-headless   # geometry + input region, off-screen
make smoke               # record -> GIF and record -> MP4, off-screen
```

`scripts/headless.sh` starts an `Xvfb` on `:99` (override with
`HEADLESS_DISPLAY`), waits for it to actually accept connections rather than
sleeping a guessed interval, runs your command against it, and tears it down.
It refuses to start if that display is already in use.

**What it cannot check**, because Xvfb has neither a window manager nor a
compositor:

- **Moving and resizing.** `begin_resize` sends `_NET_WM_MOVERESIZE` and there is
  no window manager to receive it. Resize has to be verified on a real session —
  and by hand, since a synthetic drag once produced a confident false negative
  here.
- **Transparency.** Nothing composites, so the hole shows the root window instead
  of what is behind Glimpse. Geometry and the input region are still verified
  exactly, because both are checked against the X server rather than by eye.

Everything else behaves identically: geometry, the input-region hole, recording,
encoding, collision handling and cleanup.

## Verifying the child dies with the parent

`Drop` covers every exit path the process controls; `SIGKILL` is not one of them.
ffmpeg is spawned with `PR_SET_PDEATHSIG`, and that is worth re-checking after any
change to how the recorder is spawned:

```sh
scripts/headless.sh bash -c '
  count() { pgrep -x ffmpeg | wc -l; }
  echo "start:     $(count)"
  GLIMPSE_SELFTEST=record ./target/debug/glimpse >/dev/null 2>&1 &
  sleep 4;  echo "recording: $(count)"
  kill -9 $!
  sleep 2;  echo "after:     $(count)"'
```

Expect `0 / 1 / 0`. Use `pgrep -x`, not `pgrep -f` — an `-f` pattern matches the
test script's own command line and quietly inflates every count, which is how the
first version of this check reported a surviving process that did not exist.

The session's temp directory does still survive a hard kill; see
[ADR 0005](adr/0005-gif-encoding-and-the-atomic-commit.md).

## Environment variables

Every variable the binary reads, in one place.

| Variable | Effect |
|---|---|
| `GLIMPSE_DECORATIONS=server` | Hand the window frame back to the window manager. Glimpse normally draws its own chrome *and its own resize edges* — use this if resizing misbehaves on your compositor ([ADR 0006](adr/0006-the-header-is-the-chrome.md)). |
| `GLIMPSE_SELFTEST=1` | Probe geometry and the input region, write a capture to `/tmp/glimpse-selftest.png`, then exit. |
| `GLIMPSE_SELFTEST=record` | Drive a real Record → Stop cycle through the same path the button uses, print the final state, then exit. |
| `GLIMPSE_SELFTEST=record-mp4` | The same, encoding to MP4 instead of GIF. |
| `GLIMPSE_TRACE=1` | Print every state transition, effect and background result. Added after a debugging session in which a hand-added print silently failed to apply, and its absence was read as evidence about the program. |
| `GLIMPSE_DEBUG_GRIPS=1` | Paint the invisible resize grips magenta. They are the easiest thing in the codebase to break without noticing, because nothing renders when they are wrong. |

## Verifying geometry changes

Any change to `geometry.rs` or the widget hierarchy in `ui.rs` must be checked
against a real capture, not just the test suite:

```sh
make selftest            # on your screen
make selftest-headless   # on a private X server
```

Both go through `scripts/selftest.sh`, which turns the report below into an exit
status. Run `GLIMPSE_SELFTEST=1 cargo run` directly and you get the report with
no exit status at all — fine when you are reading it, misleading in a `&&` chain.

A passing run looks like this — the shape verdict is semantic, not a band count:

```
=== glimpse self-test ===
capture rect : 754x438 at 3,118
xid          : 0x9c00004
xwininfo     : window origin 0,62
input shape  : PASS — hole is click-through, border takes clicks
                 4 band(s): 760x56+0+0 3x438+0+56 3x438+757+56 760x26+0+494
grab         : wrote /tmp/glimpse-selftest.png — INSPECT IT: ...
```

The four bands are the window minus the hole: a top band down to the hole, a 3px
left and right border, and a bottom band. `window origin 0,62` plus the hole's
offset of 56 is the capture `y` of 118 — the arithmetic should reconcile, but that
reconciling is necessary, not sufficient.

Then **look at** `/tmp/glimpse-selftest.png`. If any part of Glimpse's own
interface appears in it — frame border, toolbar, status bar — the rectangle is
wrong, regardless of what the printed numbers say.

The harness deletes that PNG before every run and refuses the run if one did not
come back. Without that, a failed grab leaves the *previous* run's image on disk,
and the instruction above sends you to inspect a correct picture of a question
nobody asked — the failure that looks most like success. It also refuses a FAIL
shape verdict and a run that printed no report, both of which used to exit 0.

This is not belt-and-braces. During the spike the computed rectangle agreed with
`xwininfo` to the pixel and was still wrong by the 3px border width; only the
image showed it. [ADR 0000](adr/0000-x11-framing-window-spike.md) has the detail.

Equally: the image is a smoke test, not an oracle. A red band along the top of a
grab once looked exactly like the frame border bleeding in. Temporarily changing
the border colour and re-grabbing showed the band did not change — it was content
in the page underneath. When the picture looks wrong, *establish* what it is.

**The PNG is a picture of your screen.** Never attach it to a pull request.

## Verifying the recording path

```sh
GLIMPSE_SELFTEST=record       # to GIF
GLIMPSE_SELFTEST=record-mp4   # to MP4
```

Drives a real Record → Stop cycle through the same code path the button uses,
then prints the final state and where the output went. A passing run ends in
`Completed { output: ... }` and leaves no workspace behind in `/tmp`.

Check for leftovers as well as for the file. A successful encode once leaked its
recording directory every single time while producing a perfectly correct output
— nothing failed, so only listing `/tmp` afterwards revealed it.

## Verifying click-through

Read the input shape back from the X server:

```sh
cargo run -p glimpse-x11 --example root_geometry -- $(xdotool getactivewindow)
```

Needs `xdotool`, which is optional and only used for this check; `make check-reqs`
reports whether it is present. The self-test performs the same verification
without it.

**Do not test click-through by moving the pointer and asking what is underneath.**
`XQueryPointer`'s child field is geometric and blind to the input shape; it
reports the framing window either way. That method produced a confident false
negative during the spike, caught only by a control probe.

## Tests

```
glimpse-core/src/geometry.rs  unit tests on clipping
tests/geometry.rs integration tests over the public API
```

Clipping is the part of the chain that is testable without a display, and it is
the part that protects ffmpeg from being handed an impossible rectangle. The rest
of the chain needs a live X server and is covered by the self-test above.
