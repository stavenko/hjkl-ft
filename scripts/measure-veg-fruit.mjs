// Замер признака «овощ или фрукт» через ЖИВОЙ путь.
//
// Повод: «Мороженая вишня» не признавалась ни овощем, ни фруктом. Набор собран
// вокруг двух подозрений — ЯГОДЫ (входят ли они в «фрукты» вообще) и СЛОВА О
// ЗАМОРОЗКЕ, которые не должны менять суть продукта. Плюс граница со сладким:
// вишня в сиропе и варенье — уже не фрукт, и признак обязан их различать.
//
// ONLY=подстрока[,подстрока] — прогнать прицельно по одной позиции.
import { chromium } from "playwright";

const BASE = process.env.FE || "https://renorma-fit-dev.pages.dev";
const PAY = process.env.PAY || "https://payment-worker-dev.vg-stavenko.workers.dev";
const SECRET = process.env.JWT_SECRET || "dev-secret-change-in-production";

const CASES = [
  // Тот самый случай и его ближайшие родственники.
  ["Мороженая вишня", true, "ягода, заморозка сути не меняет"],
  ["Вишня", true, "ягода"],
  ["Вишня замороженная", true, "то же другими словами"],
  ["Черника с/м", true, "ягода, помета о заморозке"],
  ["Малина свежая", true, "ягода"],
  ["Клубника", true, "ягода"],
  // Овощи, в том числе мороженые.
  ["Брокколи замороженная", true, "овощ, заморозка сути не меняет"],
  ["Огурец", true, "овощ"],
  ["Картофель отварной", true, "овощ, приготовление сути не меняет"],
  ["Салат из помидоров", true, "очевидное овощное блюдо"],
  // Фрукты.
  ["Яблоко", true, "фрукт"],
  ["Банан", true, "фрукт"],
  // Граница со сладким: сахар делает продукт сладостью, а не фруктом.
  ["Варенье вишнёвое", false, "сладость, а не фрукт"],
  ["Вишня в сиропе", false, "сладость"],
  ["Сок вишнёвый", false, "напиток"],
  // Не овощи и не фрукты.
  ["Гречка варёная", false, "крупа"],
  ["Хлеб бородинский", false, "хлеб"],
  ["Куриная грудка", false, "мясо"],
  ["Творог 5 %", false, "молочное"],
  ["Орехи грецкие", false, "орехи — не овощ и не фрукт"],
];

if (process.env.ONLY) {
  const want = process.env.ONLY.split(",").map((s) => s.trim().toLowerCase());
  const kept = CASES.filter(([n]) => want.some((w) => n.toLowerCase().includes(w)));
  if (!kept.length) throw new Error(`ONLY=${process.env.ONLY} не совпал ни с одним продуктом`);
  CASES.length = 0;
  CASES.push(...kept);
}

const b64url = (buf) => Buffer.from(buf).toString("base64url");
const uid = `vegfruit-${Date.now()}`;
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
// Приложение печатает вердикт и причину по каждому продукту — без этого замер
// покажет «мимо», но не ЧЕМ модель это объяснила.
const said = new Map();
// Признак, оставшийся пустым, значит не «модель ошиблась», а «запрос не дошёл до
// ответа»: копим предупреждения и ошибки, иначе такую поломку видно только как
// прочерк в таблице.
const troubles = [];
page.on("console", (m) => {
  const t = m.text();
  const g = t.match(/^фрукты\/овощи «(.+?)»: (.+)$/);
  if (g) return said.set(g[1], g[2]);
  if (m.type() === "warning" || m.type() === "error" || /retry|error|ошибк/i.test(t)) {
    if (troubles.length < 20) troubles.push(`[${m.type()}] ${t.slice(0, 300)}`);
  }
});
page.on("pageerror", (e) => troubles.push(`[pageerror] ${String(e).slice(0, 300)}`));
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

// Всё, кроме овоща/фрукта, уже «выяснено» — лишних запросов не надо.
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
      id: `v${i}`, name, kcal: 60, protein: 1, fat: 0, carbs: 12,
      nutrients: { "Кальций": 0, "Клетчатка": 2 }, package_weight: null, is_recipe: false,
      recipe_id: null, archived: false, is_restaurant: false,
      is_veg_fruit: null, is_heme: false, is_milk_globule: false,
      is_red_meat: false, is_processed_meat: false,
      iron_mg: 0.3, iron_absorption: 0.05,
      fat_profile: { sfa_pct: 20, mufa_pct: 30, pufa_pct: 50, epa_dha_pct: 0 },
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
    return Object.fromEntries(all.map((f) => [f.name, f.is_veg_fruit]));
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
  if (!ok) console.log(`     модель: ${said.get(name) ?? "(в консоль ничего не пришло)"}`);
}
console.log(`\nпопаданий: ${CASES.length - bad}/${CASES.length}`);
if (troubles.length) {
  console.log("\nжалобы приложения:");
  for (const t of troubles) console.log(`  ${t}`);
}

await b.close();
process.exit(bad ? 1 : 0);
