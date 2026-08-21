// КЭШ ОПОЗНАНИЯ И АРЕНДА — живая проверка на dev.
//
// Проверяется ровно то, ради чего это делалось:
//   1. первый проход опознаёт продукт и запоминает опознание;
//   2. второй проход (не хватает ЕЩЁ ОДНОГО признака) опознание НЕ переспрашивает;
//   3. две вкладки, открытые одновременно, опознают продукт ОДИН раз, а не дважды.
//
// Считаются строки консоли «опознание «X»: …» — их печатает узел опознания. Их
// количество и есть предмет замера: платим мы именно за эти запросы.
import { chromium } from "playwright";

const BASE = process.env.FE || "https://renorma-fit-dev.pages.dev";
const PAY = process.env.PAY || "https://payment-worker-dev.vg-stavenko.workers.dev";
const SECRET = process.env.JWT_SECRET || "dev-secret-change-in-production";
const FOOD = process.env.FOOD || "Кабачок";

const b64url = (buf) => Buffer.from(buf).toString("base64url");
const uid = `cache-${Date.now()}`;
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

const browser = await chromium.launch({ headless: true });
const ctx = await browser.newContext({ viewport: { width: 430, height: 932 }, serviceWorkers: "block" });

/// Одна страница со счётчиком опознаний.
async function openPage() {
  const page = await ctx.newPage();
  const seen = [];
  page.on("console", (m) => {
    const g = m.text().match(/^опознание «(.+?)»:/);
    if (g) seen.push(g[1]);
  });
  return { page, seen };
}

const seedFlags = async (page, flags) =>
  page.evaluate(async ({ u, food, flags }) => {
    const db = await new Promise((res, rej) => {
      const q = indexedDB.open(`hjkl-ft-${u}`);
      q.onsuccess = () => res(q.result); q.onerror = () => rej(q.error);
    });
    const nowIso = new Date().toISOString();
    await new Promise((res) => {
      const tx = db.transaction(["app_flags"], "readwrite");
      [{ key: "db_schema_version", value: "999" }, { key: "push_onboarding_dismissed", value: "true" },
       { key: "welcome_shown", value: "true" }].forEach((f) => tx.objectStore("app_flags").put(f));
      tx.oncomplete = res;
    });
    await new Promise((res) => {
      const tx = db.transaction(["foods"], "readwrite");
      tx.objectStore("foods").put({
        id: "f1", name: food, kcal: 20, protein: 1, fat: 0, carbs: 4,
        nutrients: { "Кальций": 20, "Клетчатка": 1 }, package_weight: null,
        is_recipe: false, recipe_id: null, archived: false, is_restaurant: false,
        ...flags,
        iron_mg: 0.5, iron_absorption: 0.05,
        fat_profile: { sfa_pct: 20, mufa_pct: 30, pufa_pct: 50, epa_dha_pct: 0 },
        balance_fat_profile: null, created_at: nowIso, updated_at: nowIso,
      });
      tx.oncomplete = res;
    });
    db.close();
  }, { u: uid, food: FOOD, flags });

const readFlags = (page) =>
  page.evaluate(async (u) => {
    const db = await new Promise((res, rej) => {
      const q = indexedDB.open(`hjkl-ft-${u}`);
      q.onsuccess = () => res(q.result); q.onerror = () => rej(q.error);
    });
    const f = await new Promise((res) => {
      const rq = db.transaction(["foods"]).objectStore("foods").get("f1");
      rq.onsuccess = () => res(rq.result); rq.onerror = () => res(null);
    });
    const probes = await new Promise((res) => {
      const rq = db.transaction(["food_probe"]).objectStore("food_probe").getAll();
      rq.onsuccess = () => res(rq.result); rq.onerror = () => res([]);
    });
    db.close();
    return { food: f, probes };
  }, uid);

const boot = async (page) => {
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
          res(["foods", "food_probe"].every((s) => n.includes(s))); };
        q.onerror = () => res(false);
      });
    }, uid).catch(() => false);
    if (ok) break;
    await page.waitForTimeout(500);
  }
};

/// Ждать, пока перечисленные признаки перестанут быть null.
async function waitFlags(page, names, limitMs = 6 * 60 * 1000) {
  const until = Date.now() + limitMs;
  while (Date.now() < until) {
    const { food } = await readFlags(page);
    if (food && names.every((n) => food[n] === true || food[n] === false)) return food;
    await page.waitForTimeout(4000);
  }
  return null;
}

// ── 1. Первый проход: не хватает ОДНОГО признака ────────────────────────────
const one = await openPage();
await boot(one.page);
await seedFlags(one.page, {
  is_veg_fruit: null, is_heme: false, is_milk_globule: false,
  is_red_meat: false, is_processed_meat: false, is_egg: false,
});
await one.page.goto(BASE, { waitUntil: "domcontentloaded" });
const first = await waitFlags(one.page, ["is_veg_fruit"]);
console.log(`\n1) первый проход: овощ/фрукт = ${first?.is_veg_fruit} · опознаний ${one.seen.length}`);
const { probes } = await readFlags(one.page);
const cached = probes.find((p) => p.aspect === "identity");
console.log(`   в кэше: «${(cached?.identity || "").slice(0, 60)}» вес ${cached?.identity_weight}`);

// ── 2. Второй проход: не хватает ДРУГОГО признака ───────────────────────────
const two = await openPage();
await two.page.goto(BASE, { waitUntil: "domcontentloaded" });
await seedFlags(two.page, {
  is_veg_fruit: first?.is_veg_fruit ?? true, is_heme: null, is_milk_globule: false,
  is_red_meat: false, is_processed_meat: false, is_egg: false,
});
await two.page.goto(BASE, { waitUntil: "domcontentloaded" });
const second = await waitFlags(two.page, ["is_heme"]);
console.log(`2) второй проход: гем = ${second?.is_heme} · опознаний ${two.seen.length}` +
  (two.seen.length === 0 ? "  ← взято из кэша" : "  ← ОПОЗНАНИЕ ПОВТОРИЛОСЬ"));

// ── 3. Две вкладки разом: продукт должны опознать один раз ──────────────────
await seedFlags(two.page, {
  is_veg_fruit: null, is_heme: null, is_milk_globule: null,
  is_red_meat: null, is_processed_meat: null, is_egg: null,
});
await two.page.evaluate(async (u) => {
  const db = await new Promise((res, rej) => {
    const q = indexedDB.open(`hjkl-ft-${u}`);
    q.onsuccess = () => res(q.result); q.onerror = () => rej(q.error);
  });
  // Кэш и следы стираем: третий заход должен быть «как в первый раз».
  await new Promise((res) => {
    const tx = db.transaction(["food_probe"], "readwrite");
    tx.objectStore("food_probe").clear();
    tx.oncomplete = res;
  });
  db.close();
}, uid);

const a = await openPage();
const b = await openPage();
await Promise.all([
  a.page.goto(BASE, { waitUntil: "domcontentloaded" }),
  b.page.goto(BASE, { waitUntil: "domcontentloaded" }),
]);
await waitFlags(a.page, ["is_veg_fruit", "is_heme", "is_egg"]);
const total = a.seen.length + b.seen.length;
console.log(`3) две вкладки разом: опознаний ${total} (вкладка A ${a.seen.length}, B ${b.seen.length})` +
  (total <= 1 ? "  ← аренда сработала" : "  ← ПРОДУКТ ОПОЗНАН НЕСКОЛЬКО РАЗ"));

await browser.close();
