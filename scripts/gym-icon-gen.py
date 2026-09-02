#!/usr/bin/env python3
"""Иконка приложения тренировок (gym.renorma.app).

Тот же фирменный знак, что и у остальных приложений, — кольцо-«тарелка» из трёх
секторов (зелёный / розовый / золотой) на тёмном поле, — но в отверстии стоит
гриф со блинами: это приложение зала, и на домашнем экране оно должно отличаться
от приложения худеющего одним взглядом, не читая подпись.

Растеризуется ЗДЕСЬ ЖЕ, без rsvg-convert и без PIL: в окружении сборки их нет, а
иконки PWA обязаны быть PNG (Android берёт 192/512, iOS — apple-touch-icon).
Отсюда свой минимальный кодировщик PNG поверх zlib и честное сглаживание
четырёхкратной передискретизацией.

Запуск:  python3 scripts/gym-icon-gen.py
Пишет в gym/icons/: icon-192.png, icon-512.png, icon-maskable-512.png,
apple-touch-icon.png, favicon.png, а также icon.svg / icon-maskable.svg.
"""
import math
import os
import struct
import zlib

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
ICONS = os.path.join(ROOT, "gym", "icons")

BG = (0x0E, 0x11, 0x16)
GREEN = (0x14, 0xB8, 0x84)
ROSE = (0xF4, 0x53, 0x6B)
GOLD = (0xF5, 0xB3, 0x01)
BAR = (0xE8, 0xEB, 0xF1)

SS = 4  # передискретизация: 4×4 отсчёта на пиксель


def sector_colour(angle):
    """Цвет кольца по углу — те же доли, что в CSS-знаке: 60% / 30% / 10%,
    отсчёт от «без четверти двенадцать» (conic-gradient from -90deg)."""
    t = ((angle + math.pi / 2) / (2 * math.pi)) % 1.0
    if t < 0.60:
        return GREEN
    if t < 0.90:
        return ROSE
    return GOLD


def barbell(x, y, r):
    """Гриф со блинами в единичных координатах от центра (в долях радиуса).

    Рисуется прямоугольниками: горизонтальный гриф, по два блина с каждой
    стороны — внутренний выше внешнего. Пропорции подобраны так, чтобы знак
    читался и в 48 px фавиконки."""
    ax, ay = abs(x), abs(y)
    # гриф
    if ax <= 0.62 * r and ay <= 0.055 * r:
        return True
    # внутренние блины
    if 0.20 * r <= ax <= 0.34 * r and ay <= 0.30 * r:
        return True
    # внешние блины
    if 0.42 * r <= ax <= 0.53 * r and ay <= 0.20 * r:
        return True
    return False


def render(size, maskable=False):
    """RGB-растр size×size. `maskable` — знак ужат в safe zone (Android обрезает
    маскируемую иконку до круга ~80% стороны, и обычный знак потерял бы края)."""
    c = size / 2.0
    scale = 0.72 if maskable else 0.92
    r_out = c * scale                 # внешний радиус кольца
    r_in = r_out * 0.74               # внутренний радиус кольца (отверстие)
    r_glyph = r_out * 0.72            # опорный радиус для грифа

    rows = []
    for py in range(size):
        row = bytearray()
        for px in range(size):
            acc = [0.0, 0.0, 0.0]
            for sy in range(SS):
                for sx in range(SS):
                    x = px + (sx + 0.5) / SS - c
                    y = py + (sy + 0.5) / SS - c
                    d = math.hypot(x, y)
                    if r_in <= d <= r_out:
                        col = sector_colour(math.atan2(y, x))
                    elif barbell(x, y, r_glyph):
                        col = BAR
                    else:
                        col = BG
                    acc[0] += col[0]
                    acc[1] += col[1]
                    acc[2] += col[2]
            n = SS * SS
            row += bytes((int(acc[0] / n + 0.5), int(acc[1] / n + 0.5), int(acc[2] / n + 0.5)))
        rows.append(bytes(row))
    return rows


