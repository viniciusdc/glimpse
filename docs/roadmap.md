# Roadmap

Ordered by dependency, not by date.

## Done

- **The framing window.** Transparent, click-through hole; the input region is
  re-punched whenever the layout settles; `capture_rect` runs the full conversion
  chain with clipping; `lock()`/`unlock()` freeze the geometry for a session.
- **The toolkit question, settled by evidence.** See
  [ADR 0000](adr/0000-x11-framing-window-spike.md).

## Next — capture

`ffmpeg -f x11grab` against the locked rect, into a recoverable temporary video.
Stop by writing `q` to ffmpeg's stdin. The process must be reaped on **every**
exit path, including cancellation and application shutdown.

## Then — encoding

`palettegen` → `paletteuse` to GIF. Written beside the destination under a
temporary name and renamed atomically only after ffmpeg exits successfully; the
source recording is preserved on failure so a retry is possible.

## Then — the session state machine

```
Idle → Arming → Recording → Encoding → Completed | Failed
```

Cancellation is defined separately for capture and for encoding, because they
fail differently and encoding can outlast capture by a lot.

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
