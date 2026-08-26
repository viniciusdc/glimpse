# 0009 — Snapshot, and why it is not a one-frame recording

- **Status:** ACCEPTED
- **Date:** 2026-08-26

## Context

The framing window already answers "which pixels?", and often the answer wanted
is a still image rather than an animation. Reaching for a separate screenshot tool
to capture a region you have already framed is silly.

The alternative proposal was to put the output folder behind a dropdown on the
Record button. That was rejected: a folder is a *setting*, and hanging a setting
off an action button means the control means two unrelated things.

## Decision

**Record and Snapshot share a split button.** The left half performs the action,
the right half chooses which action it is, and the choice is remembered — so the
one-click path stays whatever you did last.

That is what a split button is for: two *actions* of the same kind, not an action
and a preference.

**A snapshot is not a session.** It does not go through
[`session`](../../src/session.rs) at all — there is nothing to arm, no geometry to
freeze against a moving frame, no stop, no cancellation and no retryable artifact.
It is one ffmpeg invocation and an atomic rename. Modelling it as a one-frame
recording would mean inventing states that can never be observed.

**Snapshots are always PNG**, whatever the recording format is set to. A still
frame is an image; the choice between GIF and MP4 has nothing to say about it.

It does share the two rules that matter, because they are about not corrupting
user data rather than about recording: the file is staged in the destination's own
directory and renamed into place, and a taken name is disambiguated rather than
overwritten ([ADR 0005](0005-gif-encoding-and-the-atomic-commit.md)).

## Because — the bug that justifies stating the codec

`image2` is a *container*, and its default encoder is mjpeg. Because the output is
staged under a `.png.part` name, ffmpeg can infer nothing from the extension — so
`-f image2` alone writes **a JPEG into a file called `.png`**.

It did exactly that, and the smoke test reported success, because the status line
only knows the path it was given. It surfaced from running `identify` on the
result instead of trusting the filename. `-c:v png` is now explicit and a test
asserts it.

The same class of mistake was already fixed once for GIF, where `.gif.part` left
ffmpeg unable to guess a muxer. Staging files under a neutral suffix buys
atomicity and costs every format-by-extension inference — worth paying, worth
remembering.

## Costs accepted

- A third artifact type in a directory that already collects `glimpse-1.gif` and
  `glimpse-2.mp4`. Output naming is due a rethink once there is a settings surface.
- The split button is assembled from a `Button` and a `MenuButton` joined by CSS,
  because GTK has no split button outside libadwaita. It looks like one control
  and is two, so a restyle can pull it apart without any test noticing.
- Snapshot ignores framerate, and reads `capture_mouse` — which is right, but
  means one setting silently applies to it and another silently does not.
