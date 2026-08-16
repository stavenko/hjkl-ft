// Миграция 18: мясо глубокой переработки переспрашивается у всех продуктов.
//
// Копчёная рыба считалась колбасой: съеденная скумбрия холодного копчения
// показывалась в дневнике как «86 г колбасы». Хранимые ответы этого не знают:
// признак спрашивается только когда его нет. Сеем продукты со старым (неверным)
// значением и версией базы 17 и смотрим: миграция стёрла признак, фоновый проход
// переспросил, рыба перестала быть колбасой, а настоящая колбаса ею осталась.
import { chromium } from "playwright";

const BASE = process.env.FE || "https://renorma-fit-dev.pages.dev";
const PAY = process.env.PAY || "https://payment-worker-dev.vg-stavenko.workers.dev";
const SECRET = process.env.JWT_SECRET || "dev-secret-change-in-production";
const WAIT_MS = Number(process.env.WAIT_MS || 240000);

let fail = 0;
const check = (n, ok, extra = "") => {
  console.log(`${ok ? "OK  " : "FAIL"} ${n}${extra ? " — " + extra : ""}`);
  if (!ok) fail++;
};

// [название, старое значение, ожидаемое, почему]
const CASES = [
  ["Скумбрия холодного копчения", true, false, "рыба, хоть и копчёная"],
  ["Лосось слабосолёный", true, false, "рыба, хоть и солёная"],
  ["Салями", true, true, "колбаса и осталась колбасой"],
];

const b64url = (buf) => Buffer.from(buf).toString("base64url");
const uid = `pmreask-${Date.now()}`;
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

await page.evaluate(async ({ u, cases }) => {
  const db = await new Promise((res, rej) => {
    const q = indexedDB.open(`hjkl-ft-${u}`);
    q.onsuccess = () => res(q.result); q.onerror = () => rej(q.error);
  });
  const nowIso = new Date().toISOString();
  const put = (store, rows) => new Promise((res, rej) => {
    const tx = db.transaction([store], "readwrite");
    rows.forEach((r) => tx.objectStore(store).put(r));
    tx.oncomplete = () => res(); tx.onerror = () => rej(tx.error);
  });
  // Версия базы 17 — ровно перед миграцией 18.
  await put("app_flags", [
    { key: "db_schema_version", value: "17" },
    { key: "push_onboarding_dismissed", value: "true" },
    { key: "welcome_shown", value: "true" },
  ]);
  await put("foods", cases.map(([name, was], i) => ({
    id: `p${i}`, name, kcal: 200, protein: 18, fat: 14, carbs: 0,
    nutrients: { "Кальций": 0, "Клетчатка": 0 }, package_weight: null, is_recipe: false,
    recipe_id: null, archived: false, is_restaurant: false,
    is_veg_fruit: false, is_heme: false, is_milk_globule: false,
    is_red_meat: false, is_processed_meat: was,
    iron_mg: 1.0, iron_absorption: 0.15,
    fat_profile: { sfa_pct: 35, mufa_pct: 45, pufa_pct: 20, epa_dha_pct: 0 },
    balance_fat_profile: null, created_at: nowIso, updated_at: nowIso,
  })));
  db.close();
}, { u: uid, cases: CASES.map(([n, was]) => [n, was]) });

const read = () => page.evaluate(async (u) => {
  const db = await new Promise((res, rej) => {
    const q = indexedDB.open(`hjkl-ft-${u}`);
    q.onsuccess = () => res(q.result); q.onerror = () => rej(q.error);
  });
  const all = (s) => new Promise((res) => {
    const rq = db.transaction([s], "readonly").objectStore(s).getAll();
    rq.onsuccess = () => res(rq.result); rq.onerror = () => res([]);
  });
  const foods = await all("foods");
  const flags = await all("app_flags");
  db.close();
  return {
    pm: Object.fromEntries(foods.map((f) => [f.name, f.is_processed_meat])),
    version: flags.find((x) => x.key === "db_schema_version")?.value,
  };
}, uid);

await page.goto(BASE, { waitUntil: "domcontentloaded" });
const DEADLINE = Date.now() + WAIT_MS;
let state = await read();
while (Date.now() < DEADLINE) {
  await page.waitForTimeout(10000);
  state = await read();
  const answered = CASES.filter(([n]) => state.pm[n] === true || state.pm[n] === false).length;
  console.log(`переспрошено ${answered}/${CASES.length}, версия базы ${state.version}`);
  if (answered === CASES.length && Number(state.version) >= 18) break;
}

console.log("");
check("версия базы догнала 18", Number(state.version) >= 18, `версия ${state.version}`);
for (const [name, was, want, why] of CASES) {
  check(`${name}: ${was ? "да" : "нет"} → ${want ? "да" : "нет"} (${why})`,
    state.pm[name] === want, `стало ${state.pm[name]}`);
}

await b.close();
console.log(fail ? `\n${fail} провалов` : "\nвсё сошлось");
process.exit(fail ? 1 : 0);
