#!/usr/bin/env python3
"""Generate the animated illustration used in the README.

This is a DRAWING, not a screen capture. Glimpse cannot record itself for a
README: the middle of its window is transparent, so a real capture would publish
whatever was behind it, and the headless X server used for testing has no
compositor, so the transparency would not composite at all.

The story, the timing and the cursor path are ported from the `Glimpse Demo`
design document — a 36 second loop: open settings, record, encode, save, switch
to Snapshot, then move the frame mid-recording and watch it abort.

Cursor targets are derived from THIS script's layout rather than copied from the
mock's pixel coordinates, so the pointer lands on the real controls even if the
window geometry here differs from the design canvas.

The frames are assembled with Glimpse's own pipeline — an ffv1 intermediate,
then palettegen/paletteuse — so the GIF is produced the way a real recording is.

    scripts/make-demo.py [--out docs/assets/demo.gif] [--fps 10] [--scale 0.78]
"""
import argparse
import pathlib
import shutil
import subprocess
import tempfile

W, H = 880, 560
LOOP = 36.0

FONT = "Inter, ui-sans-serif, -apple-system, Segoe UI, Roboto, Helvetica, Arial, sans-serif"
MONO = "JetBrains Mono, ui-monospace, SFMono-Regular, Menlo, Consolas, monospace"

# ---- the Glimpse window, in canvas coordinates ------------------------------
# The app is the subject, so it fills the frame. An earlier cut framed a whole
# desktop with Glimpse as one element among several, and the controls — the thing
# the animation exists to show — ended up too small to read.
WX, WY, WW = 60, 46, 760
HEADER_H, RULE_H, STATUS_H, SHEET_H, BORDER = 44, 2, 32, 56, 3
HOLE_H = 372
REC_CX = WX + WW // 2                      # split button centre
GEAR_CX = WX + WW - 26                     # settings gear
ARROW_CX = REC_CX + 58                     # the ▾ half of the split button
HEADER_CY = WY + HEADER_H // 2
SHEET_CY = WY + HEADER_H + RULE_H + HOLE_H + SHEET_H // 2

CODE = [
    ("# build an indexed palette from sampled frames", "cmt"),
    ("import subprocess", "kw"),
    ("from pathlib import Path", "kw"),
    ("", "txt"),
    ("def quantize(frames, colors=256):", "fn"),
    ("    hist = {}", "txt"),
    ("    for f in frames:", "kw"),
    ("        for px in f.sample(4096):", "kw"),
    ("            hist[px] = hist.get(px, 0) + 1", "txt"),
    ("    ranked = sorted(hist.items(), key=lambda kv: -kv[1])", "txt"),
    ("    return [c for c, _ in ranked[:colors]]", "kw"),
    ("", "txt"),
    ("def encode(src: Path, out: Path, fps=15):", "fn"),
    ('    args = ["ffmpeg", "-i", str(src), "-vf",', "str"),
    ('            f"fps={fps},split[a][b]", str(out)]', "str"),
    ("    proc = subprocess.run(args, capture_output=True)", "txt"),
    ("    if proc.returncode:", "kw"),
    ("        raise RuntimeError(proc.stderr.decode())", "txt"),
    ("    return out", "kw"),
]

