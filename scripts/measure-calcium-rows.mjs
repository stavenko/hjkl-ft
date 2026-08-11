// Замер кальция ЧЕРЕЗ ЖИВОЙ ПУТЬ приложения: продукты заводятся с пустой картой
// нутриентов, фоновый проход спрашивает модель, а мы читаем, что он записал.
//
// Меряется именно то, что работает у людей: промпт рендерится из таблицы категорий
// в ai.rs, и копии промпта здесь нет — иначе проверка мерила бы копию.
//
// Ожидания — диапазоны строк таблицы плюс справочные значения продуктов. Продукт
// считается разобранным верно, если записанное число попало в свой диапазон.
import { chromium } from "playwright";

const BASE = process.env.FE || "https://renorma-fit-dev.pages.dev";
const PAY = process.env.PAY || "https://payment-worker-dev.vg-stavenko.workers.dev";
const SECRET = process.env.JWT_SECRET || "dev-secret-change-in-production";

// [название, ожидаемый диапазон мг/100 г, чем обосновано]
const CASES = [
  ["Кижуч без головы", [0, 0], "рыба без костей → строка other_none"],
  ["Треска", [0, 0], "рыба без костей → other_none"],
  ["Куриная грудка", [0, 0], "мясо → other_none"],
  ["Сардины в масле", [120, 450], "fish_with_bones"],
  ["Молоко 3.2%", [90, 150], "milk_liquid"],
  ["Пармезан", [600, 1300], "cheese_hard"],
  ["Творог 5%", [80, 200], "cottage_cheese"],
  ["Миндаль", [200, 300], "nuts_high_calcium"],
  ["Кунжут", [500, 1500], "seeds_high"],
  ["Брокколи", [25, 90], "vegetables_other"],
  ["Соевое молоко без добавок", [5, 40], "plant_milk_plain"],
  ["Миндальное молоко, обогащённое кальцием", [100, 180], "fortified"],
];

const b64url = (buf) => Buffer.from(buf).toString("base64url");
const uid = `calmeas-${Date.now()}`;
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

await page.evaluate(async ({ u, names }) => {
  const db = await new Promise((res, rej) => {
    const q = indexedDB.open(`hjkl-ft-${u}`);
    q.onsuccess = () => res(q.result); q.onerror = () => rej(q.error);
  });
  const nowIso = new Date().toISOString();
  const flags = [{ key: "db_schema_version", value: "999" },
    { key: "push_onboarding_dismissed", value: "true" }, { key: "welcome_shown", value: "true" }];
  await new Promise((res, rej) => {
    const tx = db.transaction(["app_flags"], "readwrite");
    for (const f of flags) tx.objectStore("app_flags").put(f);
    tx.oncomplete = () => res(); tx.onerror = () => rej(tx.error);
  });
  await new Promise((res, rej) => {
    const tx = db.transaction(["foods"], "readwrite");
    names.forEach((name, i) => tx.objectStore("foods").put({
      id: `m${i}`, name, kcal: 100, protein: 5, fat: 5, carbs: 5,
      // Карта нутриентов ПУСТА — именно это и заставляет проход спросить.
      nutrients: {}, package_weight: null, is_recipe: false, recipe_id: null,
      archived: false, is_restaurant: false, is_veg_fruit: null, is_heme: null,
      iron_mg: null, iron_absorption: null, fat_profile: null,
      created_at: nowIso, updated_at: nowIso,
    }));
    tx.oncomplete = () => res(); tx.onerror = () => rej(tx.error);
  });
  db.close();
}, { u: uid, names: CASES.map(([n]) => n) });

// Перезапуск — проход идёт на активации.
await page.goto(BASE, { waitUntil: "domcontentloaded" });
const DEADLINE = Date.now() + 12 * 60 * 1000;
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
    return Object.fromEntries(all.map((f) => [f.name, f.nutrients?.["Кальций"]]));
  }, uid);
  const done = CASES.filter(([n]) => got[n] !== undefined).length;
  console.log(`заполнено ${done}/${CASES.length}`);
  if (done === CASES.length) break;
}

let bad = 0;
console.log("\nпродукт                                  наш   ожидание   строка");
for (const [name, [lo, hi], why] of CASES) {
  const v = got[name];
  const ok = v !== undefined && v >= lo && v <= hi;
  if (!ok) bad++;
  console.log(`${ok ? "OK  " : "MISS"} ${name.padEnd(38)} ${String(v ?? "—").padStart(5)}   ${lo}–${hi}   ${why}`);
}
console.log(`\nпопаданий: ${CASES.length - bad}/${CASES.length}`);

await ctx.close();
await b.close();
process.exit(bad ? 1 : 0);
