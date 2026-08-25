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

Builds run under `nice -n 19` with `-j 2`. The developer is usually using this
machine; a build should yield rather than compete with it.

## Verifying geometry changes

Any change to `geometry.rs` or the widget hierarchy in `ui.rs` must be checked
against a real capture, not just the test suite:

```sh
GLIMPSE_SELFTEST=1 cargo run
```

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
GLIMPSE_SELFTEST=record cargo run
```

Drives a real Record → Stop cycle through the same code path the button uses,
then prints the final state and the recorded file. Until GIF encoding exists the
run ends in `Failed { retryable: Some(..) }` **on purpose** — the session takes
the same path a real encoder failure would, so the preserved-artifact policy is
exercised for real rather than only in tests. The recording is left on disk; it
tells you where.

## Verifying click-through

Read the input shape back from the X server:

```sh
cargo run --example root_geometry -- $(xdotool getactivewindow)
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
src/geometry.rs   unit tests on clipping
tests/geometry.rs integration tests over the public API
```

Clipping is the part of the chain that is testable without a display, and it is
the part that protects ffmpeg from being handed an impossible rectangle. The rest
of the chain needs a live X server and is covered by the self-test above.
