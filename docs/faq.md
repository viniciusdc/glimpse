# Frequently asked questions

Answers about using Glimpse. For how it is built and why, see
[`architecture.md`](architecture.md) and the decision records in [`adr/`](adr/).

## Can I click things inside the recording area while recording?

Yes on both platforms, reached two different ways.

On **X11** the recording area is a real hole: Glimpse sets an X input shape so
the middle of the window does not accept pointer events at all, and they go to
whatever is underneath. This does not depend on your window manager or on
stacking order, and it is true whether or not you are recording.

On **macOS** the whole window stops accepting clicks for as long as a recording
runs, because GTK there cannot make one region of a window click-through — the
measurements are in [ADR 0011](adr/0011-why-the-macos-frame-is-more-than-one-window.md)
and the design in [ADR 0017](adr/0017-click-through-is-a-mode-not-a-window.md).
Two consequences you will notice: the window dims to show it is not accepting
clicks, and the Stop button cannot be pressed, so stopping moves to the menu bar
item and a shortcut. Outside a recording the window does take clicks, so if you
want to arrange the application underneath before you start, turn on **Pass
clicks through** in the settings popover.

## Where does my recording go?

Into your videos folder — `XDG_VIDEOS_DIR` if you have one, `~/Movies` on
macOS, otherwise your home directory — as `glimpse.gif` or `glimpse.mp4`. Change it under **Save to** in the header's settings popover. If that name is taken Glimpse counts up —
`glimpse-1.gif`, `glimpse-2.gif` — rather than overwriting a file you might still
want. The status line names the file it just wrote, and **Show in folder** opens
it.

## Why is my GIF so large?

Because it is a GIF. Every frame is a full image with a 256-colour palette, and
there is no motion compensation, so file size scales with how much of the screen
changes. Recording a smaller area, or something with less motion, helps most.

If the destination accepts video, choose **MP4** instead. It is meaningfully
smaller — though by less than is often claimed: on a mostly-static capture it came
out about 1.5× smaller, not ten times.

## Why use GIF at all then?

Because it plays inline, automatically, silently and everywhere — in issue
trackers, pull requests, chat clients and documentation, with no player controls
and no click to start. That is the entire reason the format survives, and it is
why it is the default here.

## My recording stopped by itself and said the frame moved. Why?

Glimpse records a fixed rectangle of the screen. If the window is moved after
recording starts — dragged, or moved by the window manager — everything captured
after the move is of the wrong region, while the resulting file still looks
perfectly plausible. Rather than hand you a wrong recording, Glimpse stops and
tells you. The captured video up to that point is kept, and the status line says
where.

Resizing is disabled while recording for the same reason.

## Encoding failed. Did I lose the recording?

No. The capture is preserved, its path is shown, and **Encode Anyway** on the
result panel converts it again without re-recording — that is the whole reason
the capture is kept. Cancelling an encode leaves the same offer.

If you would rather do it yourself, the preserved file is an ffv1 Matroska and
any tool will read it.

## What is the arrow next to Record?

It switches the button between **Record** and **Snapshot**. Snapshot grabs a
single frame of the same region and saves it as a PNG straight away — no timer,
no stop. The button remembers which you last used, so the common case stays one
click.

Snapshots are always PNG regardless of the GIF/MP4 setting, because a still frame
is an image and the recording format has nothing to say about it.

## Is the install script safe to pipe into a shell?

Read it first. That is the honest answer for any such script, this one included.

What it guarantees: the download's SHA-256 is checked against the published
checksum and **installation is refused on a mismatch**, with the file deleted
rather than kept. The archive is extracted into a temporary directory and only the
expected binary is copied out, so an archive containing paths like `../../.bashrc`
cannot place anything anywhere. It installs to `~/.local/bin` and never calls sudo
by itself.

What it does not guarantee: releases are not signed, and the checksum comes from
the same host as the tarball — so a compromise of that host defeats both. If that
matters for your threat model, build from source.

## Are there keyboard shortcuts?

On **X11**, two: **Esc** stops a recording, and **Print Screen** takes a snapshot
when nothing is in flight. Both are named in the status strip while they apply.

On **macOS** those are the same, except while a recording is running. The window
takes no clicks then, so it cannot hold keyboard focus either, and Esc goes to
whatever you are working in instead. Stopping therefore has its own **global**
shortcut, `⌃⌥S` unless you change `stop_shortcut` in `config.toml`, plus the menu
bar item. Whichever of those actually registered is shown in place of the Stop
button while the mode is on, so the label cannot name a key nothing is listening
for.