def write_png(path, rows, size):
    """Минимальный PNG: 8 бит, truecolor, без фильтрации (filter type 0)."""
    raw = b"".join(b"\x00" + r for r in rows)

    def chunk(tag, data):
        return (struct.pack(">I", len(data)) + tag + data
                + struct.pack(">I", zlib.crc32(tag + data) & 0xFFFFFFFF))

    png = (b"\x89PNG\r\n\x1a\n"
           + chunk(b"IHDR", struct.pack(">IIBBBBB", size, size, 8, 2, 0, 0, 0))
           + chunk(b"IDAT", zlib.compress(raw, 9))
           + chunk(b"IEND", b""))
    with open(path, "wb") as f:
        f.write(png)
    print(f"  {os.path.relpath(path, ROOT)}  ({size}×{size}, {len(png)} B)")


def svg(maskable=False):
    """Векторная копия того же знака — для источников, которым PNG не нужен."""
    scale = 0.72 if maskable else 0.92
    r_out, r_in = 256 * scale, 256 * scale * 0.74
    stroke = r_out - r_in
    r_mid = (r_out + r_in) / 2
    circ = 2 * math.pi * r_mid
    rg = r_out * 0.72
    plates = []
    for sign in (1, -1):
        plates.append(f'<rect x="{256 + sign * 0.20 * rg if sign > 0 else 256 - 0.34 * rg:.1f}" '
                      f'y="{256 - 0.30 * rg:.1f}" width="{0.14 * rg:.1f}" height="{0.60 * rg:.1f}" rx="3" fill="#E8EBF1"/>')
        plates.append(f'<rect x="{256 + sign * 0.42 * rg if sign > 0 else 256 - 0.53 * rg:.1f}" '
                      f'y="{256 - 0.20 * rg:.1f}" width="{0.11 * rg:.1f}" height="{0.40 * rg:.1f}" rx="3" fill="#E8EBF1"/>')
    return f'''<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 512 512" width="512" height="512">
  <rect width="512" height="512" fill="#0E1116"/>
  <g fill="none" stroke-width="{stroke:.1f}" transform="rotate(-90 256 256)">
    <circle cx="256" cy="256" r="{r_mid:.1f}" stroke="#14B884"
            stroke-dasharray="{circ * 0.60:.1f} {circ * 0.40:.1f}"/>
    <circle cx="256" cy="256" r="{r_mid:.1f}" stroke="#F4536B"
            stroke-dasharray="{circ * 0.30:.1f} {circ * 0.70:.1f}" stroke-dashoffset="{-circ * 0.60:.1f}"/>
    <circle cx="256" cy="256" r="{r_mid:.1f}" stroke="#F5B301"
            stroke-dasharray="{circ * 0.10:.1f} {circ * 0.90:.1f}" stroke-dashoffset="{-circ * 0.90:.1f}"/>
  </g>
  <rect x="{256 - 0.62 * rg:.1f}" y="{256 - 0.055 * rg:.1f}" width="{1.24 * rg:.1f}" height="{0.11 * rg:.1f}" rx="3" fill="#E8EBF1"/>
  {"".join(plates)}
</svg>
'''


def main():
    os.makedirs(ICONS, exist_ok=True)
    print("иконки приложения тренировок →", os.path.relpath(ICONS, ROOT))
    for name, size, mask in [
        ("icon-192.png", 192, False),
        ("icon-512.png", 512, False),
        ("apple-touch-icon.png", 180, False),
        ("favicon.png", 48, False),
        ("icon-maskable-512.png", 512, True),
    ]:
        write_png(os.path.join(ICONS, name), render(size, mask), size)
    for name, mask in [("icon.svg", False), ("icon-maskable.svg", True)]:
        with open(os.path.join(ICONS, name), "w") as f:
            f.write(svg(mask))
        print(f"  {os.path.relpath(os.path.join(ICONS, name), ROOT)}")


if __name__ == "__main__":
    main()
