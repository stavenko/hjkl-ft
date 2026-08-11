// Замер профиля жира ЧЕРЕЗ ЖИВОЙ ПУТЬ: продукты заводятся без профиля, фоновый
// проход спрашивает модель, читаем записанное. Копии промпта здесь нет — он
// рендерится из таблицы строк в ai.rs, и меряется то, что работает у людей.
//
// Ожидания — СПРАВОЧНЫЕ доли от собственного жира продукта, с допуском: важно не
// попасть в точку, а не промахнуться категорией и не срезать треть величины.
import { chromium } from "playwright";

const BASE = process.env.FE || "https://renorma-fit-dev.pages.dev";
const PAY = process.env.PAY || "https://payment-worker-dev.vg-stavenko.workers.dev";
const SECRET = process.env.JWT_SECRET || "dev-secret-change-in-production";
// Допуск в процентных пунктах: доли жира и в справочниках расходятся между
// источниками на несколько пунктов, требовать точности бессмысленно.
const TOL = Number(process.env.TOL || 8);

// [название, жир г/100 г, ожидаемые НЖК/МНЖК/ПНЖК/ЭПК+ДГК в % от жира, источник]
const CASES = [
  ["Кижуч без головы", 5.9, [21, 36, 34, 18], "дикий кижуч, USDA"],
  ["Сельдь атлантическая", 9.0, [23, 41, 24, 17], "USDA"],
  ["Лосось атлантический", 13.4, [23, 28, 29, 15], "фермерский, USDA"],
  ["Скумбрия", 13.9, [24, 39, 24, 19], "USDA"],
  ["Сливочное масло", 82, [63, 26, 4, 0], "контроль: молочный жир"],
  ["Оливковое масло", 100, [15, 73, 11, 0], "контроль: олива"],
  ["Подсолнечное масло", 100, [11, 22, 64, 0], "контроль: линолевое"],
];

const b64url = (buf) => Buffer.from(buf).toString("base64url");
const uid = `fatmeas-${Date.now()}`;
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
    items.forEach(([name, fat], i) => tx.objectStore("foods").put({
      id: `f${i}`, name, kcal: 200, protein: 15, fat, carbs: 0,
      // Нутриенты уже «выяснены» — мерим только профиль жира, лишних запросов не надо.
      nutrients: { "Кальций": 0, "Клетчатка": 0 }, package_weight: null, is_recipe: false,
      recipe_id: null, archived: false, is_restaurant: false,
      is_veg_fruit: false, is_heme: false, iron_mg: 0.5, iron_absorption: 0.15,
      fat_profile: null, created_at: nowIso, updated_at: nowIso,
    }));
    tx.oncomplete = () => res(); tx.onerror = () => rej(tx.error);
  });
  db.close();
}, { u: uid, items: CASES.map(([n, fat]) => [n, fat]) });

await page.goto(BASE, { waitUntil: "domcontentloaded" });
const DEADLINE = Date.now() + 10 * 60 * 1000;
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
    return Object.fromEntries(all.map((f) => [f.name, f.fat_profile]));
  }, uid);
  const done = CASES.filter(([n]) => got[n]).length;
  console.log(`заполнено ${done}/${CASES.length}`);
  if (done === CASES.length) break;
}

let bad = 0;
console.log(`\nдопуск ±${TOL} п.п.\nпродукт                     наш (НЖК/МНЖК/ПНЖК/ЭПК+ДГК)   справочно`);
for (const [name, , want, src] of CASES) {
  const p = got[name];
  const mine = p ? [p.sfa_pct, p.mufa_pct, p.pufa_pct, p.epa_dha_pct] : null;
  const ok = mine && mine.every((v, i) => Math.abs(v - want[i]) <= TOL);
  if (!ok) bad++;
  const fmt = (a) => a ? a.map((v) => String(Math.round(v)).padStart(2)).join("/") : "   —";
  console.log(`${ok ? "OK  " : "MISS"} ${name.padEnd(24)} ${fmt(mine)}        ${fmt(want)}  (${src})`);
}
console.log(`\nпопаданий: ${CASES.length - bad}/${CASES.length}`);

await ctx.close();
await b.close();
process.exit(bad ? 1 : 0);
