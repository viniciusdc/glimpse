#!/usr/bin/env python3
"""Generate the animated illustration used in the README.

This is a DRAWING, not a screen capture. Glimpse cannot record itself for a
README: the middle of its window is transparent, so a real capture would publish
whatever was behind it, and the headless X server used for testing has no
compositor, so the transparency would not composite at all. So the frames are
drawn, and the README says so.

The animation is assembled with Glimpse's own encoding pipeline — ffv1
intermediate, then palettegen/paletteuse — so the GIF in the README is produced
the same way a real recording would be.

    scripts/make-demo.py [--out docs/assets/demo.gif] [--fps 12]
"""
import argparse
import pathlib
import shutil
import subprocess
import tempfile

W, H = 800, 460
FPS_DEFAULT = 12

# The framing window, in canvas coordinates.
FX, FY, FW = 96, 74, 608
HEADER_H, STATUS_H, BORDER = 40, 30, 3
HOLE_H = 236
FRAME_H = HEADER_H + HOLE_H + STATUS_H

FONT = "Inter, ui-sans-serif, -apple-system, Segoe UI, Roboto, Helvetica, Arial, sans-serif"
MONO = "JetBrains Mono, ui-monospace, SFMono-Regular, Menlo, Consolas, monospace"

CODE = [
    ("def quantize(frames, colors=256):", "#c084d8"),
    ("    hist = {}", "#c6ccd6"),
    ("    for f in frames:", "#c084d8"),
    ("        for px in f.sample(4096):", "#c6ccd6"),
    ("            hist[px] = hist.get(px, 0) + 1", "#c6ccd6"),
    ("    return rank(hist)[:colors]", "#8fbf72"),
]


def esc(t):
    return t.replace("&", "&amp;").replace("<", "&lt;").replace(">", "&gt;")


def cursor(x, y, pressed=False):
    ring = (
        f'<circle cx="{x}" cy="{y}" r="13" fill="#3689e6" fill-opacity="0.30"/>'
        if pressed else ""
    )
    return (
        f'{ring}<path d="M{x} {y} l0 17 l4.4 -4.4 l3.1 6.6 l3.4 -1.6 l-3.1 -6.4 l6.2 -0.2 Z" '
        f'fill="#ffffff" stroke="#11151a" stroke-width="1.4" stroke-linejoin="round"/>'
    )


