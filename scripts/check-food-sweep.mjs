// Фоновый проход обязан обходить ВСЮ базу продуктов, а не окно дневника, и
// не должен переспрашивать то, что уже отвечено нулём.
//
// Сеем три продукта: съеденный год назад, вообще не съеденный, и уже полностью
// заполненный, где часть значений — нули. Первые два должны разобраться, третий —
// не породить ни одного запроса.
import { chromium } from "playwright";
import { openSeeded, DEFAULT_URL } from "./harness.mjs";

const BASE = process.env.FE || DEFAULT_URL;
const PAY = process.env.PAY || "https://payment-worker-dev.vg-stavenko.workers.dev";
const SECRET = process.env.JWT_SECRET || "dev-secret-change-in-production";
const WAIT_MS = Number(process.env.WAIT_MS || 180000);

let fail = 0;
const check = (name, ok, extra = "") => { console.log(`${ok ? "OK " : "FAIL"} ${name}${extra ? " — " + extra : ""}`); if (!ok) fail++; };

const b64url = (buf) => Buffer.from(buf).toString("base64url");
async function jwt(sub) {
  const enc = new TextEncoder();
  const now = Math.floor(Date.now() / 1000);
  const data = b64url(JSON.stringify({ alg: "HS256", typ: "JWT" })) + "." +
    b64url(JSON.stringify({ sub, iat: now, exp: now + 7200, caps: [], token_id: "t" }));
  const key = await crypto.subtle.importKey("raw", enc.encode(SECRET),
    { name: "HMAC", hash: "SHA-256" }, false, ["sign"]);
  const sig = await crypto.subtle.sign("HMAC", key, enc.encode(data));
  return `${data}.${b64url(new Uint8Array(sig))}`;
}

const uid = `sweep-${Date.now()}`;
const token = await jwt(uid);
const co = await (await fetch(`${PAY}/test/guest-checkout`, {
  method: "POST", headers: { "Content-Type": "application/json" },
  body: JSON.stringify({ planId: "test" }),
})).json();
await fetch(`${PAY}/claim`, {
  method: "POST",
  headers: { "Content-Type": "application/json", Authorization: `Bearer ${token}` },
  body: JSON.stringify({ claimId: co.claimId, secret: co.secret }),
});

