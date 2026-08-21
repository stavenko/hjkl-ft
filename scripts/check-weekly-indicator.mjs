// НЕДЕЛЬНЫЙ ИНДИКАТОР: цвет по двум последним неделям + история восьми недель.
//
// Сеется восемь завершённых недель яиц: нормой считается 7 яиц за неделю. Сценарий
// задаётся строкой из восьми символов, старая неделя первая:
//   "+" — неделя закрыта, "-" — нет.
//
//   node scripts/check-weekly-indicator.mjs "++++++--"   → ждём красный
//   node scripts/check-weekly-indicator.mjs "------+-"   → оранжевый
//   node scripts/check-weekly-indicator.mjs "------ -+"  → зелёный
import { chromium } from "playwright";

const BASE = process.env.FE || "https://renorma-fit-dev.pages.dev";
const PLAN = (process.argv[2] || "++++++--").replace(/\s/g, "");
const SHOT = process.argv[3] || "weekly-indicator.png";
const uid = `weekly-${Date.now()}`;

const browser = await chromium.launch({ headless: true });
const ctx = await browser.newContext({ viewport: { width: 430, height: 932 }, deviceScaleFactor: 2, serviceWorkers: "block" });
const page = await ctx.newPage();
page.on("pageerror", (e) => console.log("  [ошибка страницы]", String(e).slice(0, 200)));
page.on("console", (m) => {
  if (m.type() === "error") console.log("  [консоль]", m.text().slice(0, 200));
});

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
        res(["foods", "diary", "profile"].every((s) => n.includes(s))); };
      q.onerror = () => res(false);
    });
  }, uid).catch(() => false);
  if (ok) break;
  await page.waitForTimeout(500);
}

await page.evaluate(async ({ u, plan }) => {
  const db = await new Promise((res, rej) => {
    const q = indexedDB.open(`hjkl-ft-${u}`);
    q.onsuccess = () => res(q.result); q.onerror = () => rej(q.error);
  });
  const nowIso = new Date().toISOString();
  const iso = (d) => d.toISOString().slice(0, 10);

  await new Promise((res) => {
    const tx = db.transaction(["app_flags"], "readwrite");
    const os = tx.objectStore("app_flags");
    [["db_schema_version", "999"], ["welcome_shown", "true"], ["push_onboarding_dismissed", "true"],
     ["egg_week_unlocked", "true"], ["paywall_skipped_date", iso(new Date())],
     // Недели яиц режутся ОТ ДНЯ ОТКРЫТИЯ, а не пн–вс: без этой даты история пуста
     // и индикатор серый. Ставим её на девять недель назад — восемь завершённых
     // недель и текущая.
     ["egg_week_opened_at", (() => { const d = new Date(); d.setDate(d.getDate() - 63); return d.toISOString().slice(0, 10); })()],
     ["ft_subscription", JSON.stringify({ plan: "monthly", end: Date.now() + 30 * 864e5, active: true,
       start: Date.now(), status: "paid", no_renew: false, provider: "lava" })]]
      .forEach(([key, value]) => os.put({ key, value }));
    tx.oncomplete = res;
  });
  await new Promise((res) => {
    const tx = db.transaction(["profile"], "readwrite");
    tx.objectStore("profile").put({ key: "profile", sex: "male", height_cm: 180, birth_year: 1990,
      goal: "lose", cycle_start: null, steps_planka: 8000, updated_at: nowIso });
    tx.oncomplete = res;
  });
  // Одно яйцо — 60 г, признак яйца стоит: неделя «закрыта» с семи штук.
  await new Promise((res) => {
    const tx = db.transaction(["foods"], "readwrite");
    tx.objectStore("foods").put({
      id: "egg1", name: "Яйцо куриное", kcal: 155, protein: 13, fat: 11, carbs: 1,
      nutrients: {}, package_weight: null, is_recipe: false, recipe_id: null, archived: false,
      is_restaurant: false, is_veg_fruit: false, is_heme: false, is_milk_globule: false,
      is_red_meat: false, is_processed_meat: false, is_egg: true,
      iron_mg: 1.2, iron_absorption: 0.2,
      fat_profile: { sfa_pct: 30, mufa_pct: 45, pufa_pct: 25, epa_dha_pct: 0 },
      balance_fat_profile: null, created_at: nowIso, updated_at: nowIso,
    });
    tx.objectStore("foods").put({
      id: "oat1", name: "Овсянка", kcal: 88, protein: 3, fat: 2, carbs: 15,
      nutrients: {}, package_weight: null, is_recipe: false, recipe_id: null, archived: false,
      is_restaurant: false, is_veg_fruit: false, is_heme: false, is_milk_globule: false,
      is_red_meat: false, is_processed_meat: false, is_egg: false,
      iron_mg: 0.5, iron_absorption: 0.05,
      fat_profile: { sfa_pct: 20, mufa_pct: 35, pufa_pct: 45, epa_dha_pct: 0 },
      balance_fat_profile: null, created_at: nowIso, updated_at: nowIso,
    });
    tx.oncomplete = res;
  });

  // Сетка недель: недели считаются пн–вс и только ЗАВЕРШЁННЫЕ.
  const today = new Date();
  const monday = new Date(today);
  monday.setDate(today.getDate() - ((today.getDay() + 6) % 7));
  const entries = [];
  plan.split("").forEach((mark, i) => {
    const back = plan.length - i; // самая старая неделя — самая дальняя
    const start = new Date(monday);
    start.setDate(monday.getDate() - 7 * back);
    // КАЖДЫЙ день недели: сетка приложения может быть сдвинута относительно нашей
    // на день (часовой пояс), и «восемь яиц в первые дни» тогда переползают в
    // соседнюю неделю. Ровный по дням сев от этого не зависит.
    //   закрытая  — два яйца в день (14 за неделю против нормы 7);
    //   незакрытая — овсянка, яиц нет вовсе; запись нужна, иначе неделя не судится.
    for (let n = 0; n < 7; n++) {
      const d = new Date(start);
      d.setDate(start.getDate() + n);
      if (mark === "+") {
        entries.push({ id: `e${i}-${n}a`, food_id: "egg1", date: iso(d), time: null, grams: 120,
          waste_grams: 0, meal_label: null, deleted: false, created_at: nowIso, updated_at: nowIso });
      } else {
        entries.push({ id: `o${i}-${n}`, food_id: "oat1", date: iso(d), time: null, grams: 200,
          waste_grams: 0, meal_label: null, deleted: false, created_at: nowIso, updated_at: nowIso });
      }
    }
  });
  // ПОСЛЕДНИЕ СЕМЬ ДНЕЙ — иначе виджета прогресса вовсе нет: он открывается после
  // недели дневника с весом и шагами (экран «Питание 0/7, Вес 0/7, Шаги 0/7»).
  // Эти дни идут в НЕЗАВЕРШЁННУЮ неделю и на историю восьми недель не влияют.
  const weights = [];
  const steps = [];
  for (let back = 0; back < 8; back++) {
    const d = new Date(today);
    d.setDate(today.getDate() - back);
    entries.push({ id: `cur-${back}`, food_id: "oat1", date: iso(d), time: null, grams: 200,
      waste_grams: 0, meal_label: null, deleted: false, created_at: nowIso, updated_at: nowIso });
    weights.push({ id: `w-${back}`, date: iso(d), weight_kg: 80 - back * 0.1, no_water: true,
      no_food: true, no_wash: true, used_toilet: true, morning: true,
      created_at: nowIso, updated_at: nowIso });
    steps.push({ id: `s-${back}`, date: iso(d), steps: 9000, created_at: nowIso, updated_at: nowIso });
  }

  await new Promise((res) => {
    const tx = db.transaction(["diary"], "readwrite");
    const os = tx.objectStore("diary");
    entries.forEach((e) => os.put(e));
    tx.oncomplete = res;
  });
  await new Promise((res) => {
    const tx = db.transaction(["weight_entries"], "readwrite");
    const os = tx.objectStore("weight_entries");
    weights.forEach((w) => os.put(w));
    tx.oncomplete = res;
  });
  await new Promise((res) => {
    const tx = db.transaction(["step_entries"], "readwrite");
    const os = tx.objectStore("step_entries");
    steps.forEach((x) => os.put(x));
    tx.oncomplete = res;
  });
  db.close();
}, { u: uid, plan: PLAN });

