#!/usr/bin/env python3
"""re:Norma logo — plate-ring split into protein/fiber/micro arcs, micro = a
glowing 'nuclear' sliver centred at the BOTTOM. Shares 60/30/10, tight blends.
Renders navy mark tiles + a comparison sheet for colour selection."""
import math, subprocess, os

CX = CY = 100.0
R = 68.0
W = 26.0
STEP = 0.6
BLEND = 1.0                            # near-hard edge — tiny 1deg gradient
OVERLAP = 0.35
SHARES = (0.60, 0.30, 0.10)           # protein, fiber, micro
TILT = 18.0                           # micro accent offset from bottom (deg, toward bottom-right)
_sp, _sf, _sm = (s*360 for s in SHARES)
OFF = (180.0 - TILT) - (_sp + _sf + _sm/2)   # micro centre at 180-TILT

def hex2rgb(h):
    h = h.lstrip('#'); return tuple(int(h[i:i+2], 16) for i in (0, 2, 4))
def rgb2hex(c):
    return '#%02x%02x%02x' % tuple(max(0, min(255, int(round(x)))) for x in c)
def lerp(a, b, t):
    return tuple(a[i] + (b[i]-a[i])*t for i in range(3))
def pol(deg, r, cx, cy):
    t = math.radians(deg - 90.0); return (cx + r*math.cos(t), cy + r*math.sin(t))

def zones_for(p, f, m):
    return [(0.0, _sp, hex2rgb(p)), (_sp, _sp+_sf, hex2rgb(f)), (_sp+_sf, 360.0, hex2rgb(m))]

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

def mark_svg(z, bg):
    segs, glow = [], []
    zs_m, ze_m, _ = z[2]
    for k in range(int(360/STEP)):
        a1 = k*STEP; a2 = a1 + STEP + OVERLAP
        x1, y1 = pol(a1, R, CX, CY); x2, y2 = pol(a2, R, CX, CY)
        col = rgb2hex(color_at(a1 + STEP/2 - OFF, z))
        p = f'<path d="M{x1:.2f} {y1:.2f} A{R} {R} 0 0 1 {x2:.2f} {y2:.2f}" stroke="{col}" stroke-width="{W}" fill="none"/>'
        segs.append(p)
        if zs_m - 2 <= (a1 + STEP/2 - OFF) % 360.0 < ze_m + 2: glow.append(p)
    return f'''<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 200 200" width="400" height="400">
  <defs><filter id="g" x="-40%" y="-40%" width="180%" height="180%">
    <feGaussianBlur stdDeviation="4" result="b"/><feMerge><feMergeNode in="b"/><feMergeNode in="b"/></feMerge></filter></defs>
  <rect width="200" height="200" rx="40" fill="{bg}"/>
  <g filter="url(#g)" opacity="0.9">{''.join(glow)}</g>{''.join(segs)}
</svg>'''

OUT = os.path.dirname(os.path.abspath(__file__)); NAVY = '#0E1630'
# (label, protein, fiber, micro-acid)
# FIXED: emerald #10B981 (60) + rose #F43F5E (30); accent = ACID YELLOW (pops on navy)
GREEN, ROSE = '#10B981', '#F43F5E'
PALETTES = [
    ('#FFD400', GREEN, ROSE, '#FFD400'),   # golden acid yellow
    ('#FFF200', GREEN, ROSE, '#FFF200'),   # lemon acid yellow
    ('#EAFF00', GREEN, ROSE, '#EAFF00'),   # neon lime-yellow (most acid)
]
pngs = []
for lab, p, f, m in PALETTES:
    safe = lab.lstrip('#')   # '#' in a filename breaks the SVG <image href> (read as URL fragment)
    svgp = os.path.join(OUT, f'mark-{safe}.svg'); pngp = os.path.join(OUT, f'mark-{safe}.png')
    open(svgp, 'w').write(mark_svg(zones_for(p, f, m), NAVY))
    subprocess.run(['rsvg-convert', svgp, '-o', pngp, '-w', '360', '-h', '360'], check=True)
    pngs.append((lab, pngp))

# comparison sheet: 3 cols x 2 rows, navy tiles on a light page
COLS, CELL, PAD, LBL = 3, 150, 30, 28
rows = (len(pngs) + COLS - 1)//COLS
Wd = COLS*CELL + (COLS+1)*PAD
Hd = rows*(CELL+LBL) + (rows+1)*PAD
cells = []
for i, (lab, pngp) in enumerate(pngs):
    r, c = divmod(i, COLS)
    x = PAD + c*(CELL+PAD); y = PAD + r*(CELL+LBL+PAD)
    cells.append(f'<image href="{pngp}" x="{x}" y="{y}" width="{CELL}" height="{CELL}"/>')
    cells.append(f'<text x="{x+CELL/2}" y="{y+CELL+22}" font-family="Golos Text, Arial" font-size="20" fill="#0E1630" text-anchor="middle" font-weight="700">{lab}</text>')
sheet = f'''<svg xmlns="http://www.w3.org/2000/svg" width="{Wd}" height="{Hd}" viewBox="0 0 {Wd} {Hd}">
  <rect width="{Wd}" height="{Hd}" fill="#F1F4F9"/>{''.join(cells)}</svg>'''
sp = os.path.join(OUT, 'mark-sheet.svg'); spng = os.path.join(OUT, 'mark-sheet.png')
open(sp, 'w').write(sheet)
subprocess.run(['rsvg-convert', sp, '-o', spng, '-w', str(Wd), '-h', str(Hd)], check=True)
print(spng); print('\n'.join(p for _, p in pngs))
