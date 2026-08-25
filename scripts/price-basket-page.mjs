// ПЕРЕСЧЁТ ТАБЛИЦ НА СТРАНИЦЕ КОРЗИНЫ (`landing-basket/index.html`).
//
// Граммы задаёт сама страница — их подбирали руками под каждую калорийность. Всё
// остальное (цена строки, итог за неделю и день, строка индикаторов, ккал в
// подписи) СЧИТАЕТСЯ здесь из `basket-foods.mjs` и переписывается в файл. Руками
// эти числа не правят: цены на Ozon меняются, а сходиться таблица обязана всегда.
//
//   node scripts/price-basket-page.mjs           # переписать страницу
//   node scripts/price-basket-page.mjs --check   # только показать, ничего не писать
//
// Строки меню пересортировываются по цене: при смене цен порядок «сначала дорогое»
// иначе врёт.
import { readFileSync, writeFileSync } from "node:fs";
import path from "node:path";

import { RULES, evaluate } from "./basket-foods.mjs";

const ROOT = path.resolve(import.meta.dirname, "..");
const PAGE = path.join(ROOT, "landing-basket/index.html");

/// Подпись в таблице → продукт. Названия на странице человеческие и не совпадают
/// с ключами; расходятся — падаем, а не считаем молча мимо.
const BY_LABEL = {
  "Греческий йогурт 2%": "teos",
  "Творог 5%": "cottage",
  "Куриное бедро": "chickenThigh",
  "Куриная грудка": "chickenBreast",
  "Сырок глазированный": "syrok",
  "Кефир 2.5%": "kefir",
  "Чечевица": "lentils",
  "Сыр полутвёрдый 45%": "cheese",
  "Капуста белокочанная": "cabbage",
  "Яблоки": "apple",
  "Печень куриная": "chickenLiver",
  "Яйцо куриное": "egg",
  "Сельдь солёная": "herring",
  "Гречка": "buckwheat",
  "Морковь": "carrot",
  "Свёкла": "beet",
  "Семечки": "pumpkinSeeds",
  "Масло подсолнечное": "sunflowerOil",
};
const LABEL_BY_KEY = Object.fromEntries(
  Object.entries(BY_LABEL).map(([label, key]) => [key, label]));

/// Кому какой рацион. Тело — то же, на котором рационы подбирались.
const PROFILES = {
  f1800: { kcal: 1800, sex: "female", weight: 65, height: 165, age: 40 },
  f2200: { kcal: 2200, sex: "female", weight: 65, height: 165, age: 40 },
  m2200: { kcal: 2200, sex: "male", weight: 88, height: 180, age: 40 },
  m2500: { kcal: 2500, sex: "male", weight: 88, height: 180, age: 40 },
};

const check = process.argv.includes("--check");
let html = readFileSync(PAGE, "utf8");
let failed = 0;

// Блок рациона: от `<div class="ration" data-r="…">` до закрывающего его `</div>`
// на том же отступе (в файле — четыре пробела).
const RATION = /( {4}<div class="ration" data-r="(\w+)"[^>]*>\n)([\s\S]*?)(\n {4}<\/div>)/g;

