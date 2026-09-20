# 0019 — A recording must not outlive a killed Glimpse

- **Status:** PROPOSED
- **Date:** 2026-09-20
- **Relates to:** [ADR 0010](0010-capture-providers-and-a-platform-free-core.md),
  [ADR 0002](0002-ffmpeg-pipeline-and-session-model.md),
  [issue #45](https://github.com/viniciusdc/glimpse/issues/45)

## Context

This closes a gap [ADR 0010](0010-capture-providers-and-a-platform-free-core.md)
names and leaves open, in its own words:

> `die_with_parent` is still a no-op off Linux and this remains a real gap, not a
> stub. macOS has no `PR_SET_PDEATHSIG`, so `SIGKILL`ing Glimpse there orphans a
> recording ffmpeg — which is exactly why the sweep had to start working.

On Linux the kernel enforces the child's lifetime: the recorder sets
`PR_SET_PDEATHSIG`, so ffmpeg dies with its parent whatever kills it.

Every *ordinary* way out is now covered in the application itself — the window's
close handler, and the application's `shutdown` signal, which catches Cmd-Q, the
menu and a journey ending (#55). `SIGINT` and `SIGTERM` are handled. What is left
is the one signal no process can handle: `SIGKILL`, and the Force Quit dialog
that sends it.

**Measured on this machine**, killing Glimpse with `-9` mid-recording:

```
app killed -9 while Recording.
ORPHAN: ffmpeg still running
recording.mkv grew 6553600 -> 26476544 bytes over 4s
```

Roughly 5 MB/s — the intermediate is lossless ffv1 by design (ADR 0002) — so
about **18 GB/hour**, into a temp directory, holding the screen capture device,
until the disk fills or Glimpse is started again.

There is already a backstop. `sweep_stale_workspaces` runs at start-up, finds a
capture whose owning pid is gone *by the workspace path in its argument list*,
kills it and removes the directory. That is what closed #45. It bounds the damage
but only at the next launch, which may be days away, and does nothing about the
capture device being held in the meantime — by which time the user's evidence is
"my disk filled up", not "Glimpse leaked a process".

## What was eliminated, and why

**Rely on stdin EOF.** The recorder already gives ffmpeg a stdin pipe, because
`stop` writes `q` into it. A killed parent closes that pipe, so ffmpeg gets EOF
for free with no new machinery at all. This is the cheapest fix imaginable and it
does not work: the measurement above *is* that case. The parent was `SIGKILL`ed,
its pipe closed, and ffmpeg carried on recording. Eliminated by the same run that
established the problem.

**Watch from a thread inside the app.** A `kqueue` watcher in a Glimpse thread
dies with the process it is watching. It would cover every case that is already
covered and none of the one that is not.

**Cap the recording with `-t`.** A maximum duration bounds the disk cost without
a new process. It does not release the capture device promptly, it invents a
maximum recording length the product has never had, and it makes a lifetime
guarantee out of a timeout. Rejected as a fix; it is a plausible *addition* and
not one this ADR needs.

**Do nothing beyond the sweep.** Defensible, and worth stating plainly: the
recording is a temp file, the sweep kills it at next launch, and `SIGKILL` is
rare. What makes it not enough is the rate. 18 GB/hour is not a tidiness problem,
and a user who force-quits an app that appears stuck is exactly the user who will
not launch it again soon.

## Decision

**A helper process watches the app and kills the capture when it dies.**

`Recorder::start` spawns, alongside ffmpeg, a second child: Glimpse re-executed
as `glimpse --reap <parent-pid> <child-pid>`. It blocks on `kevent` with
`EVFILT_PROC`/`NOTE_EXIT` for the parent, and on waking sends `SIGKILL` to the
capture. It exits when either process is gone, so it lives exactly as long as the
recording.

- **Re-executed self, not a new binary.** A second executable would have to be
  built, installed, signed, put in the bundle and found at runtime; a hidden
  argument on the binary already running is none of those.
- **`kevent`, not polling.** It blocks with no timer and no wakeups, and its
  latency is the kernel's. Polling `kill(pid, 0)` would work and would be
  simpler, and the argument for `kqueue` is only that a watchdog that is asleep
  until the event cannot itself be the thing that misbehaves.
- **macOS only, behind the same seam as the rest.** Linux keeps
  `PR_SET_PDEATHSIG`; this must not become a second mechanism doing the same job
  on the platform that has a kernel one. [ADR 0010](0010-capture-providers-and-a-platform-free-core.md)
  puts the platform-specific part behind the capture provider, and this belongs
  there with it.

The start-up sweep **stays**. It is the backstop for the case this cannot cover —
the helper itself being killed — and it is the only thing that cleans a workspace
left by a Glimpse from a previous boot.

## The strongest argument against

This adds a process to every recording to defend against a signal most users will
never send, and the failure it prevents is already bounded by a sweep that exists
and works. Every recording pays a little so that a rare one costs less.

The counter is the rate, and that the payment is genuinely small: one blocked
process, no timer, no wakeups, gone when the recording is. If the helper is ever
found to be less reliable than the thing it protects — if it can be orphaned
itself, or if the re-exec fails on a bundled app — this decision should be
reversed rather than patched, and the sweep is what the product falls back to.

## Consequences

- A recording survives the loss of its Glimpse by at most the kernel's
  notification latency, on both platforms, by two different mechanisms.
- `glimpse --reap` is a supported, undocumented argument, and `--help` should not
  grow a line for something no user types.
- A test must kill a real parent and assert the capture dies with it, on macOS.
  Nothing weaker demonstrates this: the whole point is a path no ordinary exit
  takes, and #45 is what happens when that path is reasoned about rather than
  exercised.