const b = await chromium.launch({ headless: true });
const ctx = await b.newContext({ viewport: { width: 430, height: 932 }, serviceWorkers: "block" });
const page = await ctx.newPage();
// Считаем ЗАПРОСЫ к модели ПО ПРОДУКТАМ: общее число плавает от ретраев и ничего
// не доказывает, а «не переспрашивать» — это ровно «ни одного запроса про Воду».
const calls = [];
page.on("request", (r) => {
  if (!/\/chat\/completions/.test(r.url())) return;
  calls.push(r.postData() || "");
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
const NEED = ["app_flags", "profile", "goals", "foods", "diary"];
let ready = false;
for (let i = 0; i < 60 && !ready; i++) {
  ready = await page.evaluate(async ({ uid, NEED }) => {
    const dbs = await indexedDB.databases();
    if (!dbs.some((d) => d.name === `hjkl-ft-${uid}`)) return false;
    return await new Promise((res) => {
      const q = indexedDB.open(`hjkl-ft-${uid}`);
      q.onsuccess = () => { const n = Array.from(q.result.objectStoreNames); q.result.close(); res(NEED.every((x) => n.includes(x))); };
      q.onerror = () => res(false);
    });
  }, { uid, NEED }).catch(() => false);
  if (!ready) await page.waitForTimeout(500);
}
if (!ready) throw new Error("база так и не появилась");

await page.evaluate(async (uid) => {
  const db = await new Promise((res, rej) => {
    const q = indexedDB.open(`hjkl-ft-${uid}`);
    q.onsuccess = () => res(q.result); q.onerror = () => rej(q.error);
  });
  const nowIso = new Date().toISOString();
  const ymd = (o) => { const d = new Date(); d.setDate(d.getDate() - o);
    return `${d.getFullYear()}-${String(d.getMonth() + 1).padStart(2, "0")}-${String(d.getDate()).padStart(2, "0")}`; };
  const app_flags = [
    { key: "push_onboarding_dismissed", value: "true" }, { key: "welcome_shown", value: "true" },
    { key: "iron_week_unlocked", value: "true" }, { key: "iron_week_opened_at", value: ymd(3) },
    // Одноразовая инвалидация кальция и железа здесь ДОЛЖНА считаться пройденной:
    // иначе она сотрёт значения у «Воды», и проверка «заполненное не
    // переспрашивается» будет мерить не проход, а миграцию. Сама миграция
    // проверяется отдельно — check-calcium-iron-reset.mjs.
    { key: "calcium_iron_invalidated_v1", value: "true" },
  ];
  const profile = [{ key: "profile", sex: "male", height_cm: 180, birth_year: 1980,
    goal: "lose", steps_planka: 9000, created_at: nowIso, updated_at: nowIso }];
  const goals = [{ id: "g", nutrient: "Calories", key: "calories", direction: "AtMost",
    amount: 2500, unit: "Kcal", period: "Day", created_at: nowIso, updated_at: nowIso }];
  const bare = { kcal: 100, protein: 5, fat: 3, carbs: 10, package_weight: null, is_recipe: false,
    recipe_id: null, archived: false, is_restaurant: false, created_at: nowIso, updated_at: nowIso };
  const foods = [
    // 1. Съеден ГОД назад — в прежнее окно 14 дней не попадал бы никогда.
    { ...bare, id: "old", name: "Гречка", nutrients: {}, is_snack: null, is_liquid_cal: null,
      is_veg_fruit: null, is_egg: null, is_red_meat: null, iron_mg: null, iron_absorption: null },
    // 2. Вообще не съеден — в дневнике его нет.
    { ...bare, id: "never", name: "Творог", nutrients: {}, is_snack: null, is_liquid_cal: null,
      is_veg_fruit: null, is_egg: null, is_red_meat: null, iron_mg: null, iron_absorption: null },
    // 3. Заполнен целиком, часть значений — НУЛИ. Переспрашивать нечего.
    { ...bare, id: "done", name: "Вода", nutrients: { "Кальций": 0, "Клетчатка": 0, "Омега-3": 0 },
      is_snack: false, is_liquid_cal: false, is_veg_fruit: false, is_egg: false, is_red_meat: false,
      iron_mg: 0, iron_absorption: 0.05 },
  ];
  const diary = [{ id: "d-old", food_id: "old", date: ymd(365), time: null, grams: 100,
    waste_grams: 0, meal_label: "lunch", deleted: false, created_at: nowIso, updated_at: nowIso }];
  for (const [store, rows] of Object.entries({ app_flags, profile, goals, foods, diary })) {
    await new Promise((res, rej) => {
      const tx = db.transaction([store], "readwrite");
      for (const r of rows) tx.objectStore(store).put(r);
      tx.oncomplete = () => res(); tx.onerror = () => rej(tx.error);
    });
  }
  db.close();
}, uid);

await page.goto(BASE, { waitUntil: "domcontentloaded" });
await page.waitForSelector("#splash", { state: "detached", timeout: 20000 }).catch(() => {});

const readFoods = () => page.evaluate(async (uid) => {
  const db = await new Promise((res, rej) => {
    const q = indexedDB.open(`hjkl-ft-${uid}`);
    q.onsuccess = () => res(q.result); q.onerror = () => rej(q.error);
  });
  const all = await new Promise((res) => {
    const rq = db.transaction(["foods"], "readonly").objectStore("foods").getAll();
    rq.onsuccess = () => res(rq.result); rq.onerror = () => res([]);
  });
  db.close();
  // Новые записи кладутся обычным объектом, но в базах, заведённых до этого, поле
  // ещё лежит как JS `Map` — у него `Object.keys()` всегда даёт 0. Считаем обе формы.
  const size = (n) => (n instanceof Map ? n.size : Object.keys(n || {}).length);
  const dump = (n) => JSON.stringify(n instanceof Map ? Object.fromEntries(n) : n || {});
  return all.map((f) => ({ id: f.id, name: f.name, mg: f.iron_mg, abs: f.iron_absorption,
    tagged: f.is_snack !== null && f.is_snack !== undefined,
    values: dump(f.nutrients), nutrients: size(f.nutrients) }));
}, uid);

const t0 = Date.now();
let rows = [];
while (Date.now() - t0 < WAIT_MS) {
  rows = await readFoods();
  const pending = rows.filter((r) => r.id !== "done" && (r.mg == null || !r.tagged || r.nutrients < 3));
  if (!pending.length) break;
  await page.waitForTimeout(3000);
}

for (const r of rows) console.log(`  ${r.name.padEnd(10)} железо ${String(r.mg).padEnd(6)} усвоение ${String(r.abs).padEnd(6)} теги ${r.tagged} нутриенты ${r.values}`);
const byId = Object.fromEntries(rows.map((r) => [r.id, r]));
check("продукт, съеденный ГОД назад, разобран", byId.old?.mg != null && byId.old?.tagged && byId.old?.nutrients === 3, JSON.stringify(byId.old));
check("продукт, которого НЕТ в дневнике, разобран", byId.never?.mg != null && byId.never?.tagged && byId.never?.nutrients === 3, JSON.stringify(byId.never));
check("уже заполненный (с нулями) не изменился", byId.done?.mg === 0 && byId.done?.abs === 0.05, JSON.stringify(byId.done));
// Заполненный продукт не должен породить НИ ОДНОГО запроса — ни про нутриенты,
// ни про железо, ни про теги.
const aboutDone = calls.filter((b) => b.includes("Вода"));
check("про заполненный продукт не спрошено ничего", aboutDone.length === 0, `${aboutDone.length} запр.`);
check("незаполненные продукты запросы породили", calls.length > 0, `${calls.length} запр. всего`);

console.log(fail === 0 ? "\n=== ALL OK ===" : `\n=== FAILURES: ${fail} ===`);
await b.close();
process.exit(fail === 0 ? 0 : 1);
