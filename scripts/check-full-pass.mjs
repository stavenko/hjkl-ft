// ПОЛНЫЙ ФОНОВЫЙ ПРОХОД ПО ПРОДУКТУ — на живом приложении и живой модели.
//
// Сеет в дневник пустые продукты (ни признаков, ни нутриентов, ни железа, ни жира)
// и ждёт, пока очередь `classify::run_worker` разберёт их целиком: пять признаков,
// клетчатка, кальций, железо с усвоением, профиль жира. Печатает, что заполнилось,
// а что нет.
//
// Это проверка не промптов, а СВЯЗНОСТИ: опознание должно доехать до кальция, жиров
// и железа, очередь — не умереть на первом отказе, поля — лечь в базу.
//
// Нужен доступ к AI-воркеру: он 402-гейтится по подписке, поэтому аккаунту выдаётся
// подписанный dev-JWT и фейковая оплаченная подписка — тем же путём, что
// frontend/scripts/seed-test-subscription.mjs.
import { chromium } from "playwright";

const FE = process.env.FE || "https://renorma-fit-dev.pages.dev";
const PAY = process.env.PAY || "https://payment-worker-dev.vg-stavenko.workers.dev";
const SECRET = process.env.JWT_SECRET || "dev-secret-change-in-production";
// Названия, на которых были боли: редкая рыба, копчение, каталонская колбаса,
// заморозка, корнеплод, молочное с жирностью, рыбья печень.
const FOODS = (process.env.FOODS || [
  "Голец",
  "Скумбрия х/к",
  "Fuet truffle",
  "Мороженая вишня",
  "Картофель отварной",
  "Творог 5%",
  "Яичница глазунья",
].join("|")).split("|");
const WAIT_MS = Number(process.env.WAIT_MS || 420000);

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

const uid = `full-pass-${Date.now()}`;
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
const logs = [];
page.on("console", (m) => logs.push(`${m.type()}: ${m.text()}`));
const calls = [];
page.on("response", (r) => {
  const u = r.url();
  if (!/ai-worker|ai\.renorma/.test(u)) return;
  calls.push({ status: r.status(), url: u.replace(/^https?:\/\/[^/]+/, "") });
});

await page.goto(FE, { waitUntil: "domcontentloaded" });
await page.evaluate(({ uid, token }) => {
  localStorage.clear();
  localStorage.setItem("user_id", uid);
  localStorage.setItem("auth_token", token);
  localStorage.setItem("token_id", "t");
  localStorage.setItem("auth_ctx", "browser");
  localStorage.setItem("pwa_dismissed", "true");
}, { uid, token });
await page.goto(FE, { waitUntil: "domcontentloaded" });

const NEED = ["app_flags", "profile", "goals", "foods", "diary"];
let ready = false;
for (let i = 0; i < 60 && !ready; i++) {
  ready = await page.evaluate(async ({ uid, NEED }) => {
    const dbs = await indexedDB.databases();
    if (!dbs.some((d) => d.name === `hjkl-ft-${uid}`)) return false;
    return await new Promise((res) => {
      const q = indexedDB.open(`hjkl-ft-${uid}`);
      q.onsuccess = () => {
        const names = Array.from(q.result.objectStoreNames);
        q.result.close();
        res(NEED.every((n) => names.includes(n)));
      };
      q.onerror = () => res(false);
    });
  }, { uid, NEED }).catch(() => false);
  if (!ready) await page.waitForTimeout(500);
}
if (!ready) throw new Error("per-user база так и не появилась со всеми сторами");

await page.evaluate(async ({ uid, FOODS }) => {
  const db = await new Promise((res, rej) => {
    const q = indexedDB.open(`hjkl-ft-${uid}`);
    q.onsuccess = () => res(q.result); q.onerror = () => rej(q.error);
  });
  const nowIso = new Date().toISOString();
  const ymd = (o) => { const d = new Date(); d.setDate(d.getDate() - o);
    return `${d.getFullYear()}-${String(d.getMonth() + 1).padStart(2, "0")}-${String(d.getDate()).padStart(2, "0")}`; };
  const app_flags = [
    { key: "push_onboarding_dismissed", value: "true" },
    { key: "welcome_shown", value: "true" },
    { key: "activity_week_unlocked", value: "true" },
    { key: "calcium_week_unlocked", value: "true" },
    { key: "iron_week_unlocked", value: "true" },
    { key: "iron_week_opened_at", value: ymd(2) },
    { key: "fats_week_unlocked", value: "true" },
    { key: "red_meat_week_unlocked", value: "true" },
  ];
  const profile = [{ key: "profile", sex: "male", height_cm: 180,
    birth_year: new Date().getFullYear() - 40, goal: "lose", steps_planka: 9000,
    created_at: nowIso, updated_at: nowIso }];
  const goals = [{ id: "g-cal", nutrient: "Calories", key: "calories",
    direction: "AtMost", amount: 2500, unit: "Kcal", period: "Day",
    created_at: nowIso, updated_at: nowIso }];
  // Продукт совсем пустой: ни признаков, ни нутриентов, ни железа, ни жира.
  const foods = FOODS.map((name, i) => ({
    id: `f${i}`, name, kcal: 150, protein: 12, fat: 8, carbs: 5,
    nutrients: {},
    package_weight: null, is_recipe: false, recipe_id: null, archived: false,
    is_restaurant: false, is_snack: null, is_liquid_cal: null, is_veg_fruit: null,
    is_heme: null, is_milk_globule: null, is_red_meat: null, is_processed_meat: null,
    is_egg: null, iron_mg: null, iron_absorption: null, fat_profile: null,
    created_at: nowIso, updated_at: nowIso,
  }));
  const diary = FOODS.map((_, i) => ({
    id: `d${i}`, food_id: `f${i}`, date: ymd(0), time: null, grams: 100,
    waste_grams: 0, meal_label: "lunch", deleted: false, created_at: nowIso, updated_at: nowIso,
  }));
  for (const [store, rows] of Object.entries({ app_flags, profile, goals, foods, diary })) {
    await new Promise((res, rej) => {
      const tx = db.transaction([store], "readwrite");
      for (const row of rows) tx.objectStore(store).put(row);
      tx.oncomplete = () => res(); tx.onerror = () => rej(tx.error);
    });
  }
  db.close();
}, { uid, FOODS });

