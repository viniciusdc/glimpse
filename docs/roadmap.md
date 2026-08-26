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

## Done — the Record button, wired through the machine

Every user action goes through `session::transition`, so the policies stay in the
tested pure module instead of spreading into callbacks. `src/worker.rs` owns the
`Recorder` on its own thread — dropping the worker joins that thread, which
guarantees the child is killed and reaped before shutdown proceeds. A 100ms driver
polls for results and, while recording, calls `geometry_drifted()`: a frame that
moves aborts the recording rather than silently capturing the wrong region.

`refresh()` is the only writer of widget state, so the button label cannot
disagree with the machine.

Verified end to end with `GLIMPSE_SELFTEST=record`: Record → Recording at the
framing window's exact rect → Stop → a valid finalised ffv1 file, no orphaned
ffmpeg and no zombies.

## Done — encoding

`src/encode.rs`, **GIF and MP4** ([ADR 0007](adr/0007-gif-and-mp4.md)). GIF uses
`palettegen` → `paletteuse` with the **default** filter options
because the recommended screencast tweaks were measured and earned nothing
([ADR 0005](adr/0005-gif-encoding-and-the-atomic-commit.md)). Staged in the
destination's own directory so the rename is same-filesystem and genuinely
atomic, and committed only after ffmpeg exits successfully. A taken destination is
disambiguated (`glimpse-1.gif`) rather than replaced or refused.

Record → GIF works end to end.

## Done — settings and themes

`src/config.rs` and `~/.config/glimpse/config.toml`: theme, output format, output
folder, framerate and cursor capture, written on every change rather than at exit
([ADR 0008](adr/0008-settings-and-themes.md)). Three themes — follow system, light
and dark — with the colours that carry meaning identical across both palettes.

## Done — snapshot

A split button: Record or Snapshot, remembered between sessions. A snapshot is
one ffmpeg invocation and an atomic rename rather than a one-frame recording —
it has no session, no lifecycle and nothing to stop
([ADR 0009](adr/0009-snapshot.md)).

## Done — settings interface, stale-workspace sweep, encode cancellation

Frame rate and pointer capture are in the header menu. Stale `glimpse-*`
workspaces left by killed processes are removed at startup — only ones whose pid
is gone, so a second running Glimpse is never touched. Encoding can be cancelled
mid-flight: the destination is left untouched and the source recording preserved.

## Done — the v2 interface

Settings popover, read-only format chip, result sheet with the full path and real
buttons, three recording cues, and a determinate encode progress bar driven by
ffmpeg's own `-progress` output. **Encode Anyway** re-encodes a preserved capture
without re-recording, and Esc and Print Screen do what the status strip says they
do.

## Done — tidying up after a hard kill

Nothing can clean up during a `SIGKILL`, so it happens at the next startup:
stale session directories in `/tmp`, and the `.part` file and palette a kill
between writing and renaming leaves in the output folder. Both match only
Glimpse's own naming and only dead process ids
([ADR 0005](adr/0005-gif-encoding-and-the-atomic-commit.md)).

## Next

Nothing is queued. The product does what it set out to do: frame a region,
record it to GIF or MP4, snapshot it, and refuse to hand over a file that is
quietly wrong.

Candidates, in no particular order and none of them committed to:

- **APNG or WebM output.** Cheap — another arm on the encoder — but neither has
  asked to exist yet.
- **A shortcuts window**, once there are more than two shortcuts.
- **Multi-monitor awareness.** The capture rect is clipped to the root window,
  which spans all outputs, so nothing is broken; but the app has no notion of
  which screen it is on.
- **Wayland**, which remains a different application rather than a port
  ([ADR 0002](adr/0002-ffmpeg-pipeline-and-session-model.md)).

## Deliberately not planned for v0.1

APNG and WebM export; audio; i18n; global hotkeys; DBus activation;
flatpak/snap/appimage packaging.

**Wayland is not on this list as a "later backend".** It is a different
interaction model — a compositor that mediates selection cannot support a window
that decides its own capture rectangle. See
[ADR 0002](adr/0002-ffmpeg-pipeline-and-session-model.md).