def frame_svg(state, *, elapsed="", typed=0, cur=(0, 0), pressed=False):
    """state: idle | recording | stopping | saved"""
    accent = {"idle": "#3689e6", "recording": "#e04b4b",
              "stopping": "#e04b4b", "saved": "#3689e6"}[state]
    label = "Stop" if state in ("recording", "stopping") else "Record"
    btn = "#c6262e" if state in ("recording", "stopping") else "#3689e6"
    status = {
        "idle": ("Position the frame, then Record.", "#8b939e"),
        "recording": (f"recording 602 &#215; 230", "#d78f8f"),
        "stopping": ("encoding…", "#8b939e"),
        "saved": ("saved ~/glimpse.gif", "#8b939e"),
    }[state]

    # Editor behind, drawn first so it shows through the hole.
    lines = []
    caret = None          # (x, y) just past the last character actually drawn
    y = 150
    for i, (text, colour) in enumerate(CODE):
        shown = text if i < typed else ("" if i > typed else text[: max(0, (typed - i + 1) * 40)])
        if shown.strip():
            # Indent with an x offset rather than leading spaces: SVG collapses
            # leading whitespace whatever xml:space says, and unindented Python
            # reads as broken code.
            indent = len(shown) - len(shown.lstrip())
            lines.append(
                f'<text x="{150 + indent * 7.4:.0f}" y="{y}" font-family="{MONO}" font-size="13" '
                f'fill="{colour}">{esc(shown.strip())}</text>'
            )
            caret = (150 + len(shown) * 7.4, y)
        y += 24


    parts = [
        f'<svg xmlns="http://www.w3.org/2000/svg" width="{W}" height="{H}" viewBox="0 0 {W} {H}">',
        f'<rect width="{W}" height="{H}" fill="#0f1216"/>',
        f'<rect width="{W}" height="{H}" fill="#151a21"/>',
        # editor window
        '<rect x="60" y="40" width="680" height="380" rx="9" fill="#1c2026" stroke="#2b313a"/>',
        '<path d="M60 49 a9 9 0 0 1 9 -9 h662 a9 9 0 0 1 9 9 v22 h-680 Z" fill="#23272e"/>',
        '<circle cx="78" cy="57" r="4" fill="#4a505a"/>',
        f'<text x="94" y="61" font-family="{FONT}" font-size="11.5" fill="#c3c9d2">encoder.py</text>',
        f'<text x="176" y="61" font-family="{FONT}" font-size="11.5" fill="#6d7681">palette.py</text>',
        '<rect x="60" y="71" width="34" height="349" fill="#191d23"/>',
    ]
    for i in range(len(CODE)):
        parts.append(
            f'<text x="82" y="{150 + i * 24}" font-family="{MONO}" font-size="12" '
            f'fill="#4d5560" text-anchor="end">{i + 1}</text>'
        )
    parts += lines
    if typed < len(CODE) and caret:
        parts.append(
            f'<rect x="{caret[0]:.0f}" y="{caret[1] - 11}" width="7" height="15" fill="#c6ccd6"/>'
        )

    # framing window: header, hole (never painted), status
    hy = FY + HEADER_H
    parts += [
        f'<path d="M{FX} {FY+8} a8 8 0 0 1 8 -8 h{FW-16} a8 8 0 0 1 8 8 v{HEADER_H-8} h-{FW} Z" fill="#282c33"/>',
        f'<text x="{FX+16}" y="{FY+25}" font-family="{FONT}" font-size="11.5" fill="#8b939e">602 &#215; 230</text>',
    ]
    if state == "recording":
        parts.append(f'<circle cx="{FX+112}" cy="{FY+20}" r="4" fill="#e04b4b"/>')
        parts.append(f'<text x="{FX+124}" y="{FY+25}" font-family="{FONT}" font-size="11.5" fill="#c3c9d2">{elapsed}</text>')
    bx = FX + FW // 2 - 44
    parts += [
        f'<rect x="{bx}" y="{FY+11}" width="88" height="20" rx="10" fill="{btn}"/>',
        (f'<circle cx="{bx+15}" cy="{FY+21}" r="4" fill="#fff"/>' if label == "Record"
         else f'<rect x="{bx+11}" y="{FY+17}" width="8" height="8" fill="#fff"/>'),
        f'<text x="{bx+26}" y="{FY+25}" font-family="{FONT}" font-size="11.5" font-weight="500" fill="#ffffff">{label}</text>',
        f'<rect x="{FX+FW-74}" y="{FY+13}" width="28" height="15" rx="4" fill="none" stroke="#a7aeb9" stroke-opacity="0.45"/>',
        f'<text x="{FX+FW-69}" y="{FY+24}" font-family="{FONT}" font-size="9" fill="#a7aeb9">GIF</text>',
    ]
    for i in range(3):
        parts.append(f'<line x1="{FX+FW-34}" y1="{FY+15+i*5}" x2="{FX+FW-22}" y2="{FY+15+i*5}" stroke="#a7aeb9" stroke-width="1.5" stroke-linecap="round"/>')
    parts += [
        f'<rect x="{FX+1.5}" y="{hy+1.5}" width="{FW-3}" height="{HOLE_H-3}" fill="none" stroke="{accent}" stroke-width="{BORDER}"/>',
        f'<path d="M{FX} {hy+HOLE_H} h{FW} v{STATUS_H-8} a8 8 0 0 1 -8 8 h-{FW-16} a8 8 0 0 1 -8 -8 Z" fill="#101216"/>',
        f'<text x="{FX+16}" y="{hy+HOLE_H+19}" font-family="{FONT}" font-size="11" fill="{status[1]}">{status[0]}</text>',
    ]
    if state == "saved":
        parts.append(f'<circle cx="{FX+FW-92}" cy="{hy+HOLE_H+15}" r="3.5" fill="#68b3f0"/>')
        parts.append(f'<text x="{FX+FW-82}" y="{hy+HOLE_H+19}" font-family="{FONT}" font-size="11" fill="#8ab4f8">Show in folder</text>')

    parts.append(cursor(cur[0], cur[1], pressed))
    parts.append("</svg>")
    return "\n".join(parts)


