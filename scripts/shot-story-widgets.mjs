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
///
/// Кадры второй недели (`planka_*`, `indicators`) героя не имеют: там ещё нечего
/// выделять состоянием — виджет только что появился, и обводка показывает, ГДЕ он.
/// Индикаторы у них выданы три дня назад при двенадцати днях дневника: гейт недели
/// шагов ещё идёт («осталось N дней»), а ряд значков уже зелёный — овощи судят и те
/// дни, что были до выдачи индикаторов, и без еды под ними значок оранжевел бы.
const SHOTS = {
  planka_cal: {
    week: "base", target: null, days: 12, openedDaysAgo: 3,
    sparks: ['[data-gauge="Калории"]'],
    file: "dashboard-planka-cal.gif",
  },
  planka_macros: {
    week: "base", target: null, days: 12, openedDaysAgo: 3,
    sparks: ['[data-gauge="Белок"]', '[data-gauge="Фр/овощи"]'],
    file: "dashboard-planka-macros.gif",
  },
  indicators: {
    week: "base", target: null, days: 12, openedDaysAgo: 3,
    sparks: ['[data-ind="Калории"] > div', '[data-ind="Белок"] > div', '[data-ind="Фр/овощи"] > div'],
    file: "dashboard-indicators.gif",
  },
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
    seed: sceneSeed({
      week: shot.week, target: shot.target,
      ...(shot.days ? { days: shot.days } : {}),
      ...(shot.openedDaysAgo !== undefined ? { openedDaysAgo: shot.openedDaysAgo } : {}),
    }),
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
  // Кадру без героя (вторая неделя) полагается ноль оранжевых: выделять там нечего.
  const wantOrange = shot.target ? 1 : 0;
  let row = [], orange = [], red = [];
  for (let attempt = 0; attempt < 6; attempt++) {
    row = await page.evaluate(() =>
      [...document.querySelectorAll("[data-ind]")].map((el) => [
        el.getAttribute("data-ind"),
        getComputedStyle(el.querySelector("div")).backgroundColor,
      ]));
    orange = row.filter(([, c]) => /232, *133/.test(c)).map(([n]) => n);
    red = row.filter(([, c]) => /224, *48/.test(c)).map(([n]) => n);
    if (orange.length === wantOrange && !red.length && row.length) break;
    await page.waitForTimeout(3000);
  }
  console.log(`${name}: оранжевых [${orange}] красных [${red}] всего ${row.length}`);
  if (red.length) throw new Error(`${name}: в ряду красный индикатор — сцена не та`);
  if (orange.length !== wantOrange) throw new Error(`${name}: оранжевых должно быть ${wantOrange}`);

  await makeWidgetGif(page, shot.sparks, out(shot.file), {});
  console.log(`  собран ${shot.file}`);
  await context.close();
}
await browser.close();
