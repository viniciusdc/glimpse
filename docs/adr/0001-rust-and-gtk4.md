# 0001 — Rust and GTK4, rewriting Peek

- **Status:** ACCEPTED
- **Date:** 2026-08-25
- **Brain:** mirrors `D-0057`
- **Superseded in part by:** [ADR 0003](0003-apache-2-0.md) — the licence is now Apache-2.0

## Context

[Peek](https://github.com/phw/peek) is a GIF screen recorder with a framing
window — 3,768 lines of Vala on GTK3, GPL-3. It works, and its shape is right.
The rewrite is about the implementation language and the encoding pipeline, not
about the product concept, which is Peek's.

Rust and Go were both candidates.

## Decision

**Rust, on GTK4**, licensed GPL-3.0-or-later *(the licence half is superseded — see [ADR 0003](0003-apache-2-0.md); the toolkit half stands)*.

## Because

`gtk4-rs` is a first-class, actively maintained binding; Go's `gotk4` is
auto-generated with a considerably thinner community around it. Rust also has
`ashpd`, which is the credible path to XDG-portal/PipeWire capture if Wayland is
ever attempted — there is no Go equivalent.

Alternatives, and why not:

- **Go + GTK3.** The fastest route to a running MVP; the GTK3 headers were
  already installed. Rejected because every encode would shell out to ffmpeg
  anyway, inheriting Peek's exact quality ceiling rather than beating it.
- **Rust + GTK3.** Needed no new system dependency and is what Peek proves works.
  Genuinely the strongest counter-argument, and it was reconsidered mid-decision
  when the `get_xid` deprecation surfaced. GTK4 was kept only because the spike
  ([ADR 0000](0000-x11-framing-window-spike.md)) was run and passed. Had it
  failed, this ADR would read GTK3.

An argument that was made for Rust and then **withdrawn**, recorded because it
influenced the decision: that `gifski` links in-process as a crate, where Peek
can only shell out to the binary after dumping PNG frames to disk. True, but
[ADR 0002](0002-ffmpeg-pipeline-and-session-model.md) drops gifski from v0.1
entirely, so it is a roadmap argument rather than one this release can claim.

## Costs accepted

- A system dependency on `libgtk-4-dev` to build.
- The central invariant rests on a deprecated API. See
  [ADR 0000](0000-x11-framing-window-spike.md) for the exit strategy.
- GPL-3, inherited from Peek. This is a rewrite informed by reading GPL-3 source,
  so matching the licence is the safe answer rather than a preference.
