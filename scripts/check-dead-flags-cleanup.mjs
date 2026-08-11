// Поля снятых классификаторов уходят из базы, а данные собираются с первого дня.
//
// Проверяется два обещания:
//   1. миграция 10 стирает у продуктов ключи is_snack / is_liquid_cal / is_egg /
//      is_red_meat — они висели в форме и врали (у рыбы стояло «Красное мясо: да»);
//   2. фоновый проход больше не ждёт открытия недель: продукт без профиля жира и
//      без железа ставится в очередь СРАЗУ, при закрытых жирах и закрытом железе.
//
// Второе меряется по самому продукту: очередь ходит к модели, ответ может и не
// прийти, а вот факт постановки в очередь виден по тому, что поля начинают
// заполняться. Поэтому смотрим на запрос к ai-воркеру, а не на итоговое значение.
import { chromium } from "playwright";

const BASE = process.env.FE || "https://renorma-fit-dev.pages.dev";
const PAY = process.env.PAY || "https://payment-worker-dev.vg-stavenko.workers.dev";
const SECRET = process.env.JWT_SECRET || "dev-secret-change-in-production";

let fail = 0;
const check = (n, ok, extra = "") => {
  console.log(`${ok ? "OK  " : "FAIL"} ${n}${extra ? " — " + extra : ""}`);
  if (!ok) fail++;
};

const b64url = (buf) => Buffer.from(buf).toString("base64url");
const uid = `dead-${Date.now()}`;
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
// Запросы к модели на профиль жира и железо — свидетельство, что проход пошёл, не
// дожидаясь открытия недель.
const asked = [];
page.on("request", (r) => {
  const u = r.url();
  if (/ai-worker|ai\.renorma/.test(u)) asked.push(u);
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

// Продукт СО СТАРЫМИ полями и версией базы 9 — чтобы прогналась миграция 10.
// Недели жиров и железа НЕ открыты: ни одного соответствующего флага нет.
await page.evaluate(async (u) => {
  const db = await new Promise((res, rej) => {
    const q = indexedDB.open(`hjkl-ft-${u}`);
    q.onsuccess = () => res(q.result); q.onerror = () => rej(q.error);
  });
  const nowIso = new Date().toISOString();
  const rows = {
    app_flags: [{ key: "db_schema_version", value: "9" },
      { key: "push_onboarding_dismissed", value: "true" }, { key: "welcome_shown", value: "true" }],
    foods: [{ id: "old", name: "Кижуч без головы", kcal: 180, protein: 21, fat: 8, carbs: 0,
      nutrients: { "Кальций": 36, "Клетчатка": 0 }, package_weight: null, is_recipe: false,
      recipe_id: null, archived: false, is_restaurant: false,
      is_snack: false, is_liquid_cal: false, is_veg_fruit: false,
      is_egg: false, is_red_meat: true, is_heme: false,
      iron_mg: 0.5, iron_absorption: 0.15, fat_profile: null,
      created_at: nowIso, updated_at: nowIso }],
  };
  for (const [store, list] of Object.entries(rows)) {
    await new Promise((res, rej) => {
      const tx = db.transaction([store], "readwrite");
      for (const r of list) tx.objectStore(store).put(r);
      tx.oncomplete = () => res(); tx.onerror = () => rej(tx.error);
    });
  }
  db.close();
}, uid);

await page.goto(BASE, { waitUntil: "domcontentloaded" });
await page.waitForTimeout(15000);

const stored = await page.evaluate(async (u) => {
  const db = await new Promise((res, rej) => {
    const q = indexedDB.open(`hjkl-ft-${u}`);
    q.onsuccess = () => res(q.result); q.onerror = () => rej(q.error);
  });
  const one = await new Promise((res) => {
    const rq = db.transaction(["foods"]).objectStore("foods").get("old");
    rq.onsuccess = () => res(rq.result); rq.onerror = () => res(null);
  });
  const ver = await new Promise((res) => {
    const rq = db.transaction(["app_flags"]).objectStore("app_flags").get("db_schema_version");
    rq.onsuccess = () => res(rq.result?.value); rq.onerror = () => res(null);
  });
  db.close();
  return { one, ver };
}, uid);

check("миграция прогналась", Number(stored.ver) >= 10, String(stored.ver));
for (const k of ["is_snack", "is_liquid_cal", "is_egg", "is_red_meat"]) {
  check(`ключ ${k} стёрт`, stored.one && stored.one[k] === undefined, JSON.stringify(stored.one?.[k]));
}
check("живые поля на месте", stored.one?.is_heme === false && stored.one?.is_veg_fruit === false
  && stored.one?.iron_mg === 0.5, JSON.stringify(stored.one));
check("профиль жира запрошен при ЗАКРЫТОЙ неделе жиров", asked.length > 0,
  `запросов к модели: ${asked.length}`);

await ctx.close();
await b.close();
console.log(fail ? `\n${fail} провалов` : "\nвсё сошлось");
process.exit(fail ? 1 : 0);
