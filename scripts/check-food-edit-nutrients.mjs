// Форма правки продукта перечисляет нутриенты ПО СПИСКУ, а не по тому, что уже
// лежит в продукте.
//
// После инвалидации кальция (и у только что заведённого продукта) значения нет —
// и форма показывала строку не для всех: кальция в ней просто не было, вписать его
// руками было негде. Сеем продукт с ПУСТОЙ картой нутриентов и смотрим, что строки
// на месте. Заодно: пустое поле не должно сохраниться нулём — иначе фоновый
// проход решит, что нутриент выяснен, и никогда его не запросит.
import { chromium } from "playwright";

const BASE = process.env.FE || "https://renorma-fit-dev.pages.dev";
const PAY = process.env.PAY || "https://payment-worker-dev.vg-stavenko.workers.dev";
const SECRET = process.env.JWT_SECRET || "dev-secret-change-in-production";

let fail = 0;
const check = (n, ok, extra = "") => { console.log(`${ok ? "OK " : "FAIL"} ${n}${extra ? " — " + extra : ""}`); if (!ok) fail++; };

const b64url = (buf) => Buffer.from(buf).toString("base64url");
const uid = `fedit-${Date.now()}`;
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
        res(["foods", "diary", "app_flags"].every((s) => n.includes(s))); };
      q.onerror = () => res(false);
    });
  }, uid).catch(() => false);
  if (ok) break;
  await page.waitForTimeout(500);
}

await page.evaluate(async (u) => {
  const db = await new Promise((res, rej) => {
    const q = indexedDB.open(`hjkl-ft-${u}`);
    q.onsuccess = () => res(q.result); q.onerror = () => rej(q.error);
  });
  const nowIso = new Date().toISOString();
  const d = new Date();
  const today = `${d.getFullYear()}-${String(d.getMonth() + 1).padStart(2, "0")}-${String(d.getDate()).padStart(2, "0")}`;
  const rows = {
    // Версия заведомо выше любой миграции: этот тест про форму, а не про них.
    app_flags: [{ key: "db_schema_version", value: "999" },
      { key: "push_onboarding_dismissed", value: "true" }, { key: "welcome_shown", value: "true" }],
    // Нутриентов НЕТ вовсе — ровно состояние после инвалидации.
    foods: [{ id: "f1", name: "Треска", kcal: 78, protein: 17, fat: 0.7, carbs: 0,
      nutrients: {}, package_weight: null, is_recipe: false, recipe_id: null, archived: false,
      is_restaurant: false, is_snack: false, is_liquid_cal: false, is_veg_fruit: false,
      is_egg: false, is_red_meat: false, iron_mg: null, iron_absorption: null,
      created_at: nowIso, updated_at: nowIso }],
    diary: [{ id: "d1", food_id: "f1", date: today, time: null, grams: 200, waste_grams: 0,
      meal_label: "lunch", deleted: false, created_at: nowIso, updated_at: nowIso }],
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

await page.goto(`${BASE}/diary`, { waitUntil: "domcontentloaded" });
await page.waitForSelector("#splash", { state: "detached", timeout: 20000 }).catch(() => {});
await page.waitForTimeout(3000);

// Правка открывается из меню строки дневника: «⋮» → «Изменить». Долгого нажатия
// больше нет: вчера и сегодня устроены одинаково, и меню у обоих вызывается
// кнопкой.
await page.getByText("Треска").first().waitFor({ timeout: 15000 });
await page.locator('[data-testid="diary-row-menu"]').first().click();
await page.locator('[data-testid="diary-menu-edit"]').first().click();
await page.waitForSelector('[data-testid="food-edit-page"]', { timeout: 15000 });

const block = await page.locator('[data-testid="ai-found-block"]').innerText();
console.log("блок «Найдено автоматически»:\n  " + block.replace(/\n+/g, " | "));
// Спрашиваемый нутриент остался ОДИН — кальций; клетчатку отдаёт растительный
// узел конвейера признаков, и своей строки в форме у неё больше нет. Омега-3
// показывается не как нутриент, а как часть состава жира: «из них омега-3».
for (const n of ["Кальций", "Железо", "Усвоение железа", "из них омега-3"]) {
  check(`строка «${n}» на месте`, block.includes(n), block.includes(n) ? "" : "нет в блоке");
}

// Сохраняем, ничего не введя: пустые поля не должны записаться нулями.
await page.locator('[data-testid="food-edit-save"]').click();
await page.waitForTimeout(2500);
const saved = await page.evaluate(async (u) => {
  const db = await new Promise((res, rej) => {
    const q = indexedDB.open(`hjkl-ft-${u}`);
    q.onsuccess = () => res(q.result); q.onerror = () => rej(q.error);
  });
  const all = await new Promise((res) => {
    const rq = db.transaction(["foods"], "readonly").objectStore("foods").getAll();
    rq.onsuccess = () => res(rq.result); rq.onerror = () => res([]);
  });
  db.close();
  const f = all.find((x) => x.name === "Треска");
  const n = f?.nutrients instanceof Map ? Object.fromEntries(f.nutrients) : (f?.nutrients || {});
  return { keys: Object.keys(n), mg: f?.iron_mg };
}, uid);
console.log(`после сохранения пустой формы: нутриенты ${JSON.stringify(saved.keys)}, железо ${saved.mg}`);
check("пустые поля не записались нулями", saved.keys.length === 0, JSON.stringify(saved.keys));
check("пустое железо осталось незаполненным", saved.mg == null, String(saved.mg));

console.log(fail === 0 ? "\n=== ALL OK ===" : `\n=== FAILURES: ${fail} ===`);
await b.close();
process.exit(fail === 0 ? 0 : 1);
