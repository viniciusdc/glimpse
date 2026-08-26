# 0005 — GIF encoding, and how the output is committed

- **Status:** ACCEPTED
- **Date:** 2026-08-25

## Context

[ADR 0002](0002-ffmpeg-pipeline-and-session-model.md) chose `palettegen` /
`paletteuse` and required that the final GIF be committed atomically, with the
source preserved on failure. It left three things open that only become real when
the code exists: which filter options to use, where the file is staged, and what
happens when the destination is already taken.

## Decision

### Filter options: the defaults

Measured on a real 3-second screen capture rather than reasoned about:

| option | output | vs default |
|---|---|---|
| `palettegen=stats_mode=full` (default) | 1,471,070 B | — |
| `palettegen=stats_mode=diff` | 1,453,758 B | −1.2%, inside noise |
| `palettegen=stats_mode=single` | 2,170,981 B | **+48% worse** |
| `paletteuse=diff_mode=rectangle` | 1,471,577 B | marginally *larger* |

`stats_mode=diff` and `diff_mode=rectangle` are both widely recommended for
screencasts, on the reasoning that most of the frame is static. On this workload
they buy nothing — one is within noise, the other is slightly worse.

**So Glimpse uses the defaults.** A flag that cannot demonstrate a benefit does
not go in, and a test asserts that neither option is smuggled back in later.

The one option that *is* stated explicitly is `-f gif`, because the output is
staged under a `.gif.part` name and ffmpeg cannot infer a muxer from that.

### Collision policy: disambiguate, never replace, never fail

If the destination exists, `glimpse.gif` becomes `glimpse-1.gif`, then
`glimpse-2.gif`.

The alternatives are both worse. **Replacing** silently destroys a file the user
may still want. **Failing** loses a recording they just made, because of a name
clash that has an obvious safe resolution — and the recording is the expensive
thing here; the filename is not.

### The commit: staged in the destination's own directory

The GIF is written to a hidden `.glimpse-<pid>-<name>.gif.part` **in the
destination directory**, then renamed onto the final path.

Staging in the session temp directory would have been the obvious symmetry with
the recording, and it would have been wrong: `/tmp` is frequently a different
filesystem, so the rename would degrade to a copy and stop being atomic. A reader
could then observe a half-written GIF. Same directory, same filesystem, real
rename.

On any failure the staged file and the palette are removed, and the source
recording is never touched.

## Costs accepted

- **An encode in progress cannot be cancelled.** `encode_gif` waits on ffmpeg, so
  dropping the encoding worker joins it — quitting during an encode waits for it
  to finish rather than killing it. ADR 0002 asked for cancellation to be defined
  separately for capture and encoding; capture has it, encoding does not yet.
- **A hard kill (SIGKILL, `XKillClient`) orphans the ffmpeg child and leaks
  the session workspace.** `Drop` cannot run, so nothing reaps it — demonstrated
  accidentally while testing resize, which left two ffmpeg processes recording
  into deleted directories. Every exit path the *process* controls is covered;
  one it does not control is not. `prctl(PR_SET_PDEATHSIG)` would close this.
- **A process killed mid-encode leaves litter**: the `.part` file and the palette
  stay in the destination directory. They are hidden and prefixed `.glimpse-`, so
  they are identifiable, but nothing sweeps them up.
- Disambiguating means a user who records repeatedly accumulates
  `glimpse-1.gif`, `glimpse-2.gif`, … until output selection exists. That is
  noisy, and it is still better than either destroying files or refusing to save.

## A bug this decision's verification caught

Cleanup originally recovered the workspace path from the session state, via
`State::retryable()`. On the happy path the state is `Completed` by the time
cleanup runs, and `Completed` carries no `CapturedVideo` — so **every successful
encode leaked its recording directory**, several megabytes at a time. Nothing
failed; the GIF was correct and the temp directory simply stayed.

It surfaced by listing `/tmp` after a successful run rather than trusting that
"Completed" meant everything had been cleaned. The controller now tracks the
workspace itself.


---

## Update — the orphaned child is fixed; the workspace still leaks

The cost recorded above ("a hard kill orphans the ffmpeg child and leaks the
session workspace") was half-closed by asking the kernel for help.

`Recorder::start` now sets `PR_SET_PDEATHSIG` to `SIGKILL` in `pre_exec`, so the
kernel kills ffmpeg when the thread that spawned it dies — including on `SIGKILL`,
where no Rust destructor can run. Verified by starting a recording, sending the
application `SIGKILL`, and counting processes: **0 → 1 while recording → 0 two
seconds after the kill.**

Gated on `target_os = "linux"` rather than `unix`. `PR_SET_PDEATHSIG` is a Linux
interface and macOS is also `unix`, so a `cfg(unix)` guard would compile there and
fail to link. Glimpse is Linux-only today, but the guard should be true rather
than merely sufficient.

**Both leftovers are now swept at the next startup**, which is the only moment
code can run to remove them: `capture::sweep_stale_workspaces` for the session
directory in `/tmp`, and `encode::sweep_stale_staging` for the `.part` file and
palette that a kill between writing and renaming leaves in the *output* folder.

Both are deliberately narrow. They match only Glimpse's own naming, and only when
the process id in the name is no longer alive — a second Glimpse recording or
encoding right now must not have its files deleted out from under it. Verified
against a live-pid file, a dead-pid file, and ordinary files sitting in the same
folder.


---

## Update — encoding can now be cancelled, with one unresolved case

The cost recorded above ("an encode in progress cannot be cancelled") is largely
closed. `encode_cancellable` polls rather than blocking on `wait`, so a
`Canceller` can kill the ffmpeg child mid-encode; the session machine already
modelled `Cancel` during `Encoding`, so only the executor was missing.

Verified: with an encode running, cancelling takes the live ffmpeg count from 1 to
0, and the source recording is preserved.

**The defect recorded here has been found, and it was not a race.**

The symptom was real: the session reported `Cancelled` while a finished file
appeared at the destination. The explanation written here first — a cancel
arriving just after the atomic rename — was wrong, which is why two attempts to
narrow that window changed nothing.

The actual cause was that the UI called `encode` rather than the cancellable
variant. The Cancel button fired a `Canceller` connected to nothing; the encode
ran to completion and committed; and the job's result was discarded when the
session cleaned up. Every observation followed from that, including the absence
of any settle or drain event in the trace.

It came from an edit that silently failed to apply — the anchor no longer matched
after the file had been reformatted — so the code kept calling the old function
while everything around it was rewritten to assume otherwise.

Two changes, one fixing it and one making it unrepeatable:

- `encode` now checks the cancel flag immediately before the rename. Without that,
  a cancellation landing after the last ffmpeg pass exits still commits, because
  nothing would look at the flag again.
- **The convenience overload that omitted the canceller is gone.** Callers must
  pass one; those with nothing to cancel pass `&Canceller::new()` and say so.
  Requiring the argument makes the original mistake unspellable rather than
  merely fixed.

Verified end to end: cancelling mid-encode now leaves the destination empty and
the source recording preserved.

```sh
GLIMPSE_TRACE=1 GLIMPSE_SELFTEST=cancel-encode scripts/headless.sh cargo run
```
