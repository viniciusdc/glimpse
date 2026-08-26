# Frequently asked questions

Answers about using Glimpse. For how it is built and why, see
[`architecture.md`](architecture.md) and the decision records in [`adr/`](adr/).

## Can I click things inside the recording area while recording?

Yes. The recording area is a real hole: Glimpse sets an X input shape so the
middle of the window does not accept pointer events at all, and they go to
whatever is underneath. This does not depend on your window manager or on
stacking order.

## Where does my recording go?

Into your videos folder — `XDG_VIDEOS_DIR` if you have one, otherwise your home
directory — as `glimpse.gif` or `glimpse.mp4`. Change it with **Save recordings
to…** in the header menu. If that name is taken Glimpse counts up —
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

No. The captured video is preserved and the status line gives you its path. Only
the conversion failed, so you can retry from that file with ffmpeg directly.

## What is the arrow next to Record?

It switches the button between **Record** and **Snapshot**. Snapshot grabs a
single frame of the same region and saves it as a PNG straight away — no timer,
no stop. The button remembers which you last used, so the common case stays one
click.

Snapshots are always PNG regardless of the GIF/MP4 setting, because a still frame
is an image and the recording format has nothing to say about it.

## Where are my settings stored?

`~/.config/glimpse/config.toml`. It holds the theme, the output format and
folder, and the framerate and cursor setting. It is written whenever you change
something rather than at exit, so a preference survives even if Glimpse is killed.
If the file is unreadable Glimpse says so and starts with defaults rather than
refusing to run.

## Can I record audio, or my webcam, or the whole desktop?

No, and none of these are planned. Glimpse records one silent region. See *About*.

## Are there macOS or Windows builds?

No, and there will not be. Glimpse works by being a window that knows its own
position on screen and declares its own capture rectangle; macOS and Windows both
refuse that, and it links X11 libraries besides. A build for either would be a
different application sharing a name, not a port — the same reason Wayland is out.

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
[`scripts/make-demo.py`](../scripts/make-demo.py) — though it is assembled into a GIF
by Glimpse's own pipeline, an ffv1 intermediate and then `palettegen`/`paletteuse`,
so the file itself is produced exactly the way a real recording would be.

There is no real screen capture in this README for two reasons. The middle of the
Glimpse window is transparent, so any genuine capture of it also publishes
whatever happened to be behind it. And the headless X server used for automated
testing has no compositor, so on it the transparency would not composite and the
hole — the one thing worth showing — would come out black.

Run `make demo` to regenerate the animation.
