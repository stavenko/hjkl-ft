"""Кадр «Яйца: микронутриенты» — фотография рук с подписями веществ.

Подписи вписаны В САМУ фотографию, а не выведены разметкой: так они лежат между
яйцами и не спорят с текстом кадра. Снимок под ними размыт сильнее исходного,
иначе буквы тонут в деталях.

Обозначения — как в таблице Менделеева: элементы символами (Se, I, Fe, Zn, P),
витамины своими именами (B12, B2, A, D), холин — словом, он не элемент. Шрифт с
засечками: речь о составе, и подпись должна выглядеть как в справочнике, а не как
надпись на плакате.

    python3 scripts/gen-eggs-hands.py [исходник.jpg]

Пишет frontend/story-img/eggs-hands.jpg. Исходник по умолчанию — тот, из которого
кадр собирали в первый раз.
"""

import sys
from pathlib import Path

from PIL import Image, ImageDraw, ImageFilter, ImageFont

ROOT = Path(__file__).resolve().parent.parent
OUT = ROOT / "frontend/story-img/eggs-hands.jpg"
SRC = Path(
    sys.argv[1]
    if len(sys.argv) > 1
    else Path.home()
    / "Downloads/hands-holding-fresh-eggs-for-easter-baking-2026-03-26-04-09-05-utc.jpg"
)

# Кадр истории: та же ширина и высота, что у прежнего файла, — иначе съедет вёрстка.
W, H = 1200, 716

SERIF = "/System/Library/Fonts/Supplemental/Times New Roman.ttf"
SERIF_BOLD = "/System/Library/Fonts/Supplemental/Times New Roman Bold.ttf"

# Подписи: (текст, доля ширины, доля высоты, кегль, жирный).
#
# Крупно — то, о чём говорит текст кадра (холин, Se, B12, I). Мельче — остальное,
# что в яйце есть, но в тексте не названо: железо, цинк, фосфор, рибофлавин,
# витамины A и D. Позиции подобраны по фотографии: подписи стоят на фоне и на
# рукаве, но не на самих яйцах и не на пальцах.
LABELS = [
    ("Холин", 0.07, 0.22, 78, True),
    ("Se", 0.30, 0.14, 92, True),
    ("B12", 0.13, 0.63, 84, True),
    ("I", 0.72, 0.20, 92, True),
    ("Fe", 0.83, 0.42, 62, False),
    ("Zn", 0.80, 0.62, 62, False),
    ("P", 0.66, 0.78, 62, False),
    ("B2", 0.28, 0.80, 56, False),
    ("A", 0.46, 0.10, 56, False),
    ("D", 0.55, 0.90, 56, False),
]


def main() -> None:
    src = Image.open(SRC).convert("RGB")

    # Кадрирование по центру под пропорцию кадра.
    target = W / H
    sw, sh = src.size
    if sw / sh > target:
        new_w = int(sh * target)
        box = ((sw - new_w) // 2, 0, (sw - new_w) // 2 + new_w, sh)
    else:
        new_h = int(sw / target)
        box = (0, (sh - new_h) // 2, sw, (sh - new_h) // 2 + new_h)
    img = src.crop(box).resize((W, H), Image.LANCZOS)

    # Размытие — фон для букв. Радиус подобран так, чтобы яйца оставались узнаваемы.
    img = img.filter(ImageFilter.GaussianBlur(radius=7))

    draw = ImageDraw.Draw(img)
    for text, fx, fy, size, bold in LABELS:
        font = ImageFont.truetype(SERIF_BOLD if bold else SERIF, size)
        x, y = int(W * fx), int(H * fy)
        # Тень — иначе светлая буква пропадает на светлом рукаве.
        for dx, dy in ((3, 3), (2, 2)):
            draw.text((x + dx, y + dy), text, font=font, fill=(0, 0, 0, 255))
        draw.text((x, y), text, font=font, fill=(255, 255, 255))

    OUT.parent.mkdir(parents=True, exist_ok=True)
    # yuvj420p: браузер не декодирует yuvj444p, который ffmpeg выдаёт по умолчанию —
    # на этом кадр однажды не нарисовался вовсе (200 в сети, complete=false в DOM).
    img.save(OUT, "JPEG", quality=88, subsampling=2, progressive=False)
    print(f"собран {OUT} ({W}×{H})")


if __name__ == "__main__":
    main()