await page.goto(FE, { waitUntil: "domcontentloaded" });
await page.waitForSelector("#splash", { state: "detached", timeout: 20000 }).catch(() => {});

const readAll = () => page.evaluate(async (uid) => {
  const db = await new Promise((res, rej) => {
    const q = indexedDB.open(`hjkl-ft-${uid}`);
    q.onsuccess = () => res(q.result); q.onerror = () => rej(q.error);
  });
  const all = await new Promise((res) => {
    const rq = db.transaction(["foods"], "readonly").objectStore("foods").getAll();
    rq.onsuccess = () => res(rq.result); rq.onerror = () => res([]);
  });
  db.close();
  return all.map((f) => ({
    name: f.name,
    veg: f.is_veg_fruit, heme: f.is_heme, glob: f.is_milk_globule,
    red: f.is_red_meat, proc: f.is_processed_meat, egg: f.is_egg,
    ca: f.nutrients?.["Кальций"] ?? null,
    fibre: f.nutrients?.["Клетчатка"] ?? null,
    mg: f.iron_mg, abs: f.iron_absorption,
    fat: f.fat_profile,
  }));
}, uid);

// Продукт разобран, когда заполнено ВСЁ, что должен заполнить проход.
const done = (r) =>
  r.veg != null && r.heme != null && r.glob != null && r.red != null && r.proc != null &&
  r.egg != null &&
  r.ca != null && r.mg != null && r.abs != null && r.fat != null;

const t0 = Date.now();
let rows = [];
while (Date.now() - t0 < WAIT_MS) {
  rows = await readAll();
  if (rows.length && rows.every(done)) break;
  await page.waitForTimeout(4000);
}

const yn = (v) => (v === true ? "да" : v === false ? "нет" : "—");
console.log(`\n=== полный проход (ждали ${Math.round((Date.now() - t0) / 1000)} с) ===\n`);
console.log("продукт              ов гем гл км мгп яц  кальций клетч  железо        жир");
let ok = 0;
for (const r of rows) {
  if (done(r)) ok++;
  const fat = r.fat
    ? `${r.fat.sfa_pct.toFixed(0)}/${r.fat.mufa_pct.toFixed(0)}/${r.fat.pufa_pct.toFixed(0)}/${r.fat.epa_dha_pct.toFixed(0)}`
    : "—";
  console.log(
    `${done(r) ? "OK " : "НЕТ"} ${r.name.padEnd(18)} ` +
    `${yn(r.veg).padEnd(3)} ${yn(r.heme).padEnd(3)} ${yn(r.glob).padEnd(3)} ` +
    `${yn(r.red).padEnd(3)} ${yn(r.proc).padEnd(3)} ${yn(r.egg).padEnd(3)} ` +
    `${String(r.ca ?? "—").padEnd(7)} ${String(r.fibre ?? "—").padEnd(6)} ` +
    `${r.mg != null ? `${r.mg} мг × ${r.abs}` : "—"}`.padEnd(14) + ` ${fat}`
  );
}
console.log(`\nразобрано полностью ${ok} из ${rows.length}`);
console.log("(ов — овощ/фрукт, гем, гл — молочная глобула, км — красное мясо, мгп — мясная гастрономия, яц — яйцо)");

console.log(`\n=== обращения к AI-воркеру: ${calls.length} ===`);
const byStatus = calls.reduce((m, c) => ((m[c.status] = (m[c.status] ?? 0) + 1), m), {});
console.log(`  по статусам: ${JSON.stringify(byStatus)}`);
const errs = [...new Set(logs.filter((l) => /error|ошибк|не разобран|unknown/i.test(l)))];
if (errs.length) {
  console.log("\n=== что говорило приложение об ошибках ===");
  for (const l of errs.slice(-20)) console.log("  " + l.slice(0, 220));
}
const said = [...new Set(logs.filter((l) => /calcium|iron|жиры|железо|признаки|опозна/i.test(l)))];
if (said.length) {
  console.log("\n=== что приложение записало в лог ===");
  for (const l of said.slice(-40)) console.log("  " + l.slice(0, 220));
}
// Снимок дашборда в ТОМ ЖЕ прогоне: разобранное должно не только лечь в базу, но и
// нарисоваться. Отдельным скриптом это не снять — пришлось бы сеять руками то, что
// приложение и так только что посчитало.
if (process.env.SHOT) {
  await page.reload({ waitUntil: "domcontentloaded" });
  await page.waitForSelector("#splash", { state: "detached", timeout: 20000 }).catch(() => {});
  await page.waitForTimeout(4000);
  await page.screenshot({ path: process.env.SHOT, fullPage: true });
  const screen = (await page.locator("body").innerText().catch(() => "")).replace(/\n+/g, " | ");
  console.log(`\nэкран: ${screen.slice(0, 700)}`);
}
await b.close();
process.exit(ok === rows.length ? 0 : 1);
