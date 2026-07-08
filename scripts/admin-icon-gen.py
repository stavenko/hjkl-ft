#!/usr/bin/env python3
"""re:Norma admin (ops console) app icon — the main-app plate-ring mark with the
letter «а» set into the centre hole, mirroring how the payment bot's mini-app
icon put a «$» in the middle. The «а» outline is lifted straight from the app's
own Golos Text font (instanced to 700) so it matches the product typography;
no runtime font dependency — it ships as a vector path.

Outputs into admin/icons/:  icon.svg, icon-maskable.svg and the PNG raster set
(icon-192, icon-512, apple-touch-icon 180, icon-maskable-512) plus favicon.png.
Rasterised with rsvg-convert (same tool as scripts/logo-gen.py).
"""
import os
import subprocess

from fontTools.ttLib import TTFont
from fontTools.pens.svgPathPen import SVGPathPen
from fontTools.pens.boundsPen import BoundsPen
from fontTools.varLib.instancer import instantiateVariableFont

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
FONT = os.path.join(ROOT, "admin", "fonts", "golos-latin.woff2")
ICONS = os.path.join(ROOT, "admin", "icons")

BG = "#0E1116"
GREEN, ROSE, GOLD = "#10B981", "#F43F5E", "#F5B301"
GLYPH_FILL = "#F5F5F7"   # near-white: reads on the dark centre hole
LETTER = "a"             # Latin/Cyrillic «а» are identical in Golos


def glyph_path_and_bounds(char):
    """SVG path 'd' for `char` (Golos @700) and its (xMin,yMin,xMax,yMax)."""
    font = TTFont(FONT)
    if "fvar" in font:
        instantiateVariableFont(font, {"wght": 700}, inplace=True)
    upm = font["head"].unitsPerEm
    glyph_set = font.getGlyphSet()
    cmap = font.getBestCmap()
    name = cmap[ord(char)]
    pen = SVGPathPen(glyph_set)
    glyph_set[name].draw(pen)
    bpen = BoundsPen(glyph_set)
    glyph_set[name].draw(bpen)
    return pen.getCommands(), bpen.bounds, upm


def centred_glyph(d, bounds, target_h, cx=256.0, cy=256.0):
    """Wrap path `d` in a transform that scales it to `target_h` px tall and
    centres it at (cx,cy). Font paths are y-up → flip y with scale(s,-s)."""
    x_min, y_min, x_max, y_max = bounds
    gh = y_max - y_min
    s = target_h / gh
    gcx = (x_min + x_max) / 2.0
    gcy = (y_min + y_max) / 2.0
    tx = cx - gcx * s
    ty = cy + gcy * s
    return (
        f'<g transform="translate({tx:.2f} {ty:.2f}) scale({s:.5f} {-s:.5f})" '
        f'fill="{GLYPH_FILL}"><path d="{d}"/></g>'
    )


def ring(stroke_w, r, dash_scale):
    """Three-arc plate ring (60/30/10) rotated so it starts at the top."""
    circ = 2 * 3.141592653589793 * r
    g = circ * 0.60
    ro = circ * 0.30
    go = circ * 0.10
    return f'''  <g fill="none" stroke-width="{stroke_w}" transform="rotate(-90 256 256)">
    <circle cx="256" cy="256" r="{r}" stroke="{GREEN}" stroke-dasharray="{g:.1f} {circ-g:.1f}"/>
    <circle cx="256" cy="256" r="{r}" stroke="{ROSE}" stroke-dasharray="{ro:.1f} {circ-ro:.1f}" stroke-dashoffset="{-g:.1f}"/>
    <circle cx="256" cy="256" r="{r}" stroke="{GOLD}" stroke-dasharray="{go:.1f} {circ-go:.1f}" stroke-dashoffset="{-(circ-go):.1f}"/>
  </g>'''


def build():
    d, bounds, _ = glyph_path_and_bounds(LETTER)

    # Rounded-square (any-purpose) icon: ring r=168, hole r=118.
    icon = f'''<svg xmlns="http://www.w3.org/2000/svg" width="512" height="512" viewBox="0 0 512 512">
  <rect width="512" height="512" rx="112" fill="{BG}"/>
{ring(52, 168, 1.0)}
  <circle cx="256" cy="256" r="118" fill="{BG}"/>
{centred_glyph(d, bounds, 150.0)}
</svg>
'''
    # Maskable (full-bleed, safe-zone) icon: ring r=132, hole r=94, smaller «а».
    mask = f'''<svg xmlns="http://www.w3.org/2000/svg" width="512" height="512" viewBox="0 0 512 512">
  <rect width="512" height="512" fill="{BG}"/>
{ring(40, 132, 1.0)}
  <circle cx="256" cy="256" r="94" fill="{BG}"/>
{centred_glyph(d, bounds, 118.0)}
</svg>
'''
    open(os.path.join(ICONS, "icon.svg"), "w").write(icon)
    open(os.path.join(ICONS, "icon-maskable.svg"), "w").write(mask)

    def raster(svg_name, png_name, size):
        subprocess.run(
            ["rsvg-convert", os.path.join(ICONS, svg_name),
             "-o", os.path.join(ICONS, png_name),
             "-w", str(size), "-h", str(size)],
            check=True,
        )

    raster("icon.svg", "icon-192.png", 192)
    raster("icon.svg", "icon-512.png", 512)
    raster("icon.svg", "apple-touch-icon.png", 180)
    raster("icon.svg", "favicon.png", 48)
    raster("icon-maskable.svg", "icon-maskable-512.png", 512)
    print("admin-icon-gen: wrote icon.svg, icon-maskable.svg + PNG set to", ICONS)


if __name__ == "__main__":
    build()
