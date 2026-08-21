// МИГРАЦИЯ 23 — сброс профиля жира: проверка на живом приложении.
//
// Сеются два продукта с профилем и одно блюдо, база помечается версией 22 —
// миграция 23 обязана: у продуктов профиль стереть, у блюда оставить (его профиль
// считается из состава и пересчитается сам).
import { chromium } from "playwright";

const BASE = process.env.FE || "https://renorma-fit-dev.pages.dev";
const uid = `fatreset-${Date.now()}`;

const browser = await chromium.launch({ headless: true });
const ctx = await browser.newContext({ viewport: { width: 430, height: 932 }, serviceWorkers: "block" });
const page = await ctx.newPage();

await page.goto(BASE, { waitUntil: "domcontentloaded" });
await page.evaluate((u) => {
  localStorage.clear();
  localStorage.setItem("user_id", u);
  localStorage.setItem("auth_token", "x");
  localStorage.setItem("token_id", "t");
  localStorage.setItem("auth_ctx", "browser");
  localStorage.setItem("pwa_dismissed", "true");
}, uid);
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

// Версия базы — 22: миграция 23 ещё не проходила.
await page.evaluate(async (u) => {
  const db = await new Promise((res, rej) => {
    const q = indexedDB.open(`hjkl-ft-${u}`);
    q.onsuccess = () => res(q.result); q.onerror = () => rej(q.error);
  });
  const nowIso = new Date().toISOString();
  const profile = { sfa_pct: 25, mufa_pct: 35, pufa_pct: 35, epa_dha_pct: 5 };
  const base = {
    kcal: 100, protein: 5, fat: 10, carbs: 5, nutrients: {}, package_weight: null,
    recipe_id: null, archived: false, is_restaurant: false,
    is_veg_fruit: false, is_heme: false, is_milk_globule: false,
    is_red_meat: false, is_processed_meat: false, is_egg: false,
    iron_mg: 1, iron_absorption: 0.1, created_at: nowIso, updated_at: nowIso,
  };
  await new Promise((res) => {
    const tx = db.transaction(["app_flags"], "readwrite");
    [{ key: "db_schema_version", value: "22" }, { key: "welcome_shown", value: "true" },
     { key: "push_onboarding_dismissed", value: "true" },
     { key: "paywall_skipped_date", value: nowIso.slice(0, 10) },
     { key: "ft_subscription", value: JSON.stringify({ plan: "monthly", end: Date.now() + 30 * 864e5,
       active: true, start: Date.now(), status: "paid", no_renew: false, provider: "lava" }) }].forEach((f) => tx.objectStore("app_flags").put(f));
    tx.oncomplete = res;
  });
  // Без профиля приложение показывает онбординг и до Ready не доходит, а миграции
  // запускаются только там.
  await new Promise((res) => {
    const tx = db.transaction(["profile"], "readwrite");
    tx.objectStore("profile").put({ key: "profile", sex: "male", height_cm: 180,
      birth_year: 1990, goal: "lose", cycle_start: null, steps_planka: 8000, updated_at: nowIso });
    tx.oncomplete = res;
  });
  await new Promise((res) => {
    const tx = db.transaction(["foods"], "readwrite");
    const os = tx.objectStore("foods");
    os.put({ ...base, id: "p1", name: "Сыр", is_recipe: false, fat_profile: profile, balance_fat_profile: null });
    os.put({ ...base, id: "p2", name: "Масло", is_recipe: false, fat_profile: profile, balance_fat_profile: null });
    os.put({ ...base, id: "d1", name: "Салат", is_recipe: true, fat_profile: profile, balance_fat_profile: profile });
    tx.oncomplete = res;
  });
  db.close();
}, uid);

page.on("console", (m) => {
  const t = m.text();
  if (/миграци|db_schema|Ready|splash/i.test(t)) console.log("  [консоль]", t.slice(0, 140));
});
await page.goto(BASE, { waitUntil: "domcontentloaded" });
await page.waitForSelector("#splash", { state: "detached", timeout: 30000 }).catch(() => console.log("  splash не ушёл"));
await page.waitForTimeout(20000);
console.log("  на экране:", (await page.evaluate(() => document.body.innerText)).replace(/\n+/g, " | ").slice(0, 160));

const out = await page.evaluate(async (u) => {
  const db = await new Promise((res, rej) => {
    const q = indexedDB.open(`hjkl-ft-${u}`);
    q.onsuccess = () => res(q.result); q.onerror = () => rej(q.error);
  });
  const all = await new Promise((res) => {
    const rq = db.transaction(["foods"]).objectStore("foods").getAll();
    rq.onsuccess = () => res(rq.result); rq.onerror = () => res([]);
  });
  const ver = await new Promise((res) => {
    const rq = db.transaction(["app_flags"]).objectStore("app_flags").get("db_schema_version");
    rq.onsuccess = () => res(rq.result?.value); rq.onerror = () => res(null);
  });
  db.close();
  return { ver, foods: all.map((f) => ({ name: f.name, recipe: f.is_recipe,
    fat: f.fat_profile ? "есть" : "СБРОШЕН", bal: f.balance_fat_profile ? "есть" : "нет" })) };
}, uid);

console.log(`версия базы после запуска: ${out.ver}`);
for (const f of out.foods) {
  console.log(`  ${f.name.padEnd(8)} ${f.recipe ? "блюдо " : "продукт"}  профиль: ${f.fat.padEnd(8)} балансовый: ${f.bal}`);
}
await browser.close();
