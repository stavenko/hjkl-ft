// Одноразовая инвалидация кальция и железа.
//
// Значения, набранные испорченными промптами, обязаны быть стёрты, а фоновый
// проход — запросить их заново. Сеем продукт с ЗАВЕДОМО испорченными числами
// (железо равно доле усвоения — ровно тот дефект) и смотрим: значения меняются,
// флаг выставлен, а повторный запуск ничего заново не стирает.
import { chromium } from "playwright";
import { openSeeded, DEFAULT_URL } from "./harness.mjs";

const BASE = process.env.FE || DEFAULT_URL;
const PAY = process.env.PAY || "https://payment-worker-dev.vg-stavenko.workers.dev";
const SECRET = process.env.JWT_SECRET || "dev-secret-change-in-production";
const WAIT_MS = Number(process.env.WAIT_MS || 180000);

let fail = 0;
const check = (n, ok, extra = "") => { console.log(`${ok ? "OK " : "FAIL"} ${n}${extra ? " — " + extra : ""}`); if (!ok) fail++; };

const b64url = (buf) => Buffer.from(buf).toString("base64url");
const uid = `reset-${Date.now()}`;
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

const seed = async (page, u) => {
  await page.evaluate(async (u) => {
    const db = await new Promise((res, rej) => {
      const q = indexedDB.open(`hjkl-ft-${u}`);
      q.onsuccess = () => res(q.result); q.onerror = () => rej(q.error);
    });
    const nowIso = new Date().toISOString();
    const ymd = (o) => { const d = new Date(); d.setDate(d.getDate() - o);
      return `${d.getFullYear()}-${String(d.getMonth() + 1).padStart(2, "0")}-${String(d.getDate()).padStart(2, "0")}`; };
    const app_flags = [
      { key: "push_onboarding_dismissed", value: "true" }, { key: "welcome_shown", value: "true" },
      { key: "iron_week_unlocked", value: "true" }, { key: "iron_week_opened_at", value: ymd(2) },
    ];
    const profile = [{ key: "profile", sex: "male", height_cm: 180, birth_year: 1985,
      goal: "lose", steps_planka: 9000, created_at: nowIso, updated_at: nowIso }];
    const bare = { kcal: 137, protein: 20, fat: 6, carbs: 1, package_weight: null, is_recipe: false,
      recipe_id: null, archived: false, is_restaurant: false, is_snack: false, is_liquid_cal: false,
      is_veg_fruit: false, is_egg: false, is_red_meat: true, created_at: nowIso, updated_at: nowIso };
    const foods = [
      // Испорченные числа ровно того вида, что писал прежний промпт: железо
      // равно доле усвоения, кальций взят с потолка.
      { ...bare, id: "spoiled", name: "Куриная печень",
        nutrients: { "Кальций": 999, "Железо": 0.25, "Клетчатка": 0, "Омега-3": 0 },
        iron_mg: 0.25, iron_absorption: 0.25 },
    ];
    const diary = [{ id: "d1", food_id: "spoiled", date: ymd(1), time: null, grams: 100,
      waste_grams: 0, meal_label: "lunch", deleted: false, created_at: nowIso, updated_at: nowIso }];
    for (const [store, rows] of Object.entries({ app_flags, profile, foods, diary })) {
      await new Promise((res, rej) => {
        const tx = db.transaction([store], "readwrite");
        for (const r of rows) tx.objectStore(store).put(r);
        tx.oncomplete = () => res(); tx.onerror = () => rej(tx.error);
      });
    }
    db.close();
  }, u);
};

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
        res(["foods", "app_flags", "profile", "diary"].every((s) => n.includes(s))); };
      q.onerror = () => res(false);
    });
  }, uid).catch(() => false);
  if (ok) break;
  await page.waitForTimeout(500);
}
await seed(page, uid);

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
  const f = foods.find((x) => x.id === "spoiled");
  const n = f?.nutrients instanceof Map ? Object.fromEntries(f.nutrients) : (f?.nutrients || {});
  return { ca: n["Кальций"], legacyFe: n["Железо"], mg: f?.iron_mg, abs: f?.iron_absorption,
    // Отметка о выполнении теперь одна на все миграции — версия базы.
    flag: flags.find((x) => x.key === "db_schema_version")?.value };
}, uid);

await page.goto(BASE, { waitUntil: "domcontentloaded" });
await page.waitForSelector("#splash", { state: "detached", timeout: 20000 }).catch(() => {});

// Ждём, пока проход заново заполнит стёртое. Ждать «значение не null» нельзя:
// испорченные значения тоже не null, и цикл вышел бы мгновенно, ничего не измерив.
// Ждём ИМЕННО замены — флаг миграции выставлен И оба числа отличаются от посеянных.
let st = {};
const t0 = Date.now();
while (Date.now() - t0 < WAIT_MS) {
  st = await read();
  if (/^[1-9][0-9]*$/.test(String(st.flag)) && st.ca != null && st.ca !== 999 && st.mg != null && st.mg !== 0.25) break;
  await page.waitForTimeout(3000);
}
console.log(`после миграции: кальций ${st.ca}, железо ${st.mg} мг, усвоение ${st.abs}, флаг ${st.flag}`);

check("версия базы поднята", /^[1-9][0-9]*$/.test(String(st.flag)), String(st.flag));
check("испорченный кальций 999 заменён", st.ca !== 999, `${st.ca}`);
check("legacy-ключ «Железо» убран из карты", st.legacyFe === undefined, String(st.legacyFe));
check("испорченное железо 0.25 заменено", st.mg !== 0.25 && st.mg != null, `${st.mg} мг`);
check("усвоение взято из таблицы (печень = 0.25)", st.abs === 0.25, String(st.abs));

// Повторный запуск не должен стирать заново — иначе значения не удержатся никогда.
const before = { ca: st.ca, mg: st.mg };
await page.goto(BASE, { waitUntil: "domcontentloaded" });
await page.waitForSelector("#splash", { state: "detached", timeout: 20000 }).catch(() => {});
await page.waitForTimeout(8000);
const again = await read();
check("повторный запуск ничего не стёр", again.ca === before.ca && again.mg === before.mg,
  `${before.ca}/${before.mg} → ${again.ca}/${again.mg}`);

console.log(fail === 0 ? "\n=== ALL OK ===" : `\n=== FAILURES: ${fail} ===`);
await b.close();
process.exit(fail === 0 ? 0 : 1);
