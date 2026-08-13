// Куда модель кладёт ДОМАШНИЕ БЛЮДА, у которых источник жира не назван в имени.
//
// Боевой случай из логов: «Баклажан фаршированный» → выдуманная строка
// «vegetable» → ответ отбракован, профиль жира не появляется НИКОГДА, и продукт
// уходит в очередь на каждой активации.
//
// Меряется ЖИВОЙ ПУТЬ: продукты заводятся без профиля, фоновый проход спрашивает
// модель, а мы слушаем консоль страницы — там и выбранная строка успешного
// ответа (ai.rs: «жиры «X»: row · тип — …»), и КАЖДЫЙ провал валидатора
// (generate_validated: «generate retry #N: unknown fat row «Y»…»), которого в
// телеметрии не видно: туда уходит только последняя ошибка захода.
//
// Одно и то же имя заводится НЕСКОЛЬКО РАЗ (разные id): вопрос «устойчив ли
// выдуманный ключ» решается только повторами.
import { chromium } from "playwright";

const BASE = process.env.FE || "https://renorma-fit-dev.pages.dev";
const PAY = process.env.PAY || "https://payment-worker-dev.vg-stavenko.workers.dev";
const SECRET = process.env.JWT_SECRET || "dev-secret-change-in-production";

// [название, сколько раз завести]
const CASES = [
  ["Баклажан фаршированный", 3],   // боевой случай
  ["Перец фаршированный", 1],
  ["Голубцы", 1],
  ["Котлеты домашние", 1],
  ["Запеканка картофельная с фаршем", 1],
  ["Плов с бараниной", 1],
  // Контроль: базовые продукты обязаны остаться в таблице строк и сохранить
  // свои значения — развилка не должна утащить их в блюдовую ветку.
  ["Сливочное масло", 1],
  ["Оливковое масло", 1],
  ["Кижуч без головы", 1],
  ["Куриное бедро", 1],
];

const b64url = (buf) => Buffer.from(buf).toString("base64url");
const uid = `fatdish-${Date.now()}`;
const now = Math.floor(Date.now() / 1000);
const data = b64url(JSON.stringify({ alg: "HS256", typ: "JWT" })) + "." +
  b64url(JSON.stringify({ sub: uid, iat: now, exp: now + 7200, caps: [], token_id: "t" }));
const key = await crypto.subtle.importKey("raw", new TextEncoder().encode(SECRET),
  { name: "HMAC", hash: "SHA-256" }, false, ["sign"]);
const token = `${data}.${b64url(new Uint8Array(await crypto.subtle.sign("HMAC", key, new TextEncoder().encode(data))))}`;
const co = await (await fetch(`${PAY}/test/guest-checkout`, { method: "POST",
  headers: { "Content-Type": "application/json" }, body: JSON.stringify({ planId: "test" }) })).json();
await fetch(`${PAY}/claim`, { method: "POST",
  headers: { "Content-Type": "application/json", Authorization: `Bearer ${token}` },
  body: JSON.stringify({ claimId: co.claimId, secret: co.secret }) });

const b = await chromium.launch({ headless: true });
const ctx = await b.newContext({ viewport: { width: 430, height: 932 }, serviceWorkers: "block" });
const page = await ctx.newPage();

const picked = [];   // базовая ветка: {name, row, type, nums, reason}
const dishes = [];   // блюдовая ветка: {name, raw, nums, reason}
const split = [];    // развилка: {name, composite, reason}
const failed = [];   // провалы валидатора: {name, row}
page.on("console", (m) => {
  const t = m.text();
  let g = t.match(/составное «(.+?)»: (true|false) — (.*)/);
  if (g) split.push({ name: g[1], composite: g[2] === "true", reason: g[3] });
  g = t.match(/жиры блюда «(.+?)»: модель (\d+)\/(\d+)\/(\d+) → НЖК (\d+) МНЖК (\d+) ПНЖК (\d+) EPA\+DHA 0 \((.*)\)/);
  if (g) dishes.push({ name: g[1], raw: [g[2], g[3], g[4]].map(Number),
    nums: [g[5], g[6], g[7]].map(Number), reason: g[8] });
  g = t.match(/жиры «(.+?)»: (\S+) · (.+?) — НЖК (\d+) МНЖК (\d+) ПНЖК (\d+) EPA\+DHA (\d+) \((.*)\)/);
  if (g) picked.push({ name: g[1], row: g[2], type: g[3],
    nums: [g[4], g[5], g[6], g[7]].map(Number), reason: g[8] });
  g = t.match(/unknown fat row «(.+?)» for «(.+?)»/);
  if (g) failed.push({ name: g[2], row: g[1] });
});

