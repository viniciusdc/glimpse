# 0018 — Multi-display on macOS is refused, not guessed

- **Status:** PROPOSED
- **Date:** 2026-09-18
- **Relates to:** [ADR 0002](0002-ffmpeg-pipeline-and-session-model.md),
  [ADR 0012](0012-a-setting-a-backend-cannot-honour.md),
  [ADR 0017](0017-click-through-is-a-mode-not-a-window.md),
  [issue #14](https://github.com/viniciusdc/glimpse/issues/14)

## Context

macOS reports screen geometry per display, and Glimpse's conversion from AppKit
points to a `ScreenPixelRect` uses two facts that are **not** global:

```rust
let primary = NSScreen::screens(mtm).iter().next()?;
let height = primary.frame().size.height;   // correct: AppKit is global to primary
let scale  = primary.backingScaleFactor();  // NOT correct for a window elsewhere
```

The height is right. AppKit coordinates are relative to the primary screen
whatever display a window is on, so flipping against its height works for a frame
dragged anywhere. The **scale factor is not**: it belongs to the screen the frame
is actually on, and a 2x primary beside a 1x external gives a rectangle exactly
twice the size it should be — or half.

And the backend captures **one device**. `AvfCapture` discovers `Capture screen 0`
and crops it. A frame on the second display is cropped out of the *first*
display's capture, at coordinates that mean something there too. So today, with
two displays, dragging the frame to the second one and pressing Record produces a
recording of the wrong screen's contents, at plausible dimensions, with no error.

That is the outcome issue #14 names as the only unacceptable one, and it is the
failure [ADR 0000](0000-x11-framing-window-spike.md) exists to record: a
rectangle that is the right size on the wrong pixels looks entirely correct until
somebody watches the file.

## Decision

**Glimpse refuses to record a frame that is not entirely on the primary display,
and says why.** It does not attempt to pick a capture device, and it does not
convert using a second screen's scale factor.

The check runs where the permission check runs — when Record is pressed, before a
session exists — so a refusal costs no workspace, no ffmpeg child and no
half-started recording.

Single-display machines are unaffected and pay nothing: with one screen the
condition is trivially satisfied.

## Because

**A wrong recording is worse than a refused one.** Every other option on the
table ships a rectangle that might be right. This is the same reasoning as
[ADR 0012](0012-a-setting-a-backend-cannot-honour.md) — a control that flips and
changes nothing is worse than an absent one — applied to a capture instead of a
switch.

**Support cannot be verified on the machine that would write it.** This was
decided on a single-display laptop. Multi-display handling that nobody has run on
two displays is exactly the kind of code that looks finished and records the
wrong screen, and the project already has a rule about that: numbers agreeing
with numbers is not verification.

**A region spanning two displays may not be expressible at all.** avfoundation
captures one device. Two displays are two captures, at possibly different scales,
that would have to be composited into one frame with the gap between them
invented. That is a different feature from cropping a screen, and calling it
"multi-display support" would hide how different.

## Costs

**A two-display user cannot record on their second screen, which is where many
of them work.** This is a real loss and the message has to be honest about it:
"not supported yet" rather than an error that reads like a fault.

**The check is geometric, so a frame straddling the edge is refused even though
the primary's half is capturable.** Allowing the straddle would mean recording a
rectangle whose right half is not what the user framed. Refusing the whole thing
is the answer that cannot be subtly wrong.

**It cannot be tested where it was written.** The refusal path needs two
displays. What *is* tested here is that a single-display machine never triggers
it, which is the regression that would matter most — a check that refused
everybody would be worse than the bug it prevents.

## What would falsify this

- **Someone runs it on two displays and the refusal never fires**, or fires on a
  single-display setup. Either means the geometry test is wrong, and a wrong
  refusal is its own bug.
- **avfoundation turns out to expose a capture that spans displays.** Then the
  "one device" argument weakens and support becomes a crop again rather than a
  composite.
- **The scale factor turns out to be uniform in practice** on the configurations
  people actually use. It is not a safe assumption — Apple ships 2x laptops that
  people plug 1x monitors into — but if measurement said otherwise, the
  conversion half of this problem would disappear.

## Consequences

`glimpse_macos::screens` answers whether a rectangle is safely capturable, the
`grab` hook refuses before arming, and the self-test reports the display
situation so a bug report from a two-display machine says so without being asked.

Issue #14 stays open as the feature. This record is what makes its absence
deliberate rather than unnoticed.