DARK = dict(
    desk_a="#2c333e", desk_b="#12151a", panel="rgba(12,14,18,0.72)", panel_fg="#9aa1ad",
    card="#1c2026", card_bar="#23272e", card_line="rgba(255,255,255,0.06)",
    gutter="#4d5560", txt="#c6ccd6", cmt="#7f8894", kw="#c084d8", fn="#68b3f0",
    num="#e0a35c", str="#8fbf72",
    term_bg="rgba(14,16,20,0.97)", term_bar="#191d23", term_fg="#a9b2bd",
    header="#282c33", header_rec="#302a2c", meta="#8b939e", emph="#c3c9d2",
    rule="rgba(0,0,0,0.45)", status_bg="rgba(16,18,22,0.92)",
    sheet_bg="rgba(16,18,22,0.95)", sheet_fg="#c3c9d2", outline="rgba(255,255,255,0.16)",
    chip="#a7aeb9", pop="#23272e", pop_line="rgba(255,255,255,0.10)", pop_fg="#dfe3e9",
    cursor="#ffffff", cursor_edge="#11151a", ring="rgba(255,255,255,0.85)",
)
LIGHT = dict(
    desk_a="#e6ebf1", desk_b="#b9c2ce", panel="rgba(252,252,253,0.82)", panel_fg="#5b636d",
    card="#fdfdfe", card_bar="#eef0f3", card_line="rgba(0,0,0,0.09)",
    gutter="#b3b9c2", txt="#2d3238", cmt="#8d949e", kw="#8a3fa0", fn="#1a6fc4",
    num="#a85800", str="#3d7a2e",
    term_bg="rgba(255,255,255,0.98)", term_bar="#f1f2f5", term_fg="#4a5058",
    header="#e9ecf0", header_rec="#f6e9e9", meta="#5c6570", emph="#2f3640",
    rule="rgba(0,0,0,0.14)", status_bg="rgba(247,249,251,0.95)",
    sheet_bg="rgba(248,249,251,0.97)", sheet_fg="#3b424b", outline="rgba(0,0,0,0.18)",
    chip="#5c6570", pop="#ffffff", pop_line="rgba(0,0,0,0.10)", pop_fg="#2f3640",
    cursor="#1b2027", cursor_edge="#ffffff", ring="rgba(20,28,40,0.7)",
)

# ---- timeline, ported verbatim from the design ------------------------------
CLICKS = [1.25, 5.0, 12.0, 19.0, 21.25, 23.0, 28.0, 35.0]
STEPS = [
    (0, "Idle"), (1.25, "Settings"), (4.2, "Idle"), (5.0, "Recording"),
    (12.0, "Encoding"), (15.4, "Saved"), (20.2, "Mode menu"), (21.25, "Snapshot"),
    (23.25, "PNG saved"), (26.5, "Idle"), (28.0, "Recording"),
    (30.4, "Frame moved"), (31.0, "Aborted"), (35.0, "Idle"),
]
IDLE_POINT = (REC_CX - 180, WY + 300)
CURSOR = [
    (0, IDLE_POINT), (1.1, (GEAR_CX, HEADER_CY)), (4.2, (GEAR_CX, HEADER_CY)),
    (5.0, (REC_CX, HEADER_CY)), (11.0, (REC_CX, HEADER_CY)), (12.0, (REC_CX, HEADER_CY)),
    (15.4, (REC_CX + 120, WY + 280)), (18.8, (WX + WW - 150, SHEET_CY)),
    (19.0, (WX + WW - 150, SHEET_CY)), (20.2, (ARROW_CX, HEADER_CY)),
    (21.25, (ARROW_CX, HEADER_CY)), (22.6, (REC_CX, HEADER_CY)),
    (23.0, (REC_CX, HEADER_CY)), (26.5, (WX + WW - 150, SHEET_CY)),
    (27.8, (REC_CX, HEADER_CY)), (28.0, (REC_CX, HEADER_CY)),
    (30.0, (REC_CX - 220, WY + 240)), (31.0, (REC_CX - 160, WY + 280)),
    (34.6, (WX + WW - 90, SHEET_CY)), (35.0, (WX + WW - 90, SHEET_CY)),
    (LOOP, IDLE_POINT),
]


def ease(u):
    return 2 * u * u if u < 0.5 else 1 - pow(-2 * u + 2, 2) / 2


def cursor_at(t):
    for (ta, a), (tb, b) in zip(CURSOR, CURSOR[1:]):
        if ta <= t <= tb:
            u = 1.0 if tb == ta else ease((t - ta) / (tb - ta))
            return (a[0] + (b[0] - a[0]) * u, a[1] + (b[1] - a[1]) * u)
    return CURSOR[0][1]