await page.goto(BASE, { waitUntil: "domcontentloaded" });
await page.evaluate(({ uid, token }) => {
  localStorage.clear();
  localStorage.setItem("user_id", uid);
  localStorage.setItem("auth_token", token);
  localStorage.setItem("token_id", "t");
  localStorage.setItem("auth_ctx", "browser");
  localStorage.setItem("pwa_dismissed", "true");
}, { uid, token });
await page.goto(BASE, { waitUntil: "domcontentloaded" });
for (let i = 0; i < 60; i++) {
  const ok = await page.evaluate(async (u) => {
    const dbs = await indexedDB.databases();
    if (!dbs.some((d) => d.name === `hjkl-ft-${u}`)) return false;
    return await new Promise((res) => {
      const q = indexedDB.open(`hjkl-ft-${u}`);
      q.onsuccess = () => { const n = Array.from(q.result.objectStoreNames); q.result.close();
        res(["foods", "app_flags"].every((s) => n.includes(s))); };
      q.onerror = () => res(false);
    });
  }, uid).catch(() => false);
  if (ok) break;
  await page.waitForTimeout(500);
}

// Разворачиваем повторы в отдельные записи: имя одно, id разные.
const items = [];
for (const [name, times] of CASES) {
  for (let k = 0; k < times; k++) items.push({ id: `d${items.length}`, name });
}

await page.evaluate(async ({ u, items }) => {
  const db = await new Promise((res, rej) => {
    const q = indexedDB.open(`hjkl-ft-${u}`);
    q.onsuccess = () => res(q.result); q.onerror = () => rej(q.error);
  });
  const nowIso = new Date().toISOString();
  await new Promise((res, rej) => {
    const tx = db.transaction(["app_flags"], "readwrite");
    [{ key: "db_schema_version", value: "999" }, { key: "push_onboarding_dismissed", value: "true" },
     { key: "welcome_shown", value: "true" }].forEach((f) => tx.objectStore("app_flags").put(f));
    tx.oncomplete = () => res(); tx.onerror = () => rej(tx.error);
  });
  await new Promise((res, rej) => {
    const tx = db.transaction(["foods"], "readwrite");
    // Всё, кроме профиля жира, уже «выяснено» — лишних запросов к модели не надо.
    items.forEach(({ id, name }) => tx.objectStore("foods").put({
      id, name, kcal: 200, protein: 12, fat: 9, carbs: 10,
      nutrients: { "Кальций": 0, "Клетчатка": 0 }, package_weight: null, is_recipe: false,
      recipe_id: null, archived: false, is_restaurant: false,
      is_veg_fruit: false, is_heme: false, is_milk_globule: false,
      iron_mg: 0.5, iron_absorption: 0.15,
      fat_profile: null, balance_fat_profile: null,
      created_at: nowIso, updated_at: nowIso,
    }));
    tx.oncomplete = () => res(); tx.onerror = () => rej(tx.error);
  });
  db.close();
}, { u: uid, items });

await page.goto(BASE, { waitUntil: "domcontentloaded" });
const DEADLINE = Date.now() + 20 * 60 * 1000;
let got = {};
while (Date.now() < DEADLINE) {
  await page.waitForTimeout(15000);
  got = await page.evaluate(async (u) => {
    const db = await new Promise((res, rej) => {
      const q = indexedDB.open(`hjkl-ft-${u}`);
      q.onsuccess = () => res(q.result); q.onerror = () => rej(q.error);
    });
    const all = await new Promise((res) => {
      const rq = db.transaction(["foods"]).objectStore("foods").getAll();
      rq.onsuccess = () => res(rq.result); rq.onerror = () => res([]);
    });
    db.close();
    return Object.fromEntries(all.map((f) => [f.id, f.fat_profile]));
  }, uid);
  const done = items.filter(({ id }) => got[id]).length;
  console.log(`заполнено ${done}/${items.length}, провалов пока ${failed.length}`);
  if (done === items.length) break;
}

console.log("\n── развилка ─────────────────────────────────────────────────");
for (const s of split) {
  console.log(`${s.composite ? "БЛЮДО " : "базовый"} ${s.name.padEnd(32)} ${s.reason}`);
}
console.log("\n── базовая ветка: строка таблицы ────────────────────────────");
for (const p of picked) {
  console.log(`${p.name.padEnd(32)} ${p.row.padEnd(20)} ${p.nums.join("/").padEnd(14)} ${p.type} — ${p.reason}`);
}
console.log("\n── блюдовая ветка: ответ модели → после пессимизации ────────");
for (const d of dishes) {
  const r = (d.nums[1] + d.nums[2]) / d.nums[0];
  console.log(`${d.name.padEnd(32)} ${d.raw.join("/").padEnd(12)} → ${d.nums.join("/").padEnd(12)} ` +
    `отношение ${r.toFixed(2)}  ${d.reason}`);
}
console.log("\n── выдуманные ключи (каждая попытка) ────────────────────────");
if (!failed.length) console.log("нет");
const byRow = {};
for (const f of failed) {
  const k = `${f.name} → ${f.row}`;
  byRow[k] = (byRow[k] || 0) + 1;
}
for (const [k, n] of Object.entries(byRow).sort((a, b) => b[1] - a[1])) {
  console.log(`${String(n).padStart(3)} × ${k}`);
}
console.log("\n── чем кончилось ───────────────────────────────────────────");
for (const { id, name } of items) {
  console.log(`${got[id] ? "есть  " : "НЕТ   "} ${name}`);
}

await ctx.close();
await b.close();
