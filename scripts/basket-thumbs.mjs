// МИНИ-ФОТО ДЛЯ СТРОК ТАБЛИЦЫ КОРЗИНЫ.
//
// В строке рациона фото стоит кружком 44 px. Грузить туда панельные снимки по
// 100 КБ незачем — режем квадрат по центру и уменьшаем до 128 px (с запасом на
// retina). Источники — те же файлы, что стоят в панелях продуктов.
//
//   node scripts/basket-thumbs.mjs
//
// Какому продукту какое фото — в `basket-foods.mjs`, поле `photo`.
import { execFileSync } from "node:child_process";
import { existsSync, mkdirSync } from "node:fs";
import path from "node:path";

import { FOODS } from "./basket-foods.mjs";

const ROOT = path.resolve(import.meta.dirname, "..");
const SRC = path.join(ROOT, "landing-basket/img");
const OUT = path.join(SRC, "mini");
const SIZE = 128;

mkdirSync(OUT, { recursive: true });

const names = [...new Set(Object.values(FOODS).map((f) => f.photo).filter(Boolean))];
for (const name of names) {
  const src = path.join(SRC, `${name}.jpg`);
  if (!existsSync(src)) throw new Error(`нет исходника ${path.relative(ROOT, src)}`);
  const dst = path.join(OUT, `${name}.jpg`);
  execFileSync("ffmpeg", ["-y", "-i", src,
    "-vf", `crop='min(iw,ih)':'min(iw,ih)',scale=${SIZE}:${SIZE}:flags=lanczos`,
    "-q:v", "4", dst], { stdio: "ignore" });
  console.log(`${name}.jpg`);
}
console.log(`\n${names.length} шт в ${path.relative(ROOT, OUT)}`);