def scene(t):
    """Exactly the design's scene(t), in Python."""
    s = dict(phase="idle", sheet=None, accent="#3689e6", timer="0:00",
             status="Position the frame, then Record.", status_right="",
             enc_label="", enc_frames="", sheet_title="", sheet_path="",
             fmt="GIF", progress=0.0, popover=False, menu=False,
             off=(0, 0), shell="awaiting capture…")
    clock = lambda sec: "0:%02d" % int(sec)

    if 1.25 <= t < 4.2:
        s["popover"] = True
    elif 5.0 <= t < 12.0:
        s.update(phase="recording", accent="#e04b4b", timer=clock(t - 5),
                 status="15 fps · pointer captured · Esc to stop",
                 shell="capturing frame %d…" % int((t - 5) * 15))
    elif 12.0 <= t < 15.4:
        u = (t - 12) / 3.4
        s.update(phase="encoding", progress=u,
                 enc_label="Encoding %d%%" % round(u * 100),
                 enc_frames="frame %d / 126" % round(u * 126),
                 status="Quantising palette · 256 colours",
                 status_right="~%d s left" % max(1, int((1 - u) * 4 + 0.999)),
                 shell="gifski: %d/126 frames…" % round(u * 126))
    elif 15.4 <= t < 19.0:
        s.update(sheet="saved", sheet_title="Saved · 1.8 MiB · 7.0 s",
                 sheet_path="~/Videos/glimpse-2026-08-26_09-41.gif",
                 shell="wrote ~/Videos/glimpse-2026-08-26_09-41.gif")
    elif 20.2 <= t < 21.25:
        s["menu"] = True
    elif 21.25 <= t < 23.25:
        s.update(phase="snapshot", fmt="PNG",
                 status="One still frame, saved as PNG.", status_right="Print Screen",
                 shell="mode: snapshot (png)")
        if t >= 23.0:
            s["accent"] = "#ffffff"
    elif 23.25 <= t < 26.5:
        s.update(phase="snapshot", fmt="PNG", sheet="saved",
                 sheet_title="Saved · 214 KiB · 754 × 438",
                 sheet_path="~/Pictures/glimpse-2026-08-26_09-42.png",
                 shell="wrote ~/Pictures/glimpse-2026-08-26_09-42.png")
    elif 26.5 <= t < 27.8:
        s.update(phase="snapshot", fmt="PNG",
                 status="One still frame, saved as PNG.", status_right="Print Screen")
    elif 28.0 <= t < 31.0:
        s.update(phase="recording", accent="#e04b4b", timer=clock(t - 28),
                 status="15 fps · pointer captured · Esc to stop",
                 shell="capturing frame %d…" % int((t - 28) * 15))
        if t >= 30.4:
            u = min(1.0, (t - 30.4) / 0.45)
            s["off"] = (46 * ease(u), -22 * ease(u))
            s["shell"] = "warn: region origin changed"
    elif 31.0 <= t < 35.0:
        s.update(phase="aborted", accent="#e5a50a", sheet="failed", off=(46, -22),
                 sheet_title="Frame moved during recording — encode aborted at 3.1 s",
                 sheet_path="Raw capture kept: /tmp/glimpse-2477725-0/recording.mkv",
                 shell="warn: aborting encode, keeping source")
    return s


def esc(t):
    return t.replace("&", "&amp;").replace("<", "&lt;").replace(">", "&gt;")


def text(x, y, s, fill, size=11.5, family=FONT, weight=None, anchor=None, mono_ls=False):
    if not s:
        return ""
    a = f' text-anchor="{anchor}"' if anchor else ""
    w = f' font-weight="{weight}"' if weight else ""
    return (f'<text x="{x}" y="{y}" font-family="{family}" font-size="{size}"'
            f' fill="{fill}"{w}{a}>{esc(s)}</text>')


