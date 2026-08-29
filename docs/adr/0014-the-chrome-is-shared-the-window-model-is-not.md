# 0014 — The chrome is shared, the window model is not

- **Status:** PROPOSED
- **Date:** 2026-08-28
- **Amends:** [ADR 0010](0010-capture-providers-and-a-platform-free-core.md)
- **Relates to:** [ADR 0006](0006-the-header-is-the-chrome.md),
  [ADR 0011](0011-why-the-macos-frame-is-more-than-one-window.md)

## Context

macOS has a frame and no controls. No size readout, no Record button, no format
chip, no menu, no status line, and none of the four visual states ADR 0006
defines. The X11 frontend has all of it.

The obvious reading is that the header must now be written twice. Counting says
otherwise:

```
crates/glimpse-x11/src/ui.rs   2248 lines
lines naming X11                  18
```

Eighteen. `X11Probe`, `punch_input_hole`, `shape_covers`, `xwininfo`,
`X11Capture`. Everything else is GTK widgets, the stylesheet ported verbatim
from the design document, and the wiring between the buttons and
`glimpse_core::session` — none of which has an opinion about the platform.

The header is not something to write for macOS. It is something to move.

## Decision

**A fourth crate, `glimpse-ui`,** holding the chrome: the palette and
stylesheet, the widget tree, and the session wiring that turns a button press
into a `session::Event` and an `Effect` into work.

**The window model stays in the frontends.** `glimpse-x11` keeps its single
shaped window and its input-region punching; `glimpse-macos` keeps its five
windows and their placement. Those are genuinely different and
[ADR 0011](0011-why-the-macos-frame-is-more-than-one-window.md) explains why.

**What crosses is a struct of closures, not a trait.** The chrome needs four
things from whichever frontend hosts it:

```rust
pub struct PlatformHooks {
    /// What a recording would capture, right now.
    pub capture_rect: Box<dyn Fn() -> Result<ScreenPixelRect>>,
    /// Turn a request into the backend's ffmpeg invocation.
    pub grab: Box<dyn Fn(&GrabRequest) -> Result<GrabCommand>>,
    /// The frame's geometry settled. X11 re-punches its input region; macOS
    /// repositions its strips. Neither is the other's business.
    pub geometry_settled: Box<dyn Fn()>,
    /// Platform diagnostics for the self-test, as text.
    pub diagnostics: Box<dyn Fn() -> String>,
}
```

## Because

**Duplicating the stylesheet would undo the reason it exists.** ADR 0006 records
that the design document's tokens are "ported verbatim into the CSS rather than
approximated, so the app and the mock cannot drift apart on colour". Two copies
of that file reintroduces precisely the drift the verbatim port was there to
prevent, and it would drift silently — nothing compares two stylesheets.

**Eighteen lines is not a platform abstraction, it is a seam.** A component that
is 99.2% portable and is nonetheless kept in a crate named after one platform is
mis-filed rather than platform-specific.

**Closures rather than a trait, for the reason ADR 0010 already gives.** Both
frontends are selected at compile time, so a trait buys no dispatch, and its
only other effect would be to make the chrome generic over a parameter that has
exactly one value per binary. A struct of closures is the same shape as
`GrabCommand`: the platform hands over data describing what to do, and the
shared code does it. It also keeps the hook list visible in one place rather
than spread across trait impls.

**The seam is derived, not invented.** Each hook exists because a specific line
in `ui.rs` needs it today: `capture_rect` for the probe held as a field, `grab`
for the three `X11Capture::from_env()` calls, `geometry_settled` for
`sync_input_region`, and `diagnostics` for the `xid`/shape/`xwininfo` block in
`run_selftest`. Nothing was added for symmetry or for a platform that does not
exist.

## Consequences

`ui.rs` moves and shrinks. The X11 frontend keeps `app.rs`, `x11probe.rs`,
`geometry.rs`, `grab.rs` and the input-region code, and gains a small module that
builds the hooks.

**The self-test is the awkward one.** It currently prints an `xid`, the shape
bands read back from the server, and an `xwininfo` cross-check — none of which
exists on macOS, where there is no shape to read because there is no window over
the hole. Hence `diagnostics` returning text rather than anything structured:
the two platforms have nothing to say to each other here, and pretending
otherwise would invent a common vocabulary for two unrelated facts.

`glimpse-ui` depends on `gtk4` unconditionally, so unlike `glimpse-core` it will
not build on a machine without GTK. That is honest — it is a GTK component — but
it means Linux CI no longer covers everything the macOS build compiles.

ADR 0010 said the frontend is a crate boundary with exactly one candidate per
binary. That stays true. This record only says the *chrome* inside those
frontends was never platform-specific and should not have been filed as though
it were.

## What would falsify this

A macOS header that genuinely diverges — different controls, a different
arrangement, a platform idiom the shared widget tree cannot express. Then the
shared crate becomes a set of `cfg`s pretending to be common code, and two
honest implementations would be better than one dishonest one.

The signal to watch: hooks accumulating. Four is a seam. If it reaches eight or
nine, the chrome is not shared, it is parameterised, and this should be revisited
rather than extended.
