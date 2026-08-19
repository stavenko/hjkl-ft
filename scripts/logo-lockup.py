#!/usr/bin/env python3
"""re:Norma logo lockup — the brand plate-ring mark + the "re:Norma" wordmark set
in the brand font (Golos Text), outlined to vector paths so the result is
font-independent. Emits two variants:
  A) transparent background,
  B) dark navy background, hard (non-rounded) corners.
Renders crisp PNGs via rsvg-convert.
"""
import math, os, subprocess
from fontTools.ttLib import TTFont
from fontTools.pens.svgPathPen import SVGPathPen
from fontTools.varLib.instancer import instantiateVariableFont

OUT = os.path.dirname(os.path.abspath(__file__))
ROOT = os.path.dirname(OUT)
FONT = os.path.join(ROOT, "frontend/fonts/golos-latin.woff2")

# ── brand palette ─────────────────────────────────────────────────────────────
NAVY   = "#0E1630"
GREEN  = "#10B981"   # protein 60%
ROSE   = "#F43F5E"   # fiber 30%
ACID   = "#FFD400"   # micro accent (glow at the bottom)
INK    = "#0E1630"   # wordmark on light/transparent
PAPER  = "#F1F4F9"   # wordmark on dark

# ── the plate-ring mark (geometry ported from logo-gen.py) ────────────────────
CX = CY = 100.0
R, W, STEP, BLEND, OVERLAP = 68.0, 26.0, 0.6, 1.0, 0.35
SHARES = (0.60, 0.30, 0.10)
TILT = 18.0
_sp, _sf, _sm = (s * 360 for s in SHARES)
OFF = (180.0 - TILT) - (_sp + _sf + _sm / 2)

def hex2rgb(h): h = h.lstrip('#'); return tuple(int(h[i:i+2], 16) for i in (0, 2, 4))
def rgb2hex(c): return '#%02x%02x%02x' % tuple(max(0, min(255, int(round(x)))) for x in c)
def lerp(a, b, t): return tuple(a[i] + (b[i]-a[i])*t for i in range(3))
def pol(deg, r, cx, cy): t = math.radians(deg - 90.0); return (cx + r*math.cos(t), cy + r*math.sin(t))
def zones(p, f, m): return [(0.0, _sp, hex2rgb(p)), (_sp, _sp+_sf, hex2rgb(f)), (_sp+_sf, 360.0, hex2rgb(m))]

def color_at(deg, z):
    deg %= 360.0
    for i, (s, e, c) in enumerate(z):
        if s <= deg < e: zi, base, zs, ze = i, c, s, e; break
    else: zi, base, zs, ze = 2, z[2][2], z[2][0], z[2][1]
    nxt = z[(zi+1) % 3][2]; prv = z[(zi-1) % 3][2]
    de = ze - deg
    if de < BLEND: return lerp(base, nxt, (BLEND - de) / (2*BLEND))
    dsx = deg - zs
    if dsx < BLEND: return lerp(base, prv, (BLEND - dsx) / (2*BLEND))
    return base

def mark_group(dx, dy, scale):
    z = zones(GREEN, ROSE, ACID)
    zs_m, ze_m, _ = z[2]
    segs, glow = [], []
    for k in range(int(360/STEP)):
        a1 = k*STEP; a2 = a1 + STEP + OVERLAP
        x1, y1 = pol(a1, R, CX, CY); x2, y2 = pol(a2, R, CX, CY)
        col = rgb2hex(color_at(a1 + STEP/2 - OFF, z))
        p = f'<path d="M{x1:.2f} {y1:.2f} A{R} {R} 0 0 1 {x2:.2f} {y2:.2f}" stroke="{col}" stroke-width="{W}" fill="none"/>'
        segs.append(p)
        if zs_m - 2 <= (a1 + STEP/2 - OFF) % 360.0 < ze_m + 2: glow.append(p)
    return (f'<g transform="translate({dx} {dy}) scale({scale})">'
            f'<g filter="url(#glow)" opacity="0.9">{"".join(glow)}</g>{"".join(segs)}</g>')

# ── wordmark: outline "re:Norma" from Golos Text at a bold weight ──────────────
def wordmark(text, font_px, ink, accent):
    f = TTFont(FONT)
    try: instantiateVariableFont(f, {"wght": 760}, inplace=True)
    except Exception: pass
    upm = f["head"].unitsPerEm
    cmap = f.getBestCmap()
    gs = f.getGlyphSet()
    hmtx = f["hmtx"]
    s = font_px / upm
    cursor = 0.0
    parts = []
    for ch in text:
        gname = cmap.get(ord(ch))
        if gname is None: cursor += upm * 0.5; continue
        pen = SVGPathPen(gs)
        gs[gname].draw(pen)
        d = pen.getCommands()
        col = accent if ch == ":" else ink
        if d:
            parts.append(f'<path d="{d}" fill="{col}" '
                         f'transform="translate({cursor*s:.2f} 0) scale({s:.5f} {-s:.5f})"/>')
        cursor += hmtx[gname][0]
    width = cursor * s
    return "".join(parts), width

def compose(path, bg):
    MARK = 200.0
    scale = 1.0
    gap = 34.0
    font_px = 150.0
    wm, wm_w = wordmark("re:Norma", font_px, INK if bg is None else PAPER, GREEN)
    pad = 48.0
    baseline_shift = font_px * 0.735   # cap sits centred against the ring
    mark_y = pad
    wm_x = pad + MARK*scale + gap
    wm_y = mark_y + MARK*scale/2 + baseline_shift/2
    Wd = wm_x + wm_w + pad
    Hd = MARK*scale + 2*pad
    bgrect = f'<rect width="{Wd:.1f}" height="{Hd:.1f}" fill="{bg}"/>' if bg else ''
    svg = f'''<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 {Wd:.1f} {Hd:.1f}" width="{Wd:.0f}" height="{Hd:.0f}">
  <defs><filter id="glow" x="-40%" y="-40%" width="180%" height="180%">
    <feGaussianBlur stdDeviation="4" result="b"/><feMerge><feMergeNode in="b"/><feMergeNode in="b"/></feMerge></filter></defs>
  {bgrect}
  {mark_group(pad, mark_y, scale)}
  <g transform="translate({wm_x:.1f} {wm_y:.1f})">{wm}</g>
</svg>'''
    open(path, "w").write(svg)
    return Wd, Hd

for name, bg in [("renorma-logo-transparent", None), ("renorma-logo-dark", NAVY)]:
    svgp = os.path.join(OUT, name + ".svg")
    pngp = os.path.join(OUT, name + ".png")
    Wd, Hd = compose(svgp, bg)
    subprocess.run(["rsvg-convert", svgp, "-o", pngp, "-w", str(int(Wd*3)), "-h", str(int(Hd*3))], check=True)
    print(pngp, f"{Wd:.0f}x{Hd:.0f}")