def backdrop(p, shell):
    o = [f'<rect width="{W}" height="{H}" fill="{p["desk_b"]}"/>',
         f'<rect width="{W}" height="{H}" fill="{p["desk_a"]}" opacity="0.55"/>',
         f'<rect width="{W}" height="26" fill="{p["panel"]}"/>',
         text(14, 17, "Applications", p["panel_fg"], 11),
         text(W / 2, 17, "Tue 09:41", p["panel_fg"], 11, anchor="middle"),
         text(W - 14, 17, "◇ ◈ ▲", p["panel_fg"], 11, anchor="end")]
    # The editor behind the window. It fills the canvas because its only job is
    # to be visible THROUGH the hole — that is what makes the transparency read.
    ex, ey, ew, eh = 16, 34, W - 32, H - 50
    o += [f'<rect x="{ex}" y="{ey}" width="{ew}" height="{eh}" rx="9" fill="{p["card"]}" stroke="{p["card_line"]}"/>',
          f'<path d="M{ex} {ey+9} a9 9 0 0 1 9 -9 h{ew-18} a9 9 0 0 1 9 9 v25 h-{ew} Z" fill="{p["card_bar"]}"/>',
          f'<circle cx="{ex+18}" cy="{ey+17}" r="5" fill="{p["gutter"]}"/>',
          text(ex + 34, ey + 21, "encoder.py", p["txt"], 11),
          text(ex + 106, ey + 21, "palette.py", p["cmt"], 11),
          text(ex + 172, ey + 21, "region.py", p["cmt"], 11)]
    y = ey + 54
    for i, (line, kind) in enumerate(CODE):
        o.append(text(ex + 40, y, str(i + 1), p["gutter"], 12, MONO, anchor="end"))
        if line.strip():
            indent = len(line) - len(line.lstrip())
            o.append(text(ex + 52 + indent * 7.2, y, line.strip(), p[kind], 12, MONO))
        y += 22
    # The shell line still narrates each beat, tucked along the bottom where it
    # informs without competing.
    o.append(text(ex + 14, H - 12, "› " + shell, p["cmt"], 11, MONO))
    return "".join(o)


