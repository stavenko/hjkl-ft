// МИНИ-ФОТО ДЛЯ СТРОК ТАБЛИЦЫ КОРЗИНЫ.
//
// В строке рациона фото стоит кружком 44 px. Грузить туда панельные снимки по
// 100 КБ незачем — режем квадрат по центру и уменьшаем до 128 px (с запасом на
// retina). Источники — те же файлы, что стоят в панелях продуктов.
//
//   node scripts/basket-thumbs.mjs
//
// Какому продукту какое фото — в `basket-foods.mjs`, поле `photo`.
import { execFileSync, spawnSync } from "node:child_process";
import { existsSync, mkdirSync } from "node:fs";
import path from "node:path";

import { FOODS } from "./basket-foods.mjs";

const ROOT = path.resolve(import.meta.dirname, "..");
const SRC = path.join(ROOT, "landing-basket/img");
const OUT = path.join(SRC, "mini");
const SIZE = 128;

mkdirSync(OUT, { recursive: true });

// Фотографии режутся квадратом по центру, а ПАКШОТЫ (товар на белом фоне)
// вписываются целиком: у бутылки кефира центральный квадрат отрезает и крышку,
// и этикетку, и узнать товар становится нельзя.
const FIT = {
  cover: `crop='min(iw,ih)':'min(iw,ih)',scale=${SIZE}:${SIZE}:flags=lanczos`,
  contain: `scale=${SIZE}:${SIZE}:force_original_aspect_ratio=decrease:flags=lanczos,`
    + `pad=${SIZE}:${SIZE}:(ow-iw)/2:(oh-ih)/2:white`,
};

/// Границы товара на белом фоне.
///
/// У пакшотов половина кадра — пустое поле, и вписанный «как есть» товар выходит
/// вдвое мельче соседних фотографий. Границы ищутся по самой картинке: кадр
/// уменьшается до серого 128×128, и берётся прямоугольник пикселей темнее фона
/// (`cropdetect` на этих файлах не срабатывает — фон не чисто белый).
const PROBE = 128;
const BG_LEVEL = 244;   // всё светлее — считаем фоном
const MARGIN = 0.03;    // немного воздуха вокруг товара

function trimWhite(src) {
  const px = spawnSync("ffmpeg", ["-i", src, "-vf", `scale=${PROBE}:${PROBE},format=gray`,
    "-f", "rawvideo", "-"], { maxBuffer: 1 << 22 }).stdout;
  if (!px || px.length !== PROBE * PROBE) throw new Error(`${src}: не прочитались пиксели`);

  let x0 = PROBE, y0 = PROBE, x1 = -1, y1 = -1;
  for (let y = 0; y < PROBE; y++) {
    for (let x = 0; x < PROBE; x++) {
      if (px[y * PROBE + x] >= BG_LEVEL) continue;
      if (x < x0) x0 = x;
      if (x > x1) x1 = x;
      if (y < y0) y0 = y;
      if (y > y1) y1 = y;
    }
  }
  if (x1 < 0) throw new Error(`${src}: кадр весь светлее фона — товара не видно`);

  // Доли кадра, а не пиксели: исходники разного размера.
  const l = Math.max(0, x0 / PROBE - MARGIN), t = Math.max(0, y0 / PROBE - MARGIN);
  const r = Math.min(1, (x1 + 1) / PROBE + MARGIN), b = Math.min(1, (y1 + 1) / PROBE + MARGIN);
  return `crop=iw*${(r - l).toFixed(4)}:ih*${(b - t).toFixed(4)}`
    + `:iw*${l.toFixed(4)}:ih*${t.toFixed(4)},`;
}

const shots = new Map();
for (const f of Object.values(FOODS)) if (f.photo) shots.set(f.photo, f.photoFit);
for (const [name, fit] of shots) {
  const src = path.join(SRC, `${name}.jpg`);
  if (!existsSync(src)) throw new Error(`нет исходника ${path.relative(ROOT, src)}`);
  const vf = FIT[fit];
  if (!vf) throw new Error(`${name}: неизвестный photoFit «${fit}»`);
  const dst = path.join(OUT, `${name}.jpg`);
  const pre = fit === "contain" ? trimWhite(src) : "";
  execFileSync("ffmpeg", ["-y", "-i", src, "-vf", pre + vf, "-q:v", "4", dst], { stdio: "ignore" });
  console.log(`${name}.jpg (${fit})`);
}
const names = [...shots.keys()];
console.log(`\n${names.length} шт в ${path.relative(ROOT, OUT)}`);