await page.goto(BASE, { waitUntil: "domcontentloaded" });
await page.waitForSelector("#splash", { state: "detached", timeout: 30000 }).catch(() => {});
await page.waitForTimeout(6000);

const tint = await page.evaluate(() => {
  const el = document.querySelector('[data-ind="Яйца"] div');
  return el ? getComputedStyle(el).backgroundColor : null;
});
const colour = tint === null ? "значка нет"
  : /224, *48/.test(tint) ? "КРАСНЫЙ"
  : /245, *166|240, *160|255, *(1[0-9][0-9])/.test(tint) ? "ОРАНЖЕВЫЙ"
  : /1?[0-9]?[0-9], *(1[6-9][0-9]|2[0-9][0-9])/.test(tint) ? "ЗЕЛЁНЫЙ"
  : tint;

// Подробности открываются ТАПОМ по виджету: pointerdown + pointerup на месте
// (обычный click виджет не ловит — см. dashboard.rs, TAP_SLOP).
const widget = page.locator('[data-ind="Яйца"]').first();
const box = await widget.boundingBox();
if (box) {
  const x = box.x + box.width / 2, y = box.y + box.height / 2;
  await page.mouse.move(x, y);
  await page.mouse.down();
  await page.mouse.up();
}
await page.waitForTimeout(2500);
const panel = page.locator('[data-ind-panel="egg"]');
const shown = await panel.count();
if (shown) {
  await panel.scrollIntoViewIfNeeded();
  await page.waitForTimeout(400);
  // Нижнее меню перекрывает панель, если она у самого низа — снимаем страницу
  // целиком, там видно и значок в ряду, и восемь клеток истории.
  await page.screenshot({ path: SHOT, fullPage: true });
} else {
  await page.screenshot({ path: SHOT, fullPage: true });
}
console.log(`сценарий ${PLAN} · панель яиц: ${shown ? "есть" : "НЕ НАЙДЕНА"} · цвет: ${colour} (${tint})`);
console.log(`снимок: ${SHOT}`);
await browser.close();
