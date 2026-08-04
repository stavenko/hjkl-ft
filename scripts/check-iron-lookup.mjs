// Железный проход по продуктам — на РЕАЛЬНОЙ модели, не на заглушке.
//
// Сеет в дневник продукты БЕЗ железа (в том числе те самые названия, на которых
// пользователь получил ошибки), открывает неделю железа и ждёт, пока фоновая
// очередь их разберёт. Печатает, что заполнилось, а что упало и с какой ошибкой.
//
// Нужен доступ к AI-воркеру: он 402-гейтится по подписке, поэтому аккаунту
// выдаётся подписанный dev-JWT и фейковая оплаченная подписка — тем же путём,
// что frontend/scripts/seed-test-subscription.mjs.
import { chromium } from "playwright";

const FE = process.env.FE || "https://renorma-fit-dev.pages.dev";
const PAY = process.env.PAY || "https://payment-worker-dev.vg-stavenko.workers.dev";
const SECRET = process.env.JWT_SECRET || "dev-secret-change-in-production";
// Названия, на которых пользователь видел ошибки, + пара обычных для фона.
const FOODS = (process.env.FOODS || [
  "ВкусВилл Творог обезжиренный",
  "ВкусВилл Йогурт 2,1%",
  "Яблоко",
  "Куриная печень",
  "Говядина",
  "Чечевица",
].join("|")).split("|");
const WAIT_MS = Number(process.env.WAIT_MS || 180000);

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

const uid = `iron-lookup-${Date.now()}`;
const token = await jwt(uid);
// Фейковая оплаченная подписка, иначе AI-воркер ответит 402.
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
// Ошибки прохода печатаются в консоль приложения — ловим их дословно.
const logs = [];
page.on("console", (m) => logs.push(`${m.type()}: ${m.text()}`));
// Ошибки прохода приложение кладёт в свой журнал (в памяти), а не в консоль,
// поэтому диагностику ведём по СЕТИ: сколько запросов ушло в AI-воркер и с
// какими ответами. Без этого «ноль разобранных» неотличим от «очередь даже не
// стартовала».
const calls = [];
page.on("response", async (r) => {
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
// Ждём, пока приложение СОЗДАСТ per-user базу со ВСЕМИ нужными сторами. Открыть
// её раньше — значит поймать промежуточную версию схемы и упасть на транзакции.
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
  ];
  // Профиль обязателен: без него приложение стоит на экране «Персона».
  const profile = [{ key: "profile", sex: "male", height_cm: 180,
    birth_year: new Date().getFullYear() - 40, goal: "lose", steps_planka: 9000,
    created_at: nowIso, updated_at: nowIso }];
  const goals = [{ id: "g-cal", nutrient: "Calories", key: "calories",
    direction: "AtMost", amount: 2500, unit: "Kcal", period: "Day",
    created_at: nowIso, updated_at: nowIso }];
  // Продукты БЕЗ железа — именно их должен разобрать железный проход.
  const foods = FOODS.map((name, i) => ({
    id: `f${i}`, name, kcal: 100, protein: 5, fat: 3, carbs: 10,
    nutrients: { "Кальций": 100, "Клетчатка": 2, "Омега-3": 10 },
    package_weight: null, is_recipe: false, recipe_id: null, archived: false,
    // Теги и нутриенты тоже не проставлены — продукт совсем новый, как у
    // пользователя. Так видно, работает ли фоновая очередь вообще или молчит
    // именно железный проход.
    is_restaurant: false, is_snack: null, is_liquid_cal: null, is_veg_fruit: null,
    is_egg: null, is_red_meat: null, iron_mg: null, iron_absorption: null,
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

// Ждём, пока очередь разберёт все продукты (или истечёт бюджет).
const readIron = () => page.evaluate(async (uid) => {
  const db = await new Promise((res, rej) => {
    const q = indexedDB.open(`hjkl-ft-${uid}`);
    q.onsuccess = () => res(q.result); q.onerror = () => rej(q.error);
  });
  const all = await new Promise((res) => {
    const rq = db.transaction(["foods"], "readonly").objectStore("foods").getAll();
    rq.onsuccess = () => res(rq.result); rq.onerror = () => res([]);
  });
  db.close();
  return all.map((f) => ({ name: f.name, mg: f.iron_mg, abs: f.iron_absorption }));
}, uid);

const t0 = Date.now();
let rows = [];
while (Date.now() - t0 < WAIT_MS) {
  rows = await readIron();
  if (rows.every((r) => r.mg != null && r.abs != null)) break;
  await page.waitForTimeout(3000);
}

console.log(`\n=== железо по продуктам (ждали ${Math.round((Date.now() - t0) / 1000)} с) ===`);
let ok = 0;
for (const r of rows) {
  const done = r.mg != null && r.abs != null;
  if (done) ok++;
  console.log(`${done ? "OK " : "НЕТ"} ${r.name.padEnd(32)} ${done ? `${r.mg} мг, усвоение ${r.abs}` : "—"}`);
}
console.log(`\nразобрано ${ok} из ${rows.length}`);
// Диагностика, когда ничего не разобралось: что вообще осталось в базе и что на
// экране — «очередь не стартовала» и «данные пропали» выглядят одинаково.
const diag = await page.evaluate(async (uid) => {
  const db = await new Promise((res, rej) => {
    const q = indexedDB.open(`hjkl-ft-${uid}`);
    q.onsuccess = () => res(q.result); q.onerror = () => rej(q.error);
  });
  const count = (store) => new Promise((res) => {
    if (!Array.from(db.objectStoreNames).includes(store)) return res(-1);
    const rq = db.transaction([store], "readonly").objectStore(store).count();
    rq.onsuccess = () => res(rq.result); rq.onerror = () => res(-2);
  });
  const out = { foods: await count("foods"), diary: await count("diary") };
  db.close();
  return out;
}, uid);
const ironVisible = await page.locator('[data-gauge="Железо/нед"]').isVisible().catch(() => false);
console.log(`неделя железа открыта в UI: ${ironVisible}`);
const screen = (await page.locator("body").innerText().catch(() => "")).replace(/\s+/g, " ").slice(0, 120);
console.log(`\nв базе: продуктов ${diag.foods}, записей дневника ${diag.diary}`);
console.log(`на экране: ${screen}`);
console.log(`\n=== обращения к AI-воркеру: ${calls.length} ===`);
for (const c of calls.slice(0, 12)) console.log(`  ${c.status} ${c.url}`);
if (logs.length) {
  console.log("\n=== что говорило приложение ===");
  for (const l of [...new Set(logs)].slice(-25)) console.log("  " + l.slice(0, 200));
}
await b.close();
process.exit(ok === rows.length ? 0 : 1);
