# 0004 — Review corrections, and a lifecycle spine before capture

- **Status:** ACCEPTED
- **Date:** 2026-08-25
- **Corrects:** claims in [0000](0000-x11-framing-window-spike.md), [0001](0001-rust-and-gtk4.md), [0002](0002-ffmpeg-pipeline-and-session-model.md), [0003](0003-apache-2-0.md)

## Context

Three independent reviews ran against the repository before capture was written:
two on the code and docs, one adversarial on structure. They converged on more
than they disagreed on, and several findings contradicted claims this project had
already published about itself. Those corrections belong on the record rather than
in a commit message.

## Decision

### 1. Hexagonal architecture is rejected

All three reviews independently reached the same verdict, and it is adopted. At
roughly 520 lines, with one capture implementation, one encoder, one UI and one
platform, ports-and-adapters would add traits with a single implementor each and
indirection nobody swaps. The seams it would buy already exist more cheaply:
`RootPixelRect` is the entire contract handed to the capture side, and `X11Probe`
already isolates every server call.

The module shape adopted for the next phase is plain separation of concerns:

```
geometry.rs   coordinate conversion
x11probe.rs   the X11 platform boundary
capture.rs    the concrete ffmpeg recorder
encode.rs     the concrete ffmpeg GIF encoder
session.rs    state, artifact ownership, cancellation, transitions
ui.rs         widgets, and presentation of session events
main.rs       composition and shutdown ownership
```

No `CaptureBackend` trait, and no general encoder port, until a second
implementation actually exists.

### 2. The roadmap ordering is inverted — the lifecycle skeleton comes first

[`roadmap.md`](../roadmap.md) sequenced capture → encoding → state machine.
That is backwards. Stopping, cancellation, shutdown, artifact retention, geometry
unlocking and child reaping are not UI wrapped around a finished `Command`; they
determine how that command is owned and executed. Capture written first would
return a child handle or block on `wait`, and the state-machine phase would then
rewrite it.

The session state enum is pure, display-free and testable in CI today. It lands
first, with recording as the first vertical slice through it.

### 3. The published state machine cannot express what it promises

`Idle → Arming → Recording → Encoding → Completed | Failed` has no way to say:
ffmpeg has been sent `q` but has not finalised the container; capture cancelled
versus stopped normally; encoding cancelled; encoding failed but the source video
is retryable; shutdown arrived mid-termination. `RecordingSession.stop() →
CapturedVideo` is also deceptively synchronous — it hides a subprocess shutdown
that can fail.

At minimum the machine gains `Stopping` and a retryable captured artifact, and
state variants own the resources valid in that state rather than keeping state and
resources side by side.

## Corrections to earlier records

**ADR 0000 overstated its verification.** Three specific overclaims, all fixed in
code as part of this decision:

- Reading the shape back proves what the server stored, *not* that the intended
  hole was stored. The self-test treated any non-empty shape as success, so a
  misplaced hole passed. It now checks semantics — the hole must not take clicks,
  the border must — via `x11probe::shape_covers`.
- One PNG is a valuable smoke test, not a sufficient geometry oracle. It can miss
  timing errors, and chrome can be visually indistinguishable from content behind
  it. (This bit immediately: a red band in a grab was initially suspected to be
  our own border and turned out to be page content, established only by changing
  the border colour and re-grabbing.)
- `scale_factor == 1` proves the bogus 1mm × 1mm size was unused *in that run*. It
  does not validate scale 2 or mixed-scale monitors. Q3 is narrower than claimed.

The lesson is sharper than "numbers agreeing with numbers is never sufficient":
**an origin cross-check cannot tell you whether the widget bounds you chose are
the semantically intended capture area.**

**ADR 0001 kept an argument that its own successor invalidates.** It rejects Go
partly because "every encode would shell out to ffmpeg anyway, inheriting Peek's
exact quality ceiling" — which is precisely what ADR 0002 then chose for Rust.
Rust may still be right; that particular comparison no longer supports it and
should not be cited.

**ADR 0002's Wayland seam claim is wrong.** It said separating selection from
capture means "a Wayland selector could replace the former without touching the
latter." Not for this capture implementation: `x11grab` consumes an X11 root
rectangle, so a portal/PipeWire flow changes both halves. The X11-only decision
stands; the claim that only selection would change does not, and a contributor
who believed it would preserve an invalid `RootPixelRect → x11grab` contract.

**ADR 0003 overreached on one word.** "Impossible even in principle" is too
strong — GTK4 lacking a GTK3 API does not make transliteration impossible, and
architectural difference is not a legal test for derivation. The narrower factual
claim is the one that matters and the one to keep: no Peek expression or assets
were copied.

**ADR 0000's "exactly one function changes" was too confident.** Centralising
`xid()` is still right, but if GTK removes access rather than renaming it, the
fallback is a different window-ownership model — not a one-function edit.

## Also fixed in this pass

- **The frame was never actually locked.** `lock()` only called
  `set_resizable(false)`; a window manager can still move the window, and the
  `locked` flag was written and never read. Immobility is now a *checked
  invariant* — `geometry_drifted()` — rather than an assumption about what GTK can
  enforce. The dead flag is gone.
- **`std::mem::forget` on the window owner** made `lock`/`unlock` unreachable from
  the running application and would have made "reap ffmpeg on every exit path"
  unachievable through `Drop`. The application now retains its owner.
- **X11 was validated too late.** `X11Probe::new()` succeeding does not prove GTK
  is *using* X11 — under Wayland, XWayland answers on `$DISPLAY` while GTK selects
  its own backend, so the documented "exits under Wayland" behaviour did not
  happen. `x11probe::require_x11_display()` now checks the GDK backend at startup.
- **Startup refusal exited 0.** It now exits non-zero.
- **The input region ignored the surface transform** that `capture_rect` applies,
  and rounded differently. Dormant on a window with no CSD margins, latent
  everywhere else. Both now match.
- **A tick callback re-punched the input region every frame,** waking the app at
  monitor refresh rate forever. Replaced with the surface `layout` signal.
- **`geometry` depended on `ui`** for `window_xid`, inverting the layering.
  `window_xid` now lives beside `X11Probe` in the platform module.
- **`make check-reqs` could not report a missing ffmpeg,** because it piped
  through `head` — the exact failure the Makefile's own header forbids.

## Costs accepted

The lifecycle skeleton delays visible progress: the next commit produces a state
enum and transitions rather than a recording. That is the point — the alternative
is writing capture twice.
