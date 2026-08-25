# 0007 — GIF and MP4 as the initial output formats

- **Status:** ACCEPTED
- **Date:** 2026-08-25
- **Builds on:** [ADR 0002](0002-ffmpeg-pipeline-and-session-model.md)

## Context

v0.1 shipped GIF only. ADR 0002 deferred MP4 while noting it would be *"cheap
**only** because that intermediate is retained"* — and the intermediate is
retained, as lossless ffv1. So MP4 is now the cheap addition that record
predicted, rather than a new pipeline.

## Decision

Two formats, chosen from the header chip, fixed at arming time:

- **GIF** — `palettegen`/`paletteuse`, unchanged.
- **MP4** — `libx264`, `yuv420p`, `+faststart`.

Changing format mid-session is refused rather than half-applied, because the
destination path and the encoder would otherwise disagree.

## Because — the crop is not optional

**H.264 with `yuv420p` requires even dimensions, and a framing window produces
odd ones constantly.** The first real capture this project ever made was
754×437. A naive MP4 path fails outright on it:

```
Error while opening encoder - maybe incorrect parameters such as ... width or height
```

Three fixes work. Only one is right for a screen recorder:

| filter | result on 754×437 | cost |
|---|---|---|
| `crop=trunc(iw/2)*2:trunc(ih/2)*2` | 754×436 | loses one row; every other pixel untouched |
| `pad=ceil(iw/2)*2:ceil(ih/2)*2` | 754×438 | adds a visible black line |
| `scale=trunc(iw/2)*2:trunc(ih/2)*2` | 754×436 | **resamples the whole frame** |

Cropping wins because this is a *screen* recorder: the content is text and UI at
1:1, and rescaling blurs every glyph in the capture to save one row of pixels.
A test asserts `scale=` never appears in the MP4 arguments.

`yuv420p` is chosen for compatibility rather than fidelity — it is the pixel
format every player and browser decodes. `+faststart` moves the index to the
front so a shared file plays before it has finished downloading.

Every flag comes from ffmpeg's own documentation, not another project's source
(ADR 0003).

## Costs accepted

- **MP4 is not a drop-in replacement for GIF.** It will not autoplay inline in
  every context a GIF will, which is often the entire reason to make a GIF.
- **MP4 loses up to one row and one column.** Invisible in practice, but the
  output is not pixel-identical to the region the user framed.
- **The size advantage is smaller than folklore claims.** Measured on identical
  source, a mostly-static 4-second capture gave 27,974 B as GIF and 18,248 B as
  MP4 — **1.5×**, not the order of magnitude often quoted. A palette compresses
  static screen content well. The gap widens with motion, so no fixed number is
  promised anywhere in the code or the docs.
- The intermediate is still ffv1 in Matroska for both formats, so recording cost
  is unchanged and a future format needs only a new encoder arm.
