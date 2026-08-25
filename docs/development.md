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
make check      # fmt, clippy, test — fastest-failing first
make test       # tests only
```

Builds run under `nice -n 19` with `-j 2`. The developer is usually using this
machine; a build should yield rather than compete with it.

## Verifying geometry changes

Any change to `geometry.rs` or the widget hierarchy in `ui.rs` must be checked
against a real capture, not just the test suite:

```sh
GLIMPSE_SELFTEST=1 cargo run
```

Then **look at** `/tmp/glimpse-selftest.png`. If any part of Glimpse's own
interface appears in it — frame border, toolbar, status bar — the rectangle is
wrong, regardless of what the printed numbers say.

This is not belt-and-braces. During the spike the computed rectangle agreed with
`xwininfo` to the pixel and was still wrong by the 3px border width; only the
image showed it. [ADR 0000](adr/0000-x11-framing-window-spike.md) has the detail.

## Verifying click-through

Read the input shape back from the X server:

```sh
cargo run --example root_geometry -- $(xdotool getactivewindow)
```

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