def window(p, s):
    ox, oy = s["off"]
    x, y = WX + ox, WY + oy
    hy = y + HEADER_H + RULE_H
    sy = hy + HOLE_H
    head_bg = p["header_rec"] if s["phase"] in ("recording",) else p["header"]
    rec = s["phase"] == "recording"
    o = [f'<g>',
         f'<path d="M{x} {y+10} a10 10 0 0 1 10 -10 h{WW-20} a10 10 0 0 1 10 10 v{HEADER_H-10} h-{WW} Z" fill="{head_bg}"/>']

    # meta: dot, timer, dimensions
    mx = x + 16
    if rec:
        o.append(f'<circle cx="{mx+4}" cy="{y+22}" r="4.5" fill="#e04b4b"/>')
        o.append(text(mx + 16, y + 27, s["timer"], "#f0d4d4", 15, weight="500"))
        mx += 62
    if s["phase"] == "encoding":
        o.append(text(mx, y + 26, s["enc_label"], p["emph"], 12, weight="500"))
        o.append(text(mx + 88, y + 26, s["enc_frames"], p["meta"], 12))
    else:
        o.append(text(mx, y + 26, "754 × 438", p["meta"], 12))

    # split button
    label = {"snapshot": "Snapshot", "recording": "Stop", "encoding": "Cancel"}.get(s["phase"], "Record")
    btn = "#c6262e" if s["phase"] == "recording" else "#3689e6"
    bw = 104 if label in ("Snapshot",) else 88
    bx = REC_CX + ox - bw // 2 - 14
    o += [f'<rect x="{bx}" y="{y+12}" width="{bw}" height="20" rx="10" fill="{btn}"/>',
          f'<rect x="{bx+bw}" y="{y+12}" width="28" height="20" rx="10" fill="{btn}"/>',
          f'<rect x="{bx+bw-6}" y="{y+12}" width="12" height="20" fill="{btn}"/>']
    if s["phase"] == "recording":
        o.append(f'<rect x="{bx+11}" y="{y+18}" width="8" height="8" fill="#fff"/>')
    elif s["phase"] != "snapshot":
        o.append(f'<circle cx="{bx+15}" cy="{y+22}" r="4" fill="#fff"/>')
    o.append(text(bx + (26 if s["phase"] != "snapshot" else 14), y + 26, label, "#ffffff", 11.5, weight="500"))
    o.append(f'<path d="M{bx+bw+9} {y+20} l5 6 l5 -6 Z" fill="#fff"/>')

    # chip + gear
    o += [f'<rect x="{x+WW-88}" y="{y+14}" width="30" height="16" rx="4" fill="none" stroke="{p["outline"]}"/>',
          text(x + WW - 83, y + 26, s["fmt"], p["chip"], 9.5, weight="500")]
    gx = x + WW - 34
    for i in range(3):
        o.append(f'<line x1="{gx}" y1="{y+17+i*5}" x2="{gx+13}" y2="{y+17+i*5}" stroke="{p["chip"]}" stroke-width="1.6" stroke-linecap="round"/>')

    # hairline, or progress
    o.append(f'<rect x="{x}" y="{y+HEADER_H}" width="{WW}" height="{RULE_H}" fill="{p["rule"]}"/>')
    if s["phase"] == "encoding":
        o.append(f'<rect x="{x}" y="{y+HEADER_H}" width="{WW*s["progress"]:.0f}" height="{RULE_H}" fill="#3689e6"/>')

    # the hole: border only, never painted
    o.append(f'<rect x="{x+1.5}" y="{hy+1.5}" width="{WW-3}" height="{HOLE_H-3}" fill="none" stroke="{s["accent"]}" stroke-width="{BORDER}"/>')

    # status strip or result sheet
    if s["sheet"]:
        edge = "rgba(229,165,10,0.55)" if s["sheet"] == "failed" else "rgba(54,137,230,0.5)"
        title_fill = "#e0b45c" if s["sheet"] == "failed" else p["sheet_fg"]
        o += [f'<path d="M{x} {sy} h{WW} v{SHEET_H-10} a10 10 0 0 1 -10 10 h-{WW-20} a10 10 0 0 1 -10 -10 Z" fill="{p["sheet_bg"]}"/>',
              f'<rect x="{x}" y="{sy}" width="{WW}" height="1" fill="{edge}"/>',
              text(x + 14, sy + 24, s["sheet_title"], title_fill, 11.5, weight="500"),
              text(x + 14, sy + 42, s["sheet_path"], p["meta"], 11, MONO)]
        for i, lbl in enumerate(["Copy Path", "Show in Files"] if s["sheet"] == "saved" else ["Copy Path", "Encode Anyway"]):
            bwid = 78 if i == 0 else 96
            bxx = x + WW - 12 - bwid - (0 if i else 102)
            o += [f'<rect x="{bxx}" y="{sy+16}" width="{bwid}" height="24" rx="5" fill="none" stroke="{p["outline"]}"/>',
                  text(bxx + bwid / 2, sy + 32, lbl, p["sheet_fg"], 11, anchor="middle")]
    else:
        o += [f'<path d="M{x} {sy} h{WW} v{STATUS_H-10} a10 10 0 0 1 -10 10 h-{WW-20} a10 10 0 0 1 -10 -10 Z" fill="{p["status_bg"]}"/>']
        left = x + 14
        if rec:
            o.append(text(left, sy + 20, "REC", "#d78f8f", 11, weight="500"))
            left += 34
        o.append(text(left, sy + 20, s["status"], "#d78f8f" if rec else p["meta"], 11))
        if s["status_right"]:
            o.append(text(x + WW - 14, sy + 20, s["status_right"], p["meta"], 11, anchor="end"))

    # settings popover
    if s["popover"]:
        px, py, pw, ph = gx - 240, y + HEADER_H + 8, 268, 236
        o += [f'<rect x="{px}" y="{py}" width="{pw}" height="{ph}" rx="8" fill="{p["pop"]}" stroke="{p["pop_line"]}"/>',
              f'<path d="M{gx-6} {py} l7 -8 l7 8 Z" fill="{p["pop"]}"/>']
        rows = [("CAPTURE", None), ("Frame rate", "10 15 24 30"), ("Capture pointer", "on"),
                ("Show capture rect", "Show"), ("OUTPUT", None), ("Format", "GIF MP4"),
                ("Save to", "Change…"), ("APPEARANCE", None), ("Theme", "Auto Light Dark")]
        ry = py + 20
        for label, ctrl in rows:
            if ctrl is None:
                o.append(text(px + 14, ry, label, p["meta"], 10, weight="600"))
                ry += 18
            else:
                o.append(text(px + 14, ry, label, p["pop_fg"], 12.5))
                o.append(text(px + pw - 14, ry, ctrl, p["meta"], 11, anchor="end"))
                ry += 24
        o.append(f'<line x1="{px+10}" y1="{ry-8}" x2="{px+pw-10}" y2="{ry-8}" stroke="{p["pop_line"]}"/>')
        o.append(text(px + 14, ry + 8, "Quit Glimpse", p["pop_fg"], 12.5))

    # mode menu under the split button's arrow
    if s["menu"]:
        mx0, my0 = bx + bw - 20, y + HEADER_H + 8
        o += [f'<rect x="{mx0}" y="{my0}" width="132" height="62" rx="8" fill="{p["pop"]}" stroke="{p["pop_line"]}"/>',
              text(mx0 + 14, my0 + 24, "Record", p["pop_fg"], 12.5),
              text(mx0 + 14, my0 + 48, "Snapshot", p["pop_fg"], 12.5)]

    o.append("</g>")
    return "".join(o)


