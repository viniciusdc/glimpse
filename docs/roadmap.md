# Roadmap

Ordered by dependency, not by date.

## Done

- **The framing window.** Transparent, click-through hole; the input region is
  re-punched whenever the layout settles; `capture_rect` runs the full conversion
  chain with clipping; `lock()`/`unlock()` freeze the geometry for a session.
- **The toolkit question, settled by evidence.** See
  [ADR 0000](adr/0000-x11-framing-window-spike.md).

## Done — the session lifecycle skeleton

`src/session.rs`, 13 tests. Pure `(State, Event) → (State, Effect)`; no process
handles, no clock, no I/O. Every policy below is pinned by a test that runs in CI
without a display.

This came **before** capture, a reversal of the original ordering; see
[ADR 0004](adr/0004-review-corrections-and-the-lifecycle-spine.md). Stopping,
cancellation, shutdown, artifact retention and child reaping are not UI wrapped
around a finished `Command` — they decide how that command is owned. Writing
capture first means writing it twice.

```
Idle → Arming → Recording → Stopping → Encoding → Completed | Failed | Cancelled
```

`Stopping` is not decoration: ffmpeg can have received `q` without having
finalised the container, and that interval is where a "plausible but truncated
video" comes from. State variants own the resources valid in that state, so a
failed encode cannot lose the reference to the source video it promised to
preserve. Cancellation is defined separately for capture and for encoding,
because they fail differently and encoding can outlast capture by a lot.

## Done — capture

`src/capture.rs`. `ffmpeg -f x11grab` into a per-session workspace, stopped by
writing `q` to ffmpeg's stdin with a bounded escalation to a kill. `Recorder`
exclusively owns the child and waits on it on every exit path, including a `Drop`
backstop for panics and early returns.

The intermediate is **ffv1 in Matroska at `bgr0`**. x11grab's native output is
`bgr0` and ffv1 stores it unchanged, so nothing is converted and the intermediate
is lossless by construction — which is what ADR 0002 required before the word
could be used at all. Verified with `ffprobe` on a real capture:
`codec_name=ffv1, pix_fmt=bgr0`, and a full decode with no errors.

Every flag is derived from `ffmpeg -h demuxer=x11grab` and
[ffmpeg-devices](https://ffmpeg.org/ffmpeg-devices.html), not from Peek's source
([ADR 0003](adr/0003-apache-2-0.md)). The region uses the documented
`-grab_x`/`-grab_y` options rather than encoding the origin into the input URL.

## Next — wiring capture to the session machine and the UI

The Record button drives `Idle → Arming → Recording`, a worker thread owns the
`Recorder` so the UI thread never blocks, and `geometry_drifted()` is polled while
recording so a moved frame aborts rather than producing a plausible wrong file.

## Then — encoding, as the next slice

`palettegen` → `paletteuse` to GIF. Staged **in the destination directory** so the
rename is not cross-filesystem, and committed atomically only after ffmpeg exits
successfully; the source recording is preserved on failure so a retry is possible.

Destination collision policy — fail, unique name, or explicit replace — is decided
here rather than in the settings milestone, because it governs the commit step.

## Then — the things that make it an application

Output-path selection and collision behaviour; framerate, downsample and
capture-mouse settings persisted to `~/.config/glimpse`; an elapsed-time
indicator.

## Deliberately not planned for v0.1

APNG, WebM and MP4 export; audio; i18n; global hotkeys; DBus activation;
flatpak/snap/appimage packaging.

**Wayland is not on this list as a "later backend".** It is a different
interaction model — a compositor that mediates selection cannot support a window
that decides its own capture rectangle. See
[ADR 0002](adr/0002-ffmpeg-pipeline-and-session-model.md).
