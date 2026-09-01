// СХЛОПЫВАНИЕ ПОЧТИ-ПОВТОРОВ В ВЫДАЧЕ ПОИСКА.
//
// Проверка при заведении (measure-food-pick.mjs) не даёт появиться новой копии —
// но до тех, что уже лежат, она не дотягивается: они приехали синхронизацией или
// остались от старого пути записи. Человек ищет «макароны» и видит пять строк, в
// которых одно и то же.
//
// Схлопывается ТОЛЬКО неразличимое: одно и то же написание с точностью до описки
// И одни и те же нутриенты. «Макароны» рядом с «вермишелью» — не повтор, это
// разные слова и человек завёл их сознательно. «Макароны» с другими числами —
// тоже не повтор: по спеке (§6.4) отличие в числах и есть отдельный продукт.
//
// Здесь нет ни модели, ни сети: всё решается кодом, а значит и проверяться должно
// точно — списком того, что обязано схлопнуться, и того, что обязано остаться.
//
//   node scripts/measure-search-dedup.mjs

import { readFileSync } from "node:fs";

const DATA = JSON.parse(readFileSync("scripts/fixtures/user-foods-messy.json", "utf8"));
const FOODS = DATA.foods;

/// Имя к сравнимому виду: регистр, ё, пунктуация и пробелы прочь. Пробелы убираем
/// целиком — «ма кароны» это те же макароны с промахом по клавише.
const canonName = (s) => s.toLowerCase().replace(/ё/g, "е").replace(/[^а-яa-z0-9]/g, "");

function distance(a, b) {
  if (a === b) return 0;
  let prev = Array.from({ length: b.length + 1 }, (_, i) => i);
  for (let i = 1; i <= a.length; i++) {
    const cur = [i];
    for (let j = 1; j <= b.length; j++) {
      cur[j] = Math.min(prev[j] + 1, cur[j - 1] + 1, prev[j - 1] + (a[i - 1] === b[j - 1] ? 0 : 1));
    }
    prev = cur;
  }
  return prev[b.length];
}

/// Одно написание с точностью до описки. Короткие слова не трогаем: «мясо» и
/// «масло» — две правки из пяти букв, а еда разная.
function sameName(a, b) {
  const x = canonName(a), y = canonName(b);
  if (!x || !y) return false;
  if (x === y) return true;
  const len = Math.min(x.length, y.length);
  if (len < 6) return false;
  return distance(x, y) <= (len >= 10 ? 2 : 1);
}

/// Нутриенты совпадают с точностью до округления на упаковке.
const sameNutrition = (a, b) =>
  ["kcal", "protein", "fat", "carbs"].every((k) =>
    Math.abs((a[k] ?? 0) - (b[k] ?? 0)) <= Math.max(k === "kcal" ? 1 : 0.1, Math.abs(b[k] ?? 0) * 0.01));

/// Неразличимые для человека записи — в одну группу. Требуется И имя, И числа:
/// хватило бы одного имени — склеились бы макароны с разной калорийностью, а это
/// по спеке разные продукты; хватило бы одних чисел — склеились бы макароны с
/// вермишелью, которые человек развёл нарочно.
function cluster(foods) {
  const groups = [];
  for (const f of foods) {
    const g = groups.find((g) => sameName(g[0].name, f.name) && sameNutrition(g[0], f));
    if (g) g.push(f); else groups.push([f]);
  }
  return groups;
}

const groups = cluster(FOODS);

console.log(`в базе ${FOODS.length} записей, в выдаче будет ${groups.length}\n`);
for (const g of groups) {
  const head = `${g[0].name} — ${g[0].kcal} ккал`;
  console.log(g.length > 1
    ? `  ${head}   ← схлопнуто ${g.length}: ${g.map((f) => `«${f.name}»`).join(", ")}`
    : `  ${head}`);
}

// ── проверка против ожидаемого ──

const groupOf = (id) => groups.findIndex((g) => g.some((f) => f.id === id));
const problems = [];

for (const want of DATA.ожидается.схлопнуть) {
  const idx = want.map(groupOf);
  if (new Set(idx).size !== 1) problems.push(`не схлопнулись в одно: ${want.join(", ")}`);
  const g = groups[idx[0]];
  if (g && g.length !== want.length) {
    const extra = g.filter((f) => !want.includes(f.id)).map((f) => f.id);
    if (extra.length) problems.push(`в группу ${want[0]} затянуло лишнее: ${extra.join(", ")}`);
  }
}

for (const [id, why] of Object.entries(DATA.ожидается["оставить порознь"])) {
  const g = groups[groupOf(id)];
  if (!g) { problems.push(`запись ${id} потерялась`); continue; }
  const with_ = g.filter((f) => f.id !== id).map((f) => `«${f.name}»`);
  if (with_.length) problems.push(`${id} «${g.find((f) => f.id === id).name}» склеилось с ${with_.join(", ")} — ${why}`);
}

console.log();
console.log(problems.length ? `расхождений: ${problems.length}` : "всё как задумано");
for (const p of problems) console.log(`  ${p}`);
process.exit(problems.length ? 1 : 0);
