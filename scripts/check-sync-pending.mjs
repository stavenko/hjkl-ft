// Незавершённая синхронизация видна человеку, и кнопка её дожимает.
//
// Проверяется живой путь: заводим запись в дневник ОФЛАЙН (запросы к sync-воркеру
// рубим), очередь остаётся непустой → под треугольником должно появиться
// предупреждение. Возвращаем сеть, жмём «Продолжить синхронизацию» → очередь
// пустеет, предупреждение исчезает, а запись доезжает до СЕРВЕРА (это и
// проверяется независимым pull от второго «устройства» с тем же аккаунтом).
import { chromium } from "playwright";

const BASE = process.env.FE || "https://renorma-fit-dev.pages.dev";
const PAY = process.env.PAY || "https://payment-worker-dev.vg-stavenko.workers.dev";
const SYNC = process.env.SYNC || "https://sync-worker-dev.vg-stavenko.workers.dev";
const SECRET = process.env.JWT_SECRET || "dev-secret-change-in-production";

let fail = 0;
const check = (n, ok, extra = "") => {
  console.log(`${ok ? "OK  " : "FAIL"} ${n}${extra ? " — " + extra : ""}`);
  if (!ok) fail++;
};

const b64url = (buf) => Buffer.from(buf).toString("base64url");
const uid = `syncpend-${Date.now()}`;
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

// Рубим ТОЛЬКО синхронизацию: приложение должно жить дальше, а очередь копиться.
let syncBlocked = true;
await page.route("**/sync/v2/**", (route) =>
  syncBlocked ? route.abort("connectionfailed") : route.continue());

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
        res(["foods", "diary", "_outbox", "app_flags"].every((s) => n.includes(s))); };
      q.onerror = () => res(false);
    });
  }, uid).catch(() => false);
  if (ok) break;
  await page.waitForTimeout(500);
}

// Локальная запись: продукт + строка дневника. Пишем ЧЕРЕЗ приложение (db::put),
// иначе она не попадёт в outbox — а он и есть предмет проверки.
await page.evaluate(async (u) => {
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
  // Без заполненной персоны приложение стоит на её экране и до дашборда — где и
  // живёт треугольник — не доходит.
  await new Promise((res, rej) => {
    const tx = db.transaction(["profile"], "readwrite");
    tx.objectStore("profile").put({ key: "profile", sex: "male", height_cm: 180,
      birth_year: 1985, goal: "lose", steps_planka: 9000,
      created_at: nowIso, updated_at: nowIso });
    tx.oncomplete = () => res(); tx.onerror = () => rej(tx.error);
  });
  db.close();
}, uid);
await page.goto(BASE, { waitUntil: "domcontentloaded" });
await page.waitForTimeout(8000);

const marker = `тест-очереди-${Date.now()}`;
await page.evaluate(async ({ u, marker }) => {
  // Пишем в outbox ровно так, как это делает db::put синкаемого стора.
  const db = await new Promise((res, rej) => {
    const q = indexedDB.open(`hjkl-ft-${u}`);
    q.onsuccess = () => res(q.result); q.onerror = () => rej(q.error);
  });
  const nowIso = new Date().toISOString();
  const food = { id: "pend-food", name: marker, kcal: 100, protein: 5, fat: 5, carbs: 5,
    nutrients: {}, package_weight: null, is_recipe: false, recipe_id: null, archived: false,
    is_restaurant: false, is_veg_fruit: null, is_heme: null, is_milk_globule: null,
    iron_mg: null, iron_absorption: null, fat_profile: null, balance_fat_profile: null,
    created_at: nowIso, updated_at: nowIso };
  await new Promise((res, rej) => {
    const tx = db.transaction(["foods", "_outbox"], "readwrite");
    tx.objectStore("foods").put(food);
    tx.objectStore("_outbox").put({ seq: "0000000000000001", store: "foods", op: "upsert",
      id: "pend-food", ts: Date.now() });
    tx.oncomplete = () => res(); tx.onerror = () => rej(tx.error);
  });
  db.close();
}, { u: uid, marker });

// Перезапуск: счётчик считается на старте, и предупреждение должно появиться
// даже без единой попытки синка.
await page.goto(BASE, { waitUntil: "domcontentloaded" });
await page.waitForTimeout(9000);
await page.locator('[data-testid="dash-errors-widget"]').click({ timeout: 15000 });
await page.waitForTimeout(1500);

const pendingVisible = await page.locator('[data-testid="sync-pending"]').count();
check("предупреждение о незавершённой синхронизации показано", pendingVisible > 0);
const text = await page.locator('[data-testid="sync-pending"]').innerText().catch(() => "");
check("текст про потерю данных на месте", text.includes("Вы можете потерять данные"), text.slice(0, 90));
// Снимок ДО нажатия: после успешной отправки блок исчезает, и показывать было бы нечего.
await page.screenshot({ path: "/tmp/sync-pending.png", fullPage: true });

// Возвращаем сеть и жмём кнопку.
syncBlocked = false;
await page.locator('[data-testid="sync-pending-retry"]').click();
await page.waitForTimeout(12000);

const stillThere = await page.locator('[data-testid="sync-pending"]').count();
check("после успешной отправки предупреждение исчезло", stillThere === 0);

const left = await page.evaluate(async (u) => {
  const db = await new Promise((res, rej) => {
    const q = indexedDB.open(`hjkl-ft-${u}`);
    q.onsuccess = () => res(q.result); q.onerror = () => rej(q.error);
  });
  const n = await new Promise((res) => {
    const rq = db.transaction(["_outbox"]).objectStore("_outbox").count();
    rq.onsuccess = () => res(rq.result); rq.onerror = () => res(-1);
  });
  db.close();
  return n;
}, uid);
check("очередь пуста", left === 0, `в очереди ${left}`);

// Главное: запись доехала до СЕРВЕРА. Спрашиваем его напрямую, с нуля.
const pull = await fetch(`${SYNC}/sync/v2/pull`, {
  method: "POST",
  headers: { "Content-Type": "application/json", Authorization: `Bearer ${token}` },
  body: JSON.stringify({ since_version: 0 }),
});
const body = await pull.text();
check("сервер знает запись", body.includes(marker), `${pull.status}, ${body.length} байт`);

await page.screenshot({ path: "/tmp/sync-done.png", fullPage: true });
await ctx.close();
await b.close();
console.log(fail ? `\n${fail} провалов` : "\nвсё сошлось");
process.exit(fail ? 1 : 0);