html = html.replace(RATION, (whole, head, id, body, tail) => {
  const profile = PROFILES[id];
  if (!profile) throw new Error(`неизвестный рацион: ${id}`);

  // Граммы — из самой страницы.
  const basket = {};
  const rowRe = /<div class="mname"><b>([^<]+)<\/b><\/div>\s*<div class="mqty">(\d+) г<\/div>/g;
  for (const m of body.matchAll(rowRe)) {
    const key = BY_LABEL[m[1]];
    if (!key) throw new Error(`${id}: нет продукта под подписью «${m[1]}»`);
    basket[key] = Number(m[2]);
  }
  if (!Object.keys(basket).length) throw new Error(`${id}: строк не найдено`);

  const { sum, rows, targets: t } = evaluate(basket, profile);
  rows.sort((a, b) => b.cost - a.cost);

  const menu = rows.map((r) => [
    '          <div class="mrow">',
    `            <div class="mname"><b>${LABEL_BY_KEY[r.key]}</b></div>`,
    `            <div class="mqty">${r.grams} г</div>`,
    `            <div class="mcost">${Math.round(r.cost)} ₽</div>`,
    "          </div>",
  ].join("\n")).join("\n");

  const n = (x, d = 0) => x.toFixed(d);
  const inds = [
    ["Белок", `${n(sum.protein / 7)} / ${t.proteinDay} г`, sum.protein / 7 >= t.proteinDay],
    ["Фрукты и овощи", `${n(sum.veg / 7)} / ${t.vegDay} г`, sum.veg / 7 >= t.vegDay],
    ["Кальций", `${n(sum.calcium / 7)} / ${t.calciumDay} мг`, sum.calcium / 7 >= t.calciumDay],
    ["Клетчатка", `${n(sum.fibre / 7)} / ${n(t.fiberDay)} г`, sum.fibre / 7 >= t.fiberDay],
    ["Железо", `${n(sum.ironAbs, 1)} / ${n(t.ironWeek, 1)} мг`, sum.ironAbs >= t.ironWeek],
    ["Гемовое железо", `${n(sum.heme / RULES.hemePortionProtein)} / ${RULES.hemePortionsWeek} порции`,
      sum.heme / RULES.hemePortionProtein >= RULES.hemePortionsWeek],
    ["Омега-3", `${n(sum.epa, 1)} / ${RULES.epaDhaWeek} г`, sum.epa >= RULES.epaDhaWeek],
    ["Баланс жиров", `${n(sum.unsat / sum.sat, 1)} / ${n(RULES.unsatToSatMin, 1)}`,
      sum.unsat / sum.sat >= RULES.unsatToSatMin],
    ["Яйца", `${Math.round(sum.egg / 50)} / ${Math.round(RULES.eggGramsWeek / 50)} шт`,
      sum.egg >= RULES.eggGramsWeek],
  ];

  // Рацион, который что-то НЕ закрывает, показывать нельзя — вся страница про то,
  // что закрывает всё. Сообщаем и валим прогон.
  const kcalDay = sum.kcal / 7;
  const kcalOk = Math.abs(kcalDay - profile.kcal) < 50;
  const bad = inds.filter(([, , ok]) => !ok).map(([name]) => name);
  if (bad.length || !kcalOk) {
    failed++;
    console.log(`❌ ${id}: не закрыто [${bad.join(", ")}]${kcalOk ? "" : ` · ккал ${n(kcalDay)} против ${profile.kcal}`}`);
  } else {
    console.log(`✅ ${id}: ${n(sum.price)} ₽/нед · ${n(sum.price / 7)} ₽/день · ${n(kcalDay)} ккал`);
  }

  const indsHtml = inds
    .map(([name, val]) => `        <span class="ind">${name} <i>${val}</i></span>`)
    .join("\n");

  const rebuilt = [
    '      <div class="card" style="margin-top:16px">',
    '        <div class="menu">',
    menu,
    "        </div>",
    '        <div class="mtotal">',
    `          <div><div class="big">${Math.round(sum.price)} ₽</div><div class="sub">в неделю</div></div>`,
    `          <div style="text-align:right"><div class="big">${Math.round(sum.price / 7)} ₽</div><div class="sub">в день</div></div>`,
    "        </div>",
    "      </div>",
    `      <p class="lead" style="margin-top:16px">Закрыто на ${Math.round(kcalDay)} ккал в день:</p>`,
    '      <div class="inds">',
    indsHtml,
    "      </div>",
  ].join("\n");

  return head + rebuilt + tail;
});

if (!check) writeFileSync(PAGE, html);
console.log(check ? "\n(--check: файл не тронут)" : `\nпереписан ${path.relative(ROOT, PAGE)}`);
process.exit(failed ? 1 : 0);