def cursor(p, x, y, clicking):
    o = ""
    if clicking:
        o += f'<circle cx="{x}" cy="{y}" r="13" fill="none" stroke="{p["ring"]}" stroke-width="2" opacity="0.75"/>'
    o += (f'<path d="M{x} {y} l0 17 l4.4 -4.4 l3.1 6.6 l3.4 -1.6 l-3.1 -6.4 l6.2 -0.2 Z" '
          f'fill="{p["cursor"]}" stroke="{p["cursor_edge"]}" stroke-width="1.2" stroke-linejoin="round"/>')
    return o


def frame_svg(t, theme):
    p = DARK if theme == "dark" else LIGHT
    s = scene(t)
    cx, cy = cursor_at(t)
    clicking = any(k <= t < k + 0.4 for k in CLICKS)
    step = "Idle"
    for tt, lbl in STEPS:
        if t >= tt:
            step = lbl
    return (f'<svg xmlns="http://www.w3.org/2000/svg" width="{W}" height="{H}" viewBox="0 0 {W} {H}">'
            + backdrop(p, s["shell"]) + window(p, s) + cursor(p, cx, cy, clicking)
            + text(W - 16, H - 12, step, p["panel_fg"], 12, anchor="end")
            + "</svg>")


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--out", default="docs/assets/demo.gif")
    ap.add_argument("--fps", type=int, default=10)
    ap.add_argument("--scale", type=float, default=1.0)
    ap.add_argument("--theme", default="dark", choices=["dark", "light"])
    args = ap.parse_args()

    for tool in ("convert", "ffmpeg"):
        if not shutil.which(tool):
            raise SystemExit(f"{tool} is required")

    root = pathlib.Path(__file__).resolve().parent.parent
    out = (root / args.out).resolve()
    out.parent.mkdir(parents=True, exist_ok=True)

    with tempfile.TemporaryDirectory(prefix="glimpse-demo-") as tmp:
        tmp = pathlib.Path(tmp)
        n = int(LOOP * args.fps)
        for i in range(n):
            t = i / args.fps
            (tmp / f"f{i:04d}.svg").write_text(frame_svg(t, args.theme))
            subprocess.run(["convert", "-background", "none", str(tmp / f"f{i:04d}.svg"),
                            str(tmp / f"f{i:04d}.png")], check=True,
                           stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
            if i % 60 == 0:
                print(f"  {i}/{n}")
        print(f"drew {n} frames")

        mkv, pal = tmp / "demo.mkv", tmp / "palette.png"
        run = lambda a: subprocess.run(a, check=True, stdout=subprocess.DEVNULL,
                                       stderr=subprocess.DEVNULL)
        w = int(W * args.scale) // 2 * 2
        run(["ffmpeg", "-hide_banner", "-loglevel", "error", "-y", "-framerate", str(args.fps),
             "-i", str(tmp / "f%04d.png"), "-vf", f"scale={w}:-2:flags=lanczos",
             "-c:v", "ffv1", "-pix_fmt", "bgr0", str(mkv)])
        run(["ffmpeg", "-hide_banner", "-loglevel", "error", "-y", "-i", str(mkv),
             "-vf", "palettegen", str(pal)])
        run(["ffmpeg", "-hide_banner", "-loglevel", "error", "-y", "-i", str(mkv),
             "-i", str(pal), "-lavfi", "paletteuse", "-f", "gif", str(out)])

    print(f"wrote {out.relative_to(root)} ({out.stat().st_size:,} bytes)")


if __name__ == "__main__":
    main()
