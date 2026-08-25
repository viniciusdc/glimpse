# 0002 — An ffmpeg-only pipeline, and an explicit session model

- **Status:** ACCEPTED
- **Date:** 2026-08-25
- **Brain:** mirrors `D-0058`

## Context

The first MVP draft proposed capturing frames in-process over MIT-SHM and feeding
them straight into `gifski`, skipping ffmpeg entirely. It was sent for
adversarial review before any code was written. The review returned two blockers
and five majors; this record carries the ones that changed the design.

## Decision

**Capture pipeline:**

```
ffmpeg x11grab → recoverable temporary video → ffmpeg palettegen/paletteuse → atomic final GIF
```

`gifski` is dropped from v0.1.

**Domain model:**

```
Recorder.start(rect, options) → RecordingSession
RecordingSession.stop()       → CapturedVideo
GifEncoder.encode(CapturedVideo, destination)
```

**State machine:**

```
Idle → Arming → Recording → Encoding → Completed | Failed
```

## Because

**In-process capture would make this a screen-capture-engine project.** MIT-SHM
allocation and fallback, pixel-format and stride conversion, frame scheduling and
timestamping, an overload/drop policy, cursor acquisition and hotspot
compositing, concurrent encoding with bounded backpressure, capture error
recovery — none of it differentiates the product, and "stream into gifski" does
not remove the resource problem so much as trade unbounded memory for encoder
backpressure.

The draft's fallback was also wrong: using ffmpeg for capture does **not** oblige
decoding back into gifski. Keeping ffmpeg on both ends is simply the shorter
path.

**No `CaptureBackend` trait.** One proposal produces frames, the other produces a
media artefact; a trait pretending those are interchangeable becomes a bag of
optional methods. Do not generalise until a real second backend exists.

**The frame is locked during `Arming` and `Recording`,** and the rect is
snapshotted only after the last configure settles. `x11grab` records a fixed root
rectangle, so a frame the user can still drag mid-recording means the visible
frame and the actual capture diverge silently — the worst class of bug, because
the output looks plausible.

**Durability is designed now even though it is implemented later:** per-session
temp directory; the final GIF written beside its destination under a temporary
name and renamed atomically only on encoder success; the source recording
preserved on encoding failure so a retry is possible; cancellation defined
separately for capture and encoding; ffmpeg reaped on every exit path.

**Wayland is not a deferred backend — it is a different product.** The framing
window's invariant ("this window's inner rect is the capture region, and the app
knows where that is in root pixels") does not survive a compositor that mediates
selection. v0.1 is therefore intentionally X11-only. The consequence taken now:
*region selection* is kept separate from *capture* in the domain model, so a
Wayland selector could replace the former without touching the latter.

## Costs accepted

- A hard runtime dependency on ffmpeg.
- **GIF quality equal to Peek's, not better.** Dropping gifski gives up the one
  concrete quality advantage this rewrite could have claimed on day one. It
  returns only if measured output justifies a second pipeline.
- The intermediate video is not described as "lossless" anywhere until the exact
  codec and pixel-format command has been measured — `-lossless 1` does not prove
  that every preceding pixel-format conversion was lossless.

## Not safely deferrable

Cuts that look free and are not: **audio** needs clocks, synchronisation and
muxing and depends on the intermediate-video pipeline; **MP4** is cheap *only*
because that intermediate is retained; **alternate capture backends** are cheap
only through the session/artefact boundary above, not through a frame-oriented
trait.