def timeline(fps):
    """(state, elapsed, typed, cursor, pressed) per frame."""
    out = []
    bx = FX + FW // 2
    home, target = (250, 300), (bx + 6, FY + 27)

    def lerp(a, b, t):
        t = t * t * (3 - 2 * t)
        return (a[0] + (b[0] - a[0]) * t, a[1] + (b[1] - a[1]) * t)

    for i in range(int(1.4 * fps)):                      # approach Record
        out.append(("idle", "", 1, lerp(home, target, i / (1.4 * fps)), False))
    for _ in range(int(0.25 * fps)):                     # click
        out.append(("idle", "", 1, target, True))
    n = int(4.2 * fps)
    for i in range(n):                                   # recording, code types
        secs = int(i / fps) + 1
        typed = 1 + int(i / n * (len(CODE)))
        out.append(("recording", f"0:0{min(secs,9)}", min(typed, len(CODE)), target, False))
    for _ in range(int(0.25 * fps)):                     # click Stop
        out.append(("recording", "0:04", len(CODE), target, True))
    for _ in range(int(0.9 * fps)):
        out.append(("stopping", "", len(CODE), target, False))
    for _ in range(int(2.4 * fps)):
        out.append(("saved", "", len(CODE), target, False))
    return out


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--out", default="docs/assets/demo.gif")
    ap.add_argument("--fps", type=int, default=FPS_DEFAULT)
    args = ap.parse_args()

    for tool in ("convert", "ffmpeg"):
        if not shutil.which(tool):
            raise SystemExit(f"{tool} is required")

    root = pathlib.Path(__file__).resolve().parent.parent
    out = (root / args.out).resolve()
    out.parent.mkdir(parents=True, exist_ok=True)

    with tempfile.TemporaryDirectory(prefix="glimpse-demo-") as tmp:
        tmp = pathlib.Path(tmp)
        frames = timeline(args.fps)
        for i, (state, elapsed, typed, cur, pressed) in enumerate(frames):
            svg = tmp / f"f{i:04d}.svg"
            svg.write_text(frame_svg(state, elapsed=elapsed, typed=typed,
                                     cur=(round(cur[0]), round(cur[1])), pressed=pressed))
            subprocess.run(["convert", "-background", "none", str(svg),
                            str(tmp / f"f{i:04d}.png")], check=True,
                           stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
        print(f"drew {len(frames)} frames")

        # Assembled with Glimpse's own pipeline: lossless intermediate, then
        # the two-pass palette.
        mkv, pal = tmp / "demo.mkv", tmp / "palette.png"
        run = lambda a: subprocess.run(a, check=True, stdout=subprocess.DEVNULL,
                                       stderr=subprocess.DEVNULL)
        run(["ffmpeg", "-hide_banner", "-loglevel", "error", "-y", "-framerate", str(args.fps),
             "-i", str(tmp / "f%04d.png"), "-c:v", "ffv1", "-pix_fmt", "bgr0", str(mkv)])
        run(["ffmpeg", "-hide_banner", "-loglevel", "error", "-y", "-i", str(mkv),
             "-vf", "palettegen", str(pal)])
        run(["ffmpeg", "-hide_banner", "-loglevel", "error", "-y", "-i", str(mkv),
             "-i", str(pal), "-lavfi", "paletteuse", "-f", "gif", str(out)])

    print(f"wrote {out.relative_to(root)} ({out.stat().st_size:,} bytes)")


if __name__ == "__main__":
    main()
