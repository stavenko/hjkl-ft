// КАДРЫ ИСТОРИЙ ЦЕЛИКОМ — как их видит человек: фон, текст и картинка вместе.
//
// `shot-story-widgets.mjs` собирает саму гифку виджета; здесь снимается КАДР, в
// котором она стоит, — чтобы проверить не подсветку саму по себе, а как она
// смотрится под заголовком и текстом.
//
// Кадры с виджетом снимаются ЕЩЁ И ГИФКОЙ: мигание обводки — половина смысла
// такого кадра, и на неподвижном снимке его не видно.
//
//   node scripts/shot-story-frames.mjs egg          # одна история
//   node scripts/shot-story-frames.mjs              # все, что перечислены ниже
//
// Файлы: OUTDIR (по умолчанию ./_shots/story-frames) — <история>-NN.png и
// <история>-NN.gif там, где в кадре стоит подсветка.
import { chromium } from "playwright";
import { execSync } from "node:child_process";
import { mkdirSync, rmSync } from "node:fs";
import path from "node:path";

import { openSeeded, DEFAULT_URL } from "./harness.mjs";
import { sceneSeed } from "./widget-scene.mjs";

const BASE = process.env.FE || DEFAULT_URL;
const ROOT = path.resolve(import.meta.dirname, "..");
const OUTDIR = process.env.OUTDIR || path.join(ROOT, "_shots/story-frames");

/// Истории с виджетными кадрами: id кружка в трее и сцена под ним.
const STORIES = {
  // id кружка → чему посвящена история. Номер кружка СМЕЩЁН относительно темы: у
  // Appears-флага имя предыдущей недели (week3 = `AfterActivityWeek`), а сама
  // история АНОНСИРУЕТ следующую. Отсчёт: week3 — активность, week4 — кальций,
  // week5 — железо, week6 — жиры, week7 — красное мясо, week8 — яйца.
  calcium: { storyId: "week4", week: "calcium", target: "calcium" },
  iron: { storyId: "week5", week: "iron", target: "iron" },
  fats: { storyId: "week6", week: "fats", target: "epa_dha" },
  red_meat: { storyId: "week7", week: "red_meat", target: "red_meat" },
  egg: { storyId: "week8", week: "egg", target: "egg" },
};

/// Снять экран петлёй и собрать GIF. Шаг съёмки — период кадра исходной гифки
/// (9 кадров при 11 fps, см. `highlight-gif.mjs`), поэтому мигание переносится
/// один в один; синхронизации с фазой нет, и первый кадр петли — какой попадётся.
const LOOP_FRAMES = 9;
const LOOP_STEP_MS = 91;
async function captureLoop(page, outPath) {
  const name = path.basename(outPath, ".gif");
  const dir = `/tmp/story-frame-${name}`;
  rmSync(dir, { recursive: true, force: true });
  mkdirSync(dir, { recursive: true });
  for (let f = 0; f < LOOP_FRAMES; f++) {
    await page.screenshot({ path: `${dir}/f${String(f).padStart(2, "0")}.png` });
    await page.waitForTimeout(LOOP_STEP_MS);
  }
  const pal = `/tmp/pal-story-${name}.png`;
  const src = `-framerate 11 -pattern_type glob -i '${dir}/f*.png'`;
  execSync(`ffmpeg -y ${src} -vf 'scale=430:-1:flags=lanczos,palettegen' ${pal}`, { stdio: "ignore" });
  execSync(`ffmpeg -y ${src} -i ${pal} -lavfi 'scale=430:-1:flags=lanczos[x];[x][1:v]paletteuse' -loop 0 ${outPath}`,
    { stdio: "ignore" });
}

const wanted = process.argv.slice(2);
const names = wanted.length ? wanted : Object.keys(STORIES);
for (const n of names) if (!STORIES[n]) throw new Error(`нет такой истории: ${n}`);

mkdirSync(OUTDIR, { recursive: true });
const browser = await chromium.launch({ headless: true });

for (const name of names) {
  const st = STORIES[name];
  const { context, page } = await openSeeded(browser, {
    baseUrl: BASE,
    context: { serviceWorkers: "block", viewport: { width: 430, height: 932 }, deviceScaleFactor: 2 },
    uid: `story-frames-${name}-${Math.floor(Math.random() * 1e6)}`,
    seed: sceneSeed({ week: st.week, target: st.target }),
  });
  await page.reload({ waitUntil: "domcontentloaded" });
  await page.waitForTimeout(6000);

  const circle = page.locator(`[data-story-id="${st.storyId}"]`);
  if (!(await circle.count())) {
    console.log(`${name}: кружка нет в трее — история не открыта этой сценой`);
    await context.close();
    continue;
  }
  // Кружок — прокручиваемый ряд: без прокрутки клик уходит мимо, в дашборд, и
  // вместо истории открывается карточка «Калории».
  await circle.scrollIntoViewIfNeeded();
  await page.waitForTimeout(300);
  await circle.click({ force: true });
  await page.waitForTimeout(2000);
  // Просмотрщик живёт в ПОРТАЛЕ поверх страницы: под ним по-прежнему виден
  // дашборд, поэтому и «открылось ли», и текст кадра читаются только из портала —
  // `body.innerText` вернёт дашборд даже при открытой истории.
  const viewer = "div[style*='z-index: 100']";
  const frames = await page.evaluate((sel) => {
    const v = document.querySelector(sel);
    if (!v) return 0;
    // Полоски прогресса — ряд из flex-контейнера сверху, по одной на кадр.
    const bar = [...v.querySelectorAll("div")].find((d) => /gap: 5px/.test(d.getAttribute("style") || ""));
    return bar ? bar.children.length : 0;
  }, viewer);
  if (!frames) {
    console.log(`${name}: история не открылась`);
    await context.close();
    continue;
  }
  console.log(`${name}: кадров ${frames}`);

  for (let i = 1; i <= frames; i++) {
    // Картинкам дать ЗАГРУЗИТЬСЯ, а не только отрисоваться: обложки по 100–300 КБ,
    // и на короткой паузе кадр снимается пустым.
    await page.waitForTimeout(2200);
    await page.evaluate(() => Promise.all(
      Array.from(document.images).filter((im) => !im.complete)
        .map((im) => new Promise((r) => { im.onload = im.onerror = r; }))));
    await page.waitForTimeout(500);
    const stem = `${name}-${String(i).padStart(2, "0")}`;
    const file = path.join(OUTDIR, `${stem}.png`);
    await page.screenshot({ path: file });
    // Кадр с подсветкой снимаем ещё и петлёй: мигание обводки — то, ради чего
    // такой кадр вообще стоит в истории, и по неподвижному снимку не судится.
    const animated = await page.evaluate((sel) =>
      [...document.querySelectorAll(`${sel} img`)].some((im) => /-highlight\.gif$/.test(im.src)), viewer);
    if (animated) await captureLoop(page, path.join(OUTDIR, `${stem}.gif`));
    const text = await page.evaluate((sel) =>
      (document.querySelector(sel)?.innerText || "").replace(/\s+/g, " ").slice(0, 70), viewer);
    console.log(`${name} ${i}: ${text}`);
    if (i < frames) await page.mouse.click(400, 500); // правая зона — «дальше»
  }
  await context.close();
}

await browser.close();
console.log(`\nснимки в ${OUTDIR}`);
