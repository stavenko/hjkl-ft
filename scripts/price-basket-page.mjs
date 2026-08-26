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

import { RULES, evaluate, packLabel } from "./basket-foods.mjs";

const ROOT = path.resolve(import.meta.dirname, "..");
const PAGE = path.join(ROOT, "landing-basket/index.html");

/// Как продукт подписан в таблице. Название держится в одну строку, поэтому оно
/// короткое: жирность и фасовка всё равно видны в окошке с ценой пачки.
const LABEL_BY_KEY = {
  teos: "Греческий йогурт",
  cottage: "Творог 5%",
  chickenThigh: "Куриное бедро",
  chickenBreast: "Куриная грудка",
  syrok: "Сырок глазированный",
  kefir: "Кефир 2.5%",
  lentils: "Чечевица",
  cheese: "Сыр 45%",
  cabbage: "Капуста",
  apple: "Яблоки",
  chickenLiver: "Печень куриная",
  egg: "Яйцо куриное",
  mackerel: "Скумбрия",
  buckwheat: "Гречка",
  carrot: "Морковь",
  beet: "Свёкла",
  pumpkinSeeds: "Семечки",
  oliveOil: "Масло оливковое",
};

/// Подпись в файле → продукт. Кроме нынешних имён здесь лежат ПРЕЖНИЕ: граммы
/// читаются из уже сохранённой страницы, и переименование не должно её ронять.
const BY_LABEL = {
  ...Object.fromEntries(Object.entries(LABEL_BY_KEY).map(([key, label]) => [label, key])),
  "Греческий йогурт 2%": "teos",
  "Капуста белокочанная": "cabbage",
  "Сыр полутвёрдый 45%": "cheese",
  "Сельдь солёная": "herring",
  "Масло подсолнечное": "sunflowerOil",
};

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

  // Граммы — из самой страницы. Бренд в подписи не мешает: имя продукта лежит в
  // <b>, и берётся именно оно.
  const basket = {};
  const rowRe = /<div class="mname"><b>([^<]+)<\/b>[\s\S]*?class="mqty[^"]*"[^>]*>(\d+) г</g;
  for (const m of body.matchAll(rowRe)) {
    const key = BY_LABEL[m[1]];
    if (!key) throw new Error(`${id}: нет продукта под подписью «${m[1]}»`);
    basket[key] = Number(m[2]);
  }
  if (!Object.keys(basket).length) throw new Error(`${id}: строк не найдено`);

  const { sum, rows, targets: t } = evaluate(basket, profile);
  rows.sort((a, b) => b.cost - a.cost);

  const menu = rows.map((r) => {
    if (!r.pack) throw new Error(`${id}: у «${r.name}» нет карточки Ozon — цену нечем подтвердить`);
    // Фото есть не у всех позиций; вместо чужой картинки — плашка с буквой.
    const label = LABEL_BY_KEY[r.key];
    const pic = r.photo
      ? `            <img class="mpic" src="/img/mini/${r.photo}.jpg" alt="" loading="lazy" width="44" height="44" />`
      : `            <span class="mpic mpic-blank" aria-hidden="true">${label[0]}</span>`;
    // У весового товара (капуста, морковь) бренда нет — и повторять название
    // второй строкой незачем.
    const brand = r.pack.brand || "весовой товар";
    // Два хинта на строку: вес разворачивает суточную порцию, цена — карточку.
    const qtyId = `qty-${id}-${r.key}`;
    const popId = `pack-${id}-${r.key}`;
    return [
      '          <div class="mrow">',
      pic,
      `            <div class="mname"><b>${label}</b><span>${brand}</span></div>`,
      `            <button class="mqty mhint" type="button" aria-expanded="false" aria-controls="${qtyId}">${r.grams} г</button>`,
      `            <button class="mcost mhint" type="button" aria-expanded="false" aria-controls="${popId}">${Math.round(r.cost)} ₽</button>`,
      `            <div class="mpop" id="${qtyId}" role="tooltip" hidden>${r.grams} г на неделю<i>это ${r.day} г в день</i></div>`,
      `            <div class="mpop" id="${popId}" role="tooltip" hidden>${packLabel(r.pack)}<i>${r.pricePerKg} ₽/кг</i></div>`,
      "          </div>",
    ].join("\n");
  }).join("\n");

  const n = (x, d = 0) => x.toFixed(d);
  /// «1 порция, 2 порции, 10 порций» — иначе в тексте вылезает «10 порции».
  const portions = (x) => {
    const k = Math.round(x), last = k % 10, two = k % 100;
    if (two >= 11 && two <= 14) return `${k} порций`;
    if (last === 1) return `${k} порция`;
    if (last >= 2 && last <= 4) return `${k} порции`;
    return `${k} порций`;
  };
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
    console.log(`✅ ${id}: ${n(sum.price)} ₽/нед · ${n(sum.price / 7)} ₽/день · ${n(kcalDay)} ккал`
      + ` · магний ${n(sum.magnesium / 7)}/${t.magnesiumDay} мг · вит. D ${n(sum.vitD / 7, 1)}/${t.vitDDay} мкг`);
  }

  // Буллеты — теми же числами, что и проверка индикаторов выше. Плашками
  // «Белок 148 / 135» на странице их не дублируем: одно и то же дважды, только
  // без объяснения, зачем это читателю.
  //
  // Магний и витамин D индикаторов в приложении не имеют — они считаются только
  // здесь, против RDA.
  const fish = rows.find((r) => r.key === "mackerel" || r.key === "herring");
  const pros = [
    ["Калорийность", `${Math.round(kcalDay)} ккал в день — рацион собран ровно под неё.`],
    ["Белок", `${n(sum.protein / 7)} г в день — на дефиците уходит жир, а не мышцы.`],
    // Норма по овощам — НАША, а не общепринятая, и ссылаться на неё как на
    // внешнюю нельзя. Говорим о том, что эти граммы дают человеку.
    ["Фрукты и овощи", `${n(sum.veg / 7)} г в день позволяют чувствовать себя сытым`
      + ` при минимальной калорийности — капуста, морковь, свёкла и яблоки.`],
    ["Клетчатка", `${n(sum.fibre / 7)} г в день при норме ${n(t.fiberDay)} — из капусты, чечевицы и гречки, а не из добавок.`],
    ["Кальций", `${n(sum.calcium / 7)} мг в день при норме ${t.calciumDay} — кефир, творог, йогурт и сыр.`],
    ["Железо", `${n(sum.ironAbs, 1)} мг усвоенного железа в неделю`
      + ` (при норме в <button class="mhint" type="button" aria-expanded="false"`
      + ` aria-controls="iron-${id}">${n(t.ironWeek, 1)}</button>).`
      + ` Большое количество биодоступного, гемового железа.`
      + `<span class="mpop" id="iron-${id}" role="tooltip" hidden>Из еды усваивается около`
      + ` ${Math.round(RULES.ironBioavailability * 100)} % железа. Норма поступления`
      + ` ${(t.ironWeek / 7 / RULES.ironBioavailability).toFixed(1)} мг в сутки —`
      + ` это ${n(t.ironWeek, 1)} мг усвоенного за неделю.</span>`],
    ["Дешёвая омега-3", `${n(sum.epa, 1)} г ЭПК+ДГК в неделю — из ${fish.grams} г рыбы за ${Math.round(fish.cost)} ₽.`],
    ["Жировой профиль", `Ненасыщенных в ${n(sum.unsat / sum.sat, 1)} раза больше насыщенных — снижаем риски сердечно-сосудистых болезней.`],
    ["Магний", `${n(sum.magnesium / 7)} мг в день при норме ${t.magnesiumDay} — гречка, семечки и чечевица.`],
  ];
  // Витамин D попадает в список, ТОЛЬКО если рацион его закрывает: обычной едой
  // он не набирается, и написать «закрыт», когда это не так, нельзя. Если не
  // закрыт — молчим на странице и ругаемся в прогоне, чтобы это не прошло мимо.
  const vitDday = sum.vitD / 7;
  if (vitDday >= t.vitDDay) {
    pros.splice(7, 0, ["Витамин D", `${n(vitDday, 1)} мкг в день при норме ${t.vitDDay} — редкий случай,`
      + ` когда он набирается едой, а не добавкой. Даёт та же жирная рыба.`]);
  } else {
    console.log(`   ⚠️  ${id}: витамин D ${n(vitDday, 1)} из ${t.vitDDay} мкг — на странице про него ни слова`);
  }
  const bullets = (items) => items
    .map(([head, text]) =>
      `        <li><span class="check">✓</span><span><b>${head}.</b> ${text}</span></li>`)
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
    '      <ul class="list" style="margin-top:18px">',
    bullets(pros),
    "      </ul>",
  ].join("\n");

  return head + rebuilt + tail;
});

if (!check) writeFileSync(PAGE, html);
console.log(check ? "\n(--check: файл не тронут)" : `\nпереписан ${path.relative(ROOT, PAGE)}`);
process.exit(failed ? 1 : 0);
