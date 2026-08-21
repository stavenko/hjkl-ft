// ЖИРЫ НЕ СПРАШИВАЮТСЯ ПОВТОРНО В ТЕ ЖЕ СУТКИ.
//
// Боевой случай: «SPAR Чипсы нори» — модель не смогла назвать профиль жира, и
// проход спрашивал её при каждом запуске и каждом возвращении из фона. Человек
// получал в журнал одну и ту же ошибку столбиком.
//
// Здесь продукт заводится без профиля жира; приложение открывается ДВАЖДЫ.
// Считаются запросы к ai-worker и записи следа: во второй раз ни того, ни другого
// быть не должно.
import { chromium } from "playwright";

const BASE = process.env.FE || "https://renorma-fit-dev.pages.dev";
const PAY = process.env.PAY || "https://payment-worker-dev.vg-stavenko.workers.dev";
const SECRET = process.env.JWT_SECRET || "dev-secret-change-in-production";
const uid = `fatgate-${Date.now()}`;

const b64url = (buf) => Buffer.from(buf).toString("base64url");
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

/// Открыть приложение и посчитать запросы к модели за `ms`.
async function session(ms, seed) {
  const page = await ctx.newPage();
  let asks = 0;
  page.on("request", (r) => { if (r.url().includes("/chat/completions")) asks++; });
  await page.goto(BASE, { waitUntil: "domcontentloaded" });
  await page.evaluate(({ uid, token }) => {
    localStorage.setItem("user_id", uid);
    localStorage.setItem("auth_token", token);
    localStorage.setItem("token_id", "t");
    localStorage.setItem("auth_ctx", "browser");
    localStorage.setItem("pwa_dismissed", "true");
  }, { uid, token });
  await page.goto(BASE, { waitUntil: "domcontentloaded" });
  for (let i = 0; i < 60 && seed; i++) {
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
  if (seed) {
    await page.evaluate(async (u) => {
      const db = await new Promise((res, rej) => {
        const q = indexedDB.open(`hjkl-ft-${u}`);
        q.onsuccess = () => res(q.result); q.onerror = () => rej(q.error);
      });
      const nowIso = new Date().toISOString();
      await new Promise((res) => {
        const tx = db.transaction(["app_flags"], "readwrite");
        [["db_schema_version", "999"], ["welcome_shown", "true"],
         ["push_onboarding_dismissed", "true"]].forEach(([key, value]) =>
          tx.objectStore("app_flags").put({ key, value }));
        tx.oncomplete = res;
      });
      // Всё выяснено, кроме ПРОФИЛЯ ЖИРА: ровно та ситуация, что у «Чипсов нори».
      await new Promise((res) => {
        const tx = db.transaction(["foods"], "readwrite");
        tx.objectStore("foods").put({
          id: "f1", name: "SPAR Чипсы нори", kcal: 500, protein: 5, fat: 30, carbs: 50,
          nutrients: { "Кальций": 20, "Клетчатка": 2 }, package_weight: null,
          is_recipe: false, recipe_id: null, archived: false, is_restaurant: false,
          is_veg_fruit: false, is_heme: false, is_milk_globule: false,
          is_red_meat: false, is_processed_meat: false, is_egg: false,
          iron_mg: 1, iron_absorption: 0.05,
          fat_profile: null, balance_fat_profile: null,
          created_at: nowIso, updated_at: nowIso,
        });
        tx.oncomplete = res;
      });
      db.close();
    }, uid);
    await page.goto(BASE, { waitUntil: "domcontentloaded" });
  }
  await page.waitForTimeout(ms);
  const probes = await page.evaluate(async (u) => {
    const db = await new Promise((res, rej) => {
      const q = indexedDB.open(`hjkl-ft-${u}`);
      q.onsuccess = () => res(q.result); q.onerror = () => rej(q.error);
    });
    const all = await new Promise((res) => {
      const rq = db.transaction(["food_probe"]).objectStore("food_probe").getAll();
      rq.onsuccess = () => res(rq.result); rq.onerror = () => res([]);
    });
    db.close();
    return all.map((p) => `${p.aspect}:${p.ok ? "ok" : "нет"}`);
  }, uid);
  await page.close();
  return { asks, probes };
}

/// Третий сценарий — САМЫЙ ВАЖНЫЙ: модель не смогла, след неудачи свежий.
/// Профиль по-прежнему пуст, но спрашивать нельзя до истечения суток.
async function afterFailure(ms) {
  const page = await ctx.newPage();
  let asks = 0;
  page.on("request", (r) => { if (r.url().includes("/chat/completions")) asks++; });
  await page.goto(BASE, { waitUntil: "domcontentloaded" });
  await page.evaluate(async (u) => {
    const db = await new Promise((res, rej) => {
      const q = indexedDB.open(`hjkl-ft-${u}`);
      q.onsuccess = () => res(q.result); q.onerror = () => rej(q.error);
    });
    await new Promise((res) => {
      const tx = db.transaction(["foods", "food_probe"], "readwrite");
      const foods = tx.objectStore("foods");
      const rq = foods.get("f1");
      rq.onsuccess = () => {
        const f = rq.result;
        f.fat_profile = null;           // профиль снова пуст
        foods.put(f);
      };
      // Свежая НЕУДАЧНАЯ попытка по жирам.
      tx.objectStore("food_probe").put({
        key: "f1:fats", food_id: "f1", aspect: "fats", attempted_at: Date.now(),
        ok: false, note: "модель не смогла", identity: "", identity_weight: 0, lease_until: 0,
      });
      tx.oncomplete = res;
    });
    db.close();
  }, uid);
  await page.goto(BASE, { waitUntil: "domcontentloaded" });
  await page.waitForTimeout(ms);
  await page.close();
  return asks;
}

const first = await session(70000, true);
console.log(`первый заход:  запросов к модели ${first.asks} · след ${JSON.stringify(first.probes)}`);
const second = await session(45000, false);
console.log(`второй заход:  запросов к модели ${second.asks} · след ${JSON.stringify(second.probes)}`);
const third = await afterFailure(45000);
console.log(`после НЕУДАЧИ (профиль пуст, след свежий): запросов к модели ${third}`);
const ok = second.asks === 0 && third === 0;
console.log(ok ? "повторных запросов нет — суточный след держит" : "ПОВТОРНЫЙ ОПРОС: гейт не сработал");
await browser.close();
process.exit(ok ? 0 : 1);