## Where are my settings stored?

`~/.config/glimpse/config.toml`. It holds the theme, the output format and
folder, the framerate and cursor setting, and on macOS the `stop_shortcut` that
ends a recording. It is written whenever you change something rather than at
exit, so a preference survives even if Glimpse is killed. If the file is
unreadable Glimpse says so and starts with defaults rather than refusing to run.

Two keys there do nothing on macOS: `capture_mouse` (see below) and, on Linux,
`stop_shortcut`, which X11 has no use for because its Stop button stays
clickable.

## Why is there no Capture pointer switch on macOS?

Because it would not do anything.

Glimpse records through ffmpeg, and on macOS that means the `avfoundation`
input. It accepts a `-capture_cursor` flag and **ignores it**, in the off
direction: the pointer is never drawn, whatever you ask for. That was measured
rather than assumed — against a static window with the pointer parked in it and a
cursor provably rendered there, three runs gave a signal of zero, and the frame
was an exact match for a cursor-free reference capture.

So the switch is not shown.
[ADR 0012](adr/0012-a-setting-a-backend-cannot-honour.md) decides that a setting
a backend cannot honour is not offered at all, on the grounds that a control
which flips, persists across restarts and changes nothing is the same failure as
a file that lies about its contents. A missing control is confusing once; a
control that lies is confusing every time.

`capture_mouse` still exists in `config.toml`, because the file is shared with
X11 where the setting does work. Editing it by hand on macOS is allowed and has
no effect.

x11grab does honour it, so the switch is there on Linux.

## Can I record audio, or my webcam, or the whole desktop?

No, and none of these are planned. Glimpse records one silent region. See *About*.

## Are there macOS or Windows builds?

**macOS, yes. Windows, no.**

This answer used to say "no, and there will not be", reasoning that Glimpse works
by being a window that knows its own position and declares its own capture
rectangle, and that macOS refuses that. The second half was measured and it is
false: macOS hit-tests a non-opaque window per pixel against its alpha, so a
window with a transparent middle genuinely is click-through, and LICEcap has
shipped exactly that since 2011. The correction is recorded in
[ADR 0011](adr/0011-why-the-macos-frame-is-more-than-one-window.md), and the
window model it settled on — after two further corrections — in
[ADR 0017](adr/0017-click-through-is-a-mode-not-a-window.md).

macOS records today, through the same chrome Linux runs, resizes by dragging the
window edge, and ships as an `Glimpse.app` bundle carrying its own GTK — a
release artifact like the Linux one, installed by the same `install.sh`.

Two things differ. Its window stops accepting clicks while recording, so the
Stop button moves to the menu bar and a shortcut (ADR 0017). And the
pointer-capture switch is not offered, because avfoundation ignores it — the
answer above this one.

What is genuinely unfinished is signing: the bundle is ad-hoc signed, so a
download through a browser is quarantined while the same archive fetched with
`curl` runs.

Windows is untouched. Nobody has measured anything there, so read its absence as
unexamined rather than settled — which is precisely the mistake this answer made
about macOS.

Releases are Linux x86_64, and need an X11 session, GTK4 >= 4.10 and ffmpeg.

## Why no Wayland support?

Not an omission — the idea does not survive the transition. Glimpse works by being
a window that knows where it is on screen and declares its own capture rectangle.
Under Wayland the compositor mediates screen capture: an application asks the
portal, and the *user* picks what gets shared. A framing window cannot choose its
own region, so a Wayland version would be a different application with a different
interaction, not a port of this one.

Glimpse checks which display backend GTK actually chose at startup and exits with
an explanation rather than running and misbehaving. Note that having `DISPLAY` set
is not enough to be on X11 — under Wayland, XWayland usually answers it too.

## Is that animation a real recording?

No, and it says so under the image. It is drawn frame by frame by
[`scripts/make-demo.py`](../scripts/make-demo.py), following a 36-second script —
open settings, record, encode, save, switch to Snapshot, then move the frame
mid-recording and watch the abort — though it is assembled into a GIF
by Glimpse's own pipeline, an ffv1 intermediate and then `palettegen`/`paletteuse`,
so the file itself is produced exactly the way a real recording would be.

There is no real screen capture in this README for two reasons. The middle of the
Glimpse window is transparent, so any genuine capture of it also publishes
whatever happened to be behind it. And the headless X server used for automated
testing has no compositor, so on it the transparency would not composite and the
hole — the one thing worth showing — would come out black.

Run `make demo` to regenerate the animation.
