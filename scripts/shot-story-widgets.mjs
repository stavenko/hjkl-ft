// ПЕРЕСЪЁМКА ВИДЖЕТНЫХ КАДРОВ ИСТОРИЙ — все разом, одной сценой.
//
// Каждый такой кадр показывает виджет и обводит на нём новый индикатор. Сцена под
// снимком одна на всех (`widget-scene.mjs`): открыто ровно то, что открыто к этой
// теме, всё зелёное, кроме героя, — герой оранжевый и потому первый в ряду.
//
// Прежде у каждой темы был свой скрипт со своим севом, и они разъезжались: у жиров
// «Баланс» вообще не попадал в ряд из семи значков, у мяса недельный потолок был
// пробит фоновым рационом. Теперь сцена одна, а скрипт задаёт только героя.
//
//   node scripts/shot-story-widgets.mjs            # все кадры
//   node scripts/shot-story-widgets.mjs iron heme  # только названные
import { chromium } from "playwright";
import path from "node:path";

import { openSeeded, DEFAULT_URL } from "./harness.mjs";
import { makeWidgetGif } from "./highlight-gif.mjs";
import { sceneSeed } from "./widget-scene.mjs";

const BASE = process.env.FE || DEFAULT_URL;
const ROOT = path.resolve(import.meta.dirname, "..");
const out = (name) => path.join(ROOT, "frontend/story-img", name);

/// Кадр: тема, герой, что обводим и куда пишем.
const SHOTS = {
  calcium: {
    week: "calcium", target: "calcium",
    sparks: ['[data-ind="Кальций"] > div', '[data-gauge="Кальций"]'],
    file: "calcium-highlight.gif",
  },
  iron: {
    week: "iron", target: "iron",
    sparks: ['[data-ind="Железо"] > div', '[data-gauge="Железо/нед"]'],
    file: "iron-highlight.gif",
  },
  heme: {
    week: "iron", target: "heme",
    sparks: ['[data-ind="Гем"] > div', '[data-gauge="Гем/нед"]'],
    file: "heme-highlight.gif",
  },
  omega: {
    week: "fats", target: "epa_dha",
    sparks: ['[data-ind="Омега-3"] > div', '[data-gauge="Омега-3/нед"]'],
    file: "fats-omega-highlight.gif",
  },
  ratio: {
    week: "fats", target: "fat_ratio",
    sparks: ['[data-ind="Баланс"] > div', '[data-gauge="Баланс жира"]'],
    file: "fats-ratio-highlight.gif",
  },
  red_meat: {
    week: "red_meat", target: "red_meat",
    sparks: ['[data-ind="Кр. мясо"] > div', '[data-gauge="Кр. мясо/нед"]'],
    file: "red-meat-highlight.gif",
  },
  processed_meat: {
    // Колбасы судятся ДНЯМИ, и своей шкалы у них нет — обводится только значок.
    week: "red_meat", target: "processed_meat",
    sparks: ['[data-ind="Колбасы"] > div'],
    file: "processed-meat-highlight.gif",
  },
  egg: {
    week: "egg", target: "egg",
    sparks: ['[data-ind="Яйца"] > div', '[data-gauge="Яйца/нед"]'],
    file: "eggs-highlight.gif",
  },
};

const wanted = process.argv.slice(2);
const names = wanted.length ? wanted : Object.keys(SHOTS);
for (const n of names) if (!SHOTS[n]) throw new Error(`нет такого кадра: ${n}`);

const browser = await chromium.launch({ headless: true });
for (const name of names) {
  const shot = SHOTS[name];
  const { context, page } = await openSeeded(browser, {
    baseUrl: BASE,
    context: { serviceWorkers: "block", deviceScaleFactor: 2 },
    uid: `story-widget-${name}-${Math.floor(Math.random() * 1e6)}`,
    seed: sceneSeed({ week: shot.week, target: shot.target }),
  });
  await page.reload({ waitUntil: "domcontentloaded" });
  await page.waitForTimeout(5000);
  await page.setViewportSize({ width: 440, height: 1200 });
  await page.waitForTimeout(3000); // дать полосам доиграть заполнение

  // Проверка перед съёмкой: герой на месте и он единственный не-зелёный.
  //
  // С повторами: виджет считает недельные величины асинхронно, и на первых
  // секундах ряд успевает постоять весь зелёным. Снимать в этот момент значит
  // получить кадр, где героя не видно вовсе.
  let row = [], orange = [], red = [];
  for (let attempt = 0; attempt < 6; attempt++) {
    row = await page.evaluate(() =>
      [...document.querySelectorAll("[data-ind]")].map((el) => [
        el.getAttribute("data-ind"),
        getComputedStyle(el.querySelector("div")).backgroundColor,
      ]));
    orange = row.filter(([, c]) => /232, *133/.test(c)).map(([n]) => n);
    red = row.filter(([, c]) => /224, *48/.test(c)).map(([n]) => n);
    if (orange.length === 1 && !red.length) break;
    await page.waitForTimeout(3000);
  }
  console.log(`${name}: оранжевых [${orange}] красных [${red}] всего ${row.length}`);
  if (red.length) throw new Error(`${name}: в ряду красный индикатор — сцена не та`);
  if (orange.length !== 1) throw new Error(`${name}: оранжевых должно быть ровно один`);

  await makeWidgetGif(page, shot.sparks, out(shot.file), {});
  console.log(`  собран ${shot.file}`);
  await context.close();
}
await browser.close();
