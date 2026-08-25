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
