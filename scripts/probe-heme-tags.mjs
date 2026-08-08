// Замер: что модель отвечает про гем на РЕАЛЬНЫХ названиях продуктов.
//
// Повод — «Мидии варёно-мороженые, чилийские» с ответом «нет», хотя моллюски
// перечислены в промпте поимённо. Сеем названия-варианты, даём фоновому проходу
// их разобрать и читаем `is_heme` из базы. Никакой синтетики: идёт ровно тот
// запрос, который приложение шлёт само.
import { chromium } from "playwright";

const BASE = process.env.FE || "https://renorma-fit-dev.pages.dev";
const PAY = process.env.PAY || "https://payment-worker-dev.vg-stavenko.workers.dev";
const SECRET = process.env.JWT_SECRET || "dev-secret-change-in-production";
const WAIT_MS = Number(process.env.WAIT_MS || 240000);

// Что ждём: true — источник гема, false — не источник.
const CASES = (process.env.CASES ? JSON.parse(process.env.CASES) : [
  ["Мидии варёно-мороженые, чилийские", true],
  ["Мидии варёно-мороженые", true],
  ["Мидии чилийские", true],
  ["Мидии", true],
  ["Устрицы", true],
  ["Кальмар", true],
  ["Печень куриная", true],
  ["Креветки", false],
]);

const b64url = (buf) => Buffer.from(buf).toString("base64url");
const uid = `heme-probe-${Date.now()}`;
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
// Обоснование модель пишет в лог — по нему видно, ЧЕМ она сочла продукт.
const reasons = new Map();
page.on("console", (m) => {
  const t = m.text();
  const g = /^гем «(.+)»: (true|false) — (.*)$/.exec(t);
  if (g) reasons.set(g[1], `${g[2]} · ${g[3]}`);
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

// Ждём, пока приложение заведёт свою базу.
for (let i = 0; i < 90; i++) {
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

await page.evaluate(async ({ uid, names }) => {
  const db = await new Promise((res, rej) => {
    const q = indexedDB.open(`hjkl-ft-${uid}`);
    q.onsuccess = () => res(q.result); q.onerror = () => rej(q.error);
  });
  const nowIso = new Date().toISOString();
  await new Promise((res, rej) => {
    const tx = db.transaction(["foods"], "readwrite");
    names.forEach((name, i) => tx.objectStore("foods").put({
      id: "p" + i, name, kcal: 100, protein: 12, fat: 2, carbs: 4, nutrients: {},
      package_weight: null, is_recipe: false, recipe_id: null, archived: false,
      is_restaurant: false, is_snack: null, is_liquid_cal: null, is_veg_fruit: null,
      is_egg: null, is_red_meat: null, is_heme: null, iron_mg: null,
      iron_absorption: null, created_at: nowIso, updated_at: nowIso,
    }));
    tx.oncomplete = () => res(); tx.onerror = () => rej(tx.error);
  });
  db.close();
}, { uid, names: CASES.map(([n]) => n) });

// Перезапуск: фоновый проход разбирает базу при старте.
await page.goto(BASE, { waitUntil: "domcontentloaded" });

const read = () => page.evaluate(async (uid) => {
  const db = await new Promise((res, rej) => {
    const q = indexedDB.open(`hjkl-ft-${uid}`);
    q.onsuccess = () => res(q.result); q.onerror = () => rej(q.error);
  });
  const all = await new Promise((res) => {
    const rq = db.transaction(["foods"]).objectStore("foods").getAll();
    rq.onsuccess = () => res(rq.result); rq.onerror = () => res([]);
  });
  db.close();
  return all.map((f) => [f.name, f.is_heme, f.is_liquid_cal, f.is_veg_fruit]);
}, uid);

const t0 = Date.now();
let rows = [];
while (Date.now() - t0 < WAIT_MS) {
  rows = await read();
  if (rows.length && rows.every(([, h]) => h !== null && h !== undefined)) break;
  await page.waitForTimeout(4000);
}

let wrong = 0;
for (const [name, want] of CASES) {
  const got = rows.find((r) => r[0] === name);
  const heme = got ? got[1] : undefined;
  const ok = heme === want;
  if (!ok) wrong++;
  console.log(`${ok ? "OK " : "МИМО"} ${name} → гем ${heme === undefined ? "не отвечено" : heme}` +
    (ok ? "" : ` (ждали ${want})`) +
    `  [жидк.кал ${got?.[2]}, фр/овощи ${got?.[3]}]` +
    (reasons.has(name) ? `\n      обоснование: ${reasons.get(name)}` : ""));
}
console.log(`\nмимо: ${wrong} из ${CASES.length}`);
await b.close();
