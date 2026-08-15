// Замер признака «источник гемового железа» через ЖИВОЙ путь.
//
// Набор нарочно составлен вокруг ПРОТИВОРЕЧИЯ, на котором признак и сломался:
// печень стоит в TRUE, птица — в FALSE, и куриная печень попадает под оба правила
// сразу. Плюс прежние больные места: моллюски против ракообразных, колбаса против
// мяса, растительное железо.
import { chromium } from "playwright";

const BASE = process.env.FE || "https://renorma-fit-dev.pages.dev";
const PAY = process.env.PAY || "https://payment-worker-dev.vg-stavenko.workers.dev";
const SECRET = process.env.JWT_SECRET || "dev-secret-change-in-production";

const CASES = [
  // Рыба, из-за которой всё затевалось: голец у человека оказался помечен гемом.
  ["Голец", false, "рыба, не орган и не красное мясо"],
  ["Голец с/м", false, "та же рыба, помета о заморозке ничего не меняет"],
  ["Голец филе", false, "филе рыбы"],
  ["Арктический голец", false, "он же полным именем"],
  // Соседи по строке жиров — проверяем, не тянет ли за собой вся строка.
  ["Кижуч без головы", false, "рыба"],
  ["Форель радужная", false, "рыба"],
  ["Кета", false, "рыба"],
  // Контроль: что ДОЛЖНО быть гемом.
  ["Печень трески", true, "печень — любого животного, птицы или рыбы"],
  ["Говядина", true, "красное мясо"],
  ["Мидии варёно-мороженые", true, "моллюск из списка"],
];

const b64url = (buf) => Buffer.from(buf).toString("base64url");
const uid = `heme-${Date.now()}`;
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

// Всё, кроме гема, уже «выяснено» — лишних запросов не надо.
await page.evaluate(async ({ u, names }) => {
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
    names.forEach((name, i) => tx.objectStore("foods").put({
      id: `h${i}`, name, kcal: 150, protein: 20, fat: 8, carbs: 0,
      nutrients: { "Кальций": 0, "Клетчатка": 0 }, package_weight: null, is_recipe: false,
      recipe_id: null, archived: false, is_restaurant: false,
      is_veg_fruit: false, is_heme: null, is_milk_globule: false,
      iron_mg: 2.0, iron_absorption: 0.15,
      fat_profile: { sfa_pct: 30, mufa_pct: 40, pufa_pct: 20, epa_dha_pct: 0 },
      balance_fat_profile: null,
      created_at: nowIso, updated_at: nowIso,
    }));
    tx.oncomplete = () => res(); tx.onerror = () => rej(tx.error);
  });
  db.close();
}, { u: uid, names: CASES.map(([n]) => n) });

await page.goto(BASE, { waitUntil: "domcontentloaded" });
const DEADLINE = Date.now() + 14 * 60 * 1000;
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
    return Object.fromEntries(all.map((f) => [f.name, f.is_heme]));
  }, uid);
  const done = CASES.filter(([n]) => got[n] !== null && got[n] !== undefined).length;
  console.log(`отвечено ${done}/${CASES.length}`);
  if (done === CASES.length) break;
}

let bad = 0;
console.log("\nпродукт                          наш   ждём   почему");
for (const [name, want, why] of CASES) {
  const v = got[name];
  const ok = v === want;
  if (!ok) bad++;
  const s = (x) => (x === true ? "да " : x === false ? "нет" : " — ");
  console.log(`${ok ? "OK  " : "MISS"} ${name.padEnd(30)} ${s(v)}   ${s(want)}   ${why}`);
}
console.log(`\nпопаданий: ${CASES.length - bad}/${CASES.length}`);

await ctx.close();
await b.close();
process.exit(bad ? 1 : 0);
