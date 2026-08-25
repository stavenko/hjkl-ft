// САМАЯ ДЕШЁВАЯ КОРЗИНА, закрывающая все индикаторы программы.
//
// Считает НЕДЕЛЮ против тех же порогов, что стоят в коде приложения, — не «на глаз».
// Продукты, цены и сам счёт живут в `basket-foods.mjs`; здесь — только подбираемая
// руками корзина и вывод.
//
//   node scripts/cheap-basket.mjs                 # мужчина 88 кг, планка 2200 ккал
//   KCAL=1800 SEX=female WEIGHT=65 HEIGHT=165 node scripts/cheap-basket.mjs
//
// Рационы, которые показаны на странице корзины, живут не здесь, а в самой
// странице; пересчитывает их `price-basket-page.mjs` тем же модулем.
import { RULES, evaluate } from "./basket-foods.mjs";

// ── Корзина: граммы ЗА НЕДЕЛЮ ───────────────────────────────────────────────
const BASKET = {
  chickenThigh: 700,
  chickenBreast: 500,      // пачка грудки
  chickenLiver: 250,       // полпачки
  herring: 250,
  egg: 350,
  kefir: 1400,             // 200 г/день — семь бутылочек
  cheese: 105,             // 15 г/день
  cottage: 700,            // 100 г/день
  teos: 1050,              // 150 г/день
  cabbage: 2100,
  carrot: 700,
  beet: 700,
  apple: 700,
  buckwheat: 400,          // ~57 г/день сухой
  lentils: 700,
  syrok: 315,              // сырок 45 г в день
  pumpkinSeeds: 105,       // 15 г/день
};

// ── Счёт ────────────────────────────────────────────────────────────────────
const KCAL = Number(process.env.KCAL || 2200);
const SEX = process.env.SEX || "male";
const WEIGHT = Number(process.env.WEIGHT || 88);
const HEIGHT = Number(process.env.HEIGHT || 180);
const AGE = Number(process.env.AGE || 40);

const { sum, rows, targets: t } = evaluate(BASKET, {
  kcal: KCAL, sex: SEX, weight: WEIGHT, height: HEIGHT, age: AGE,
});

const line = (what, got, need, unit, ok) =>
  `${ok ? "✅" : "❌"} ${what.padEnd(26)} ${got.padStart(11)} / ${String(need).padStart(7)} ${unit}`;
const n = (x, d = 0) => x.toFixed(d);

console.log(`\nПРОФИЛЬ: ${SEX === "male" ? "мужчина" : "женщина"}, ${WEIGHT} кг, ${HEIGHT} см, ${AGE} лет · планка ${KCAL} ккал\n`);
console.log("КОРЗИНА НА НЕДЕЛЮ");
rows.sort((a, b) => b.cost - a.cost);
for (const r of rows) {
  console.log(`  ${r.name.padEnd(30)} ${String(r.grams).padStart(5)} г  (${String(r.day).padStart(4)} г/день)  ${n(r.cost).padStart(5)} ₽`);
  // Откуда цена: без этой строки «530 ₽/кг» нечем проверить.
  console.log(`  ${" ".repeat(30)} ${r.pack || "цена по Ozon не проверялась"}`);
}
console.log(`  ${"ИТОГО".padEnd(30)} ${" ".padStart(5)}    ${" ".padStart(4)}            ${n(sum.price).padStart(5)} ₽/нед  ·  ${n(sum.price / 7)} ₽/день`);

console.log("\nИНДИКАТОРЫ");
console.log(line("Белок (в день)", n(sum.protein / 7), t.proteinDay, "г", sum.protein / 7 >= t.proteinDay));
console.log(line("Фрукты/овощи (в день)", n(sum.veg / 7), t.vegDay, "г", sum.veg / 7 >= t.vegDay));
console.log(line("Кальций (в день)", n(sum.calcium / 7), t.calciumDay, "мг", sum.calcium / 7 >= t.calciumDay));
console.log(line("Клетчатка (в день)", n(sum.fibre / 7, 1), n(t.fiberDay, 1), "г", sum.fibre / 7 >= t.fiberDay));
console.log(line("Железо усвоенное (нед)", n(sum.ironAbs, 2), n(t.ironWeek, 2), "мг", sum.ironAbs >= t.ironWeek));
console.log(line("Гем — порции (нед)", n(sum.heme / RULES.hemePortionProtein, 2), RULES.hemePortionsWeek, "шт", sum.heme / RULES.hemePortionProtein >= RULES.hemePortionsWeek));
console.log(line("ЭПК+ДГК (нед)", n(sum.epa, 2), RULES.epaDhaWeek, "г", sum.epa >= RULES.epaDhaWeek));
console.log(line("Баланс жира", n(sum.unsat / sum.sat, 2), RULES.unsatToSatMin, "×", sum.unsat / sum.sat >= RULES.unsatToSatMin));
console.log(line("Яйца (нед)", n(sum.egg), RULES.eggGramsWeek, "г", sum.egg >= RULES.eggGramsWeek));
console.log(line("Красное мясо (нед)", n(sum.red), RULES.redMeatWeek, "г ≤", sum.red <= RULES.redMeatWeek));
console.log(line("Калории (в день)", n(sum.kcal / 7), KCAL, "ккал", Math.abs(sum.kcal / 7 - KCAL) < 50));
