# 0012 — A setting a backend cannot honour is not offered

- **Status:** ACCEPTED
- **Date:** 2026-08-28
- **Relates to:** [ADR 0010](0010-capture-providers-and-a-platform-free-core.md)

## Context

Glimpse has a **Capture pointer** switch in the settings popover. It persists to
`config.toml` and travels `Config` → `GrabRequest` → the backend, which turns it
into an ffmpeg flag.

On X11 that flag is `-draw_mouse`, it is stated explicitly in both directions,
and a test asserts both. It works.

On macOS it is `-capture_cursor`, and **avfoundation ignores it**. Measured, not
inferred: against a static window with the pointer parked in it and a cursor
provably rendered there, three runs gave a noise floor of 0 and a signal of 0,
and the resulting frame was an exact match for a cursor-free reference capture
(1.2e7 differing pixels against a with-cursor one). It is ignored in the OFF
direction — the pointer is never drawn, whatever the flag says.

So on macOS the switch would flip, persist across restarts, and change nothing.
The only place that would ever surface is the user's own recording.

The reason the program cannot warn about it is narrow. `GrabCommand` carries
`rect`, `input`, `filter` and `pix_fmt` — everything about what to *do*, and
nothing about what was *refused*. `glimpse-core` never learns the request was
dropped, so the UI has nothing to say.

## Decision

**A setting a backend cannot honour is not offered on that platform.** The macOS
frontend does not show the Capture pointer switch. No channel is added to
`GrabCommand` for reporting an unhonoured request.

The flag stays emitted in both directions in `glimpse-macos`. It costs one
argument, and an ffmpeg that starts honouring it would need no code change.

## Because

**Nothing promised cannot lie.** The alternative — a way for a backend to report
"I could not honour this", so core knows and the UI can grey the switch out — is
a new concept in the seam designed around a single case. [ADR 0010](0010-capture-providers-and-a-platform-free-core.md)
and `AGENTS.md` both say not to widen the seam before there is more than one
backend asking for it, and one measured instance is not that.

**It is the move this codebase already makes.** The frame border went on the
parent widget so no inset constant could exist. The encode overload without a
canceller was deleted rather than documented. The format `Cell` was deleted so
two sources of truth could not disagree. Each time the answer was to make the
mistake unpronounceable rather than reportable, and a control that is absent
cannot mislead.

**The timing is free.** There is no macOS frontend yet, so this is a line in a UI
that has not been written. Deciding after writing it would be rework.

## Costs

**A macOS user who wants the pointer in the recording cannot have it.** This is
the real loss and it is not symmetric with the reason for accepting it. Not
offering the switch is comfortable for someone who does not want the pointer
captured — on macOS that is already all they can get. It does nothing for someone
who does want it, and that person is simply unserved, with no setting to find and
no message explaining why.

**The absence has to be explained somewhere the user will look**, or it becomes a
different confusion: a feature present on one platform and silently missing on
another reads as a broken build. `docs/faq.md` is the place, not a tooltip on a
switch that is not there.

**`capture_mouse` still exists in `config.toml` on macOS.** Hiding the switch does
not remove the key, so hand-editing the file still produces a setting that does
nothing. That is a smaller lie than a switch in the UI — the file is not a
promise the program made — but it is not zero.

**If a second unhonourable setting appears, this decision should be revisited
rather than repeated.** Two platforms silently dropping two different settings is
the point at which "do not offer it" stops scaling and the seam earns its place.

## What would falsify this

An ffmpeg release honouring `-capture_cursor` on avfoundation, or a move to
`ScreenCaptureKit`, which composites the cursor on its own terms. Either makes the
switch honourable on macOS and this record moot. The flag is still emitted, so
that day costs nothing but showing the switch again.

A second setting that some backend cannot honour. See Costs.
