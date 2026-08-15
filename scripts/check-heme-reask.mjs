// Миграция 16: гем переспрашивается у всех продуктов.
//
// Определение изменилось по существу: колбаса стала источником гема, куриное
// бедро перестало им быть. Хранимые ответы этого не знают — признак спрашивается
// только когда его нет. Сеем продукты с ЗАВЕДОМО старыми значениями и версией
// базы 15 и смотрим: миграция стёрла признак, фоновый проход переспросил, и новые
// ответы — верные. Плюс идемпотентность: повторный запуск ничего не стирает.
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

// Старое значение — то, что дал бы прежний промпт; ждём ПРОТИВОПОЛОЖНОЕ.
const CASES = [
  ["Колбаса докторская", false, true, "мясо млекопитающих — главная часть"],
  ["Бедро куриное печёное", true, false, "мясо птицы, а не красное мясо"],
  ["Печень куриная", true, true, "субпродукт: значение то же, но спрошено заново"],
];

const b64url = (buf) => Buffer.from(buf).toString("base64url");
const uid = `reask-${Date.now()}`;
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
  // Версия базы 15 — ровно перед миграцией 16.
  await put("app_flags", [
    { key: "db_schema_version", value: "15" },
    { key: "push_onboarding_dismissed", value: "true" },
    { key: "welcome_shown", value: "true" },
  ]);
  await put("foods", cases.map(([name, was], i) => ({
    id: `h${i}`, name, kcal: 150, protein: 20, fat: 8, carbs: 0,
    nutrients: { "Кальций": 0, "Клетчатка": 0 }, package_weight: null, is_recipe: false,
    recipe_id: null, archived: false, is_restaurant: false,
    is_veg_fruit: false, is_heme: was, is_milk_globule: false,
    iron_mg: 2.0, iron_absorption: 0.15,
    fat_profile: { sfa_pct: 30, mufa_pct: 40, pufa_pct: 20, epa_dha_pct: 0 },
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
    heme: Object.fromEntries(foods.map((f) => [f.name, f.is_heme])),
    version: flags.find((x) => x.key === "db_schema_version")?.value,
  };
}, uid);

// Перезапуск приложения — миграции идут на старте.
await page.goto(BASE, { waitUntil: "domcontentloaded" });
const DEADLINE = Date.now() + WAIT_MS;
let state = await read();
while (Date.now() < DEADLINE) {
  await page.waitForTimeout(10000);
  state = await read();
  const answered = CASES.filter(([n]) => state.heme[n] === true || state.heme[n] === false).length;
  console.log(`переспрошено ${answered}/${CASES.length}, версия базы ${state.version}`);
  if (answered === CASES.length && Number(state.version) >= 16) break;
}

console.log("");
check("версия базы догнала 16", Number(state.version) >= 16, `версия ${state.version}`);
for (const [name, was, want, why] of CASES) {
  check(`${name}: ${was ? "да" : "нет"} → ${want ? "да" : "нет"} (${why})`,
    state.heme[name] === want, `стало ${state.heme[name]}`);
}

// Идемпотентность: второй запуск не сбрасывает уже полученные ответы.
const before = { ...state.heme };
await page.goto(BASE, { waitUntil: "domcontentloaded" });
await page.waitForTimeout(15000);
const again = await read();
check("повторный запуск ничего не стирает",
  CASES.every(([n]) => again.heme[n] === before[n]),
  JSON.stringify(again.heme));

await b.close();
console.log(fail ? `\n${fail} провалов` : "\nвсё сошлось");
process.exit(fail ? 1 : 0);
