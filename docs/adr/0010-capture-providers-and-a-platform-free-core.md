# 0010 — Capture providers, and a platform-free core

- **Status:** ACCEPTED for the crate split and the `GrabCommand` seam, both of
  which have landed. The `CaptureProvider` trait is deliberately **not written
  yet** — see "What is not decided here".
- **Date:** 2026-08-26
- **Amends:** [ADR 0002](0002-ffmpeg-pipeline-and-session-model.md)

## Context

[ADR 0002](0002-ffmpeg-pipeline-and-session-model.md) made v0.1 X11-only by
design rather than by omission, and closed with "do not generalise until a real
second backend exists." A second backend is now being proposed
([issue #1](https://github.com/viniciusdc/glimpse/issues/1)), so that condition is
what this record has to weigh.

macOS is not the case ADR 0002 argued against. The Wayland objection is that a
compositor which mediates screen selection cannot host a window that decides its
own capture rectangle. On macOS a window *can* read its own frame in global screen
coordinates, and the screen can be captured and cropped to it. The framing model
survives.

What does not survive is the hole. GTK4's macOS backend hard-codes input shapes
off, and AppKit has no regional input shape at all — `ignoresMouseEvents` is a
whole-window property. So the macOS frame is four border windows with an uncovered
middle, where the X11 frame is one window with its input region punched.

Two different window models, one recording pipeline.

## Decision

**A platform-free core, as a crate.** `glimpse-core` holds the session state
machine, encoding, the worker threads, configuration, the workspace and process
lifecycle, and `ScreenPixelRect`. Its manifest names no `gtk4`, no `x11rb` and no
`objc2`.

**The frame is a crate boundary, not a trait.** The window model is chosen at
compile time and there is exactly one per binary. `glimpse-x11` owns the GTK
window, the punched input region and the widget-to-pixels chain; `glimpse-macos`
will own the four-window composition. The root crate keeps the binary and selects
a frontend by target.

**What crosses the seam is data, not a trait: `GrabCommand`.** A frontend turns a
`GrabRequest` into ffmpeg input arguments, an optional video filter, and the
source's native pixel format. Core appends the output half — codec, container,
staging — and owns the child process.

**The capture rectangle carries a stated coordinate convention.** Global device
pixels, top-left origin, y increasing downward, asserted by a test rather than
left to a doc comment.

**Providers will be compiled in, never loaded.** No `dlopen`, no `cdylib`, no
discovery from disk.

## Because

**The boundary should be enforced by the compiler, not by this document.**
Nothing previously stopped `geometry.rs` importing GTK except `AGENTS.md` — and
`AGENTS.md` is a list of rules that each cost something the first time. A crate
whose manifest has no `gtk4` line physically cannot drift back into GTK. This is
the same move the codebase already makes twice: the capture target paints nothing
so no inset constant is ever needed, and `window_xid` is one choke point so a
deprecation is one edit. Structure over discipline.

**A trait belongs where there is a runtime choice.** The window model has none:
GTK-versus-AppKit is decided when the binary is built. A trait over it would buy
no dispatch and would have to be wide enough to describe two very different UIs,
which in practice means it gets shaped like whichever one was written first.

The runtime choice that *does* look real is on macOS, between `avfoundation` and
`ScreenCaptureKit` — Apple has pointed at the latter since macOS 12.3, so a binary
would plausibly compile in both and pick by OS version at startup. This record
originally justified runtime selection with X11-versus-Wayland on one Linux box;
that example is withdrawn, because ADR 0002 argues Wayland is a different product
rather than a deferred backend, and citing a backend the project has ruled out is
not an argument.

**`GrabCommand` is a struct because the backends disagree about where the region
goes.** `x11grab` takes it on the input as `-grab_x`/`-grab_y`. `avfoundation` has
no such option: it captures a whole display and the region comes from a `crop`
filter. An interface returning a flat argument list can express the first and not
the second, so `filter: Option<String>` is the entire reason this is not a
`Vec<String>`.

**The pixel format travels with the command for the same reason.** ADR 0002 is
careful to call the intermediate lossless only because x11grab emits `bgr0` and
ffv1 stores `bgr0`, so nothing is converted. That is a fact about the *source*, not
about Glimpse, and a second backend will have a different answer. Hard-coding
`-pix_fmt bgr0` in core would have quietly made the claim false on macOS.

**Dynamic plugins are refused on two grounds.** Rust has no stable ABI, so a
loadable provider means a C interface and its maintenance. And ffmpeg flag
construction is precisely the code
[ADR 0003](0003-apache-2-0.md) requires to stay in-tree and auditable — moving it
behind a plugin boundary would put the licensing basis of this project somewhere
it cannot be reviewed.

**The core split is worth doing even if macOS is abandoned.** It is true under
every version of the provider design, and it lets CI build and test the core on
macOS before a line of macOS UI exists — which is the cheapest available guard
against the core quietly re-acquiring a Linux assumption while a frontend is
written. It found two such assumptions immediately; see Consequences.

## What is not decided here

**Whether a capture provider yields ffmpeg arguments or a finished recording.**
`GrabCommand` is ffmpeg-shaped, which fits `x11grab` and `avfoundation` and
excludes `ScreenCaptureKit` — an in-process API that produces frames, not a child
to spawn. Adopting ScreenCaptureKit would mean widening the seam to "a provider
returns a `CapturedVideo`", moving process ownership behind it.

That is left open deliberately rather than guessed at. Check 3 of the macOS spike
— capture the computed rect and *look at the PNG* — has not been run, so the
macOS side of any interface is still a hypothesis, and an interface can agree with
one real backend and one imagined one while still being wrong. That is the shape
of mistake [ADR 0000](0000-x11-framing-window-spike.md) exists to record.

## Consequences

Two Linux assumptions were sitting in code that looked portable, and the split
surfaced both:

`process_is_alive` answered "alive" unconditionally off Linux, so
`sweep_stale_workspaces` skipped every candidate and removed **nothing** on
macOS — a temp directory leaked per killed session, permanently and silently. It
now uses `kill(pid, 0)`. Writing the test for it exposed a second edge: POSIX
gives pid 0 the meaning "every process in my process group", so `kill(0, 0)`
succeeds and a `glimpse-0-*` directory would have been immortal.

`die_with_parent` is still a no-op off Linux and this remains a real gap, not a
stub. macOS has no `PR_SET_PDEATHSIG`, so `SIGKILL`ing Glimpse there orphans a
recording ffmpeg — which is exactly why the sweep had to start working.

`require_x11_display`'s hard error becomes an availability question once providers
exist. Today a Wayland session gets a paragraph explaining that Glimpse is
X11-only; under providers it gets "no capture provider claims this session".

`scripts/sync-docs.sh` matched sources against the README layout block by
basename. With one crate per platform there are now several `src/geometry.rs`, so
it matches on the full resolved path instead — a basename match would let one go
undocumented while its namesake covered for it.

`make` gates cannot use `--workspace` off Linux, because `gdk4-x11` cannot build
against a Quartz-backend GTK. They select packages by `uname` instead, with
`glimpse-core` in both lists.

Release artifacts and CI currently build `linux-x86_64` on `ubuntu-latest` only.
The core's macOS job comes first and is cheap; a macOS artifact waits on a
frontend existing.

ADR 0002's refusal of a `CaptureBackend` trait is amended rather than overturned.
Its reasoning — that a trait over two things which are not interchangeable becomes
a bag of optional methods — is why the frame does not get one here, and why the
capture trait is not being written yet.

## What would falsify this

Check 3 showing a misaligned capture would kill the four-window model, and with it
the second provider that justifies the trait. It would not touch the core split,
which stands on its own.

A GTK4 macOS backend that cannot yield its `NSWindow`, or cannot set window level
and suppress the shadow, would also end the model — a shadow takes no events, so
the spike's hit test passes it, but a shadow *is* captured and would show in check
3 as a dark band.

## Corrections to the first draft of this record

It claimed the split would make the widget-to-pixels chain testable without an X
server. It does not: stages 1–3 call `compute_bounds`, `surface_transform` and
`scale_factor`, all of which need a *realized* GTK window, before and after the
split alike. What is display-free is the clipping arithmetic, which was already
tested. The chain remains verified only under `make selftest-headless`.
