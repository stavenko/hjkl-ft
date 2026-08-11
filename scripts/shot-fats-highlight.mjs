// Мигающая подсветка для кадров истории про ЖИРЫ. Тот же приём, что у железа и
// гема: сеем состояние, снимаем виджет и обводим нужную шкалу с её индикатором.
//
// Две гифки за один прогон, по кадру истории на каждую:
//   fats-omega-highlight.gif — шкала «Омега-3/нед» и индикатор «Омега-3»;
//   fats-ratio-highlight.gif — шкала «Баланс жира» и индикатор «Баланс».
//
// Состояние подобрано так, чтобы кадр объяснял, к чему стремиться: омега-3 закрыта
// (человек ел рыбу), а соотношение НЕ закрыто — сала и сыра съедено столько, что
// баланс просел. Ровно тот разговор, который ведёт история.
import { chromium } from "playwright";
import { openSeeded, DEFAULT_URL } from "./harness.mjs";
import { makeWidgetGif } from "./highlight-gif.mjs";
import path from "node:path";

const BASE = process.env.FE || DEFAULT_URL;
const ROOT = path.resolve(import.meta.dirname, "..");
const OUT_OMEGA = process.env.OUT_OMEGA || path.join(ROOT, "frontend/story-img/fats-omega-highlight.gif");
const OUT_RATIO = process.env.OUT_RATIO || path.join(ROOT, "frontend/story-img/fats-ratio-highlight.gif");

const seed = async (page, uid) => {
  await page.evaluate(async ({ uid }) => {
    const db = await new Promise((res, rej) => {
      const r = indexedDB.open(`hjkl-ft-${uid}`);
      r.onsuccess = () => res(r.result); r.onerror = () => rej(r.error);
    });
    const nowIso = new Date().toISOString();
    const ymd = (back) => {
      const d = new Date(); d.setDate(d.getDate() - back);
      return `${d.getFullYear()}-${String(d.getMonth() + 1).padStart(2, "0")}-${String(d.getDate()).padStart(2, "0")}`;
    };
    const app_flags = [
      { key: "db_schema_version", value: "999" },
      { key: "push_onboarding_dismissed", value: "true" },
      { key: "welcome_shown", value: "true" },
      { key: "activity_week_unlocked", value: "true" },
      { key: "steps_gate_opened_at", value: ymd(30) },
      { key: "calcium_week_unlocked", value: "true" },
      { key: "calcium_gate_opened_at", value: ymd(20) },
      { key: "ind_opened_at", value: ymd(40) },
      { key: "iron_week_unlocked", value: "true" },
      { key: "iron_week_opened_at", value: ymd(10) },
      { key: "fat_week_unlocked", value: "true" },
      { key: "fat_week_opened_at", value: ymd(3) }, // сегодня 4-й день недели жира
      { key: "ft_subscription", value: JSON.stringify({
          plan: "monthly", end: Date.now() + 30 * 864e5, active: true,
          start: Date.now(), status: "paid", no_renew: false, provider: "lava" }) },
    ];
    const profile = [{ key: "profile", sex: "male", height_cm: 180,
      birth_year: new Date().getFullYear() - 45, goal: "lose", steps_planka: 9000,
      created_at: nowIso, updated_at: nowIso }];
    const goals = [
      { id: "g-cal", nutrient: "Calories", key: "calories", direction: "AtMost",
        amount: 2600, unit: "Kcal", period: "Day", created_at: nowIso, updated_at: nowIso },
      { id: "g-ca", nutrient: "Кальций", key: "calcium", direction: "AtLeast",
        amount: 1000, unit: "Mg", period: "Day", created_at: nowIso, updated_at: nowIso },
    ];
    const food = (id, name, kcal, protein, fat, fp, extra) => ({
      id, name, kcal, protein, fat, carbs: 0, nutrients: { "Кальций": 200 },
      package_weight: null, is_recipe: false, recipe_id: null, archived: false,
      is_restaurant: false, is_snack: false, is_liquid_cal: false, is_veg_fruit: false,
      is_egg: false, is_red_meat: false, is_heme: false,
      iron_mg: 1.0, iron_absorption: 0.15, fat_profile: fp,
      created_at: nowIso, updated_at: nowIso, ...extra,
    });
    const foods = [
      food("mack", "Скумбрия", 191, 18, 13.9,
        { sfa_pct: 24, mufa_pct: 38, pufa_pct: 26, epa_dha_pct: 18 }, { is_heme: true }),
      food("veg", "Овощи", 30, 1, 0.2,
        { sfa_pct: 0, mufa_pct: 0, pufa_pct: 0, epa_dha_pct: 0 }, { is_veg_fruit: true }),
      // Сало и сыр — тот самый перекос, о котором говорит история.
      food("lard", "Сало", 797, 2, 89,
        { sfa_pct: 38, mufa_pct: 45, pufa_pct: 11, epa_dha_pct: 0 }, {}),
      food("cheese", "Сыр", 356, 25, 28,
        { sfa_pct: 65, mufa_pct: 26, pufa_pct: 4, epa_dha_pct: 0 }, {}),
    ];
    const diary = [], weight_entries = [], step_entries = [];
    for (let i = 0; i < 12; i++) {
      weight_entries.push({ id: "w" + i, date: ymd(i), weight_kg: 88 - i * 0.05,
        no_water: false, no_food: false, no_wash: false, used_toilet: false, morning: true,
        created_at: nowIso, updated_at: nowIso });
      step_entries.push({ id: "s" + i, date: ymd(i), steps: 11000, created_at: nowIso, updated_at: nowIso });
      diary.push({ id: "dv" + i, food_id: "veg", date: ymd(i), time: null, grams: 850,
        waste_grams: 0, meal_label: "lunch", deleted: false, created_at: nowIso, updated_at: nowIso });
      // Насыщенного — каждый день; рыбы — один раз за неделю жира.
      diary.push({ id: "dl" + i, food_id: "lard", date: ymd(i), time: null, grams: 60,
        waste_grams: 0, meal_label: "lunch", deleted: false, created_at: nowIso, updated_at: nowIso });
      diary.push({ id: "dc" + i, food_id: "cheese", date: ymd(i), time: null, grams: 80,
        waste_grams: 0, meal_label: "breakfast", deleted: false, created_at: nowIso, updated_at: nowIso });
      if (i === 1) {
        diary.push({ id: "dm" + i, food_id: "mack", date: ymd(i), time: null, grams: 200,
          waste_grams: 0, meal_label: "dinner", deleted: false, created_at: nowIso, updated_at: nowIso });
      }
    }
    const avail = Array.from(db.objectStoreNames);
    for (const [store, rows] of Object.entries({ app_flags, profile, goals, foods, diary,
      weight_entries, step_entries })) {
      if (!avail.includes(store)) continue;
      await new Promise((res, rej) => {
        const tx = db.transaction([store], "readwrite");
        for (const row of rows) tx.objectStore(store).put(row);
        tx.oncomplete = () => res(); tx.onerror = () => rej(tx.error);
      });
    }
    db.close();
  }, { uid });
};

const b = await chromium.launch({ headless: true });
const { context, page } = await openSeeded(b, {
  baseUrl: BASE,
  context: { serviceWorkers: "block", deviceScaleFactor: 2 },
  uid: `fats-hl-${Math.floor(Math.random() * 1e6)}`,
  seed,
});
await page.reload({ waitUntil: "domcontentloaded" });
await page.waitForTimeout(4000);
await page.setViewportSize({ width: 440, height: 1200 });
await page.waitForTimeout(3000); // дать полосам доиграть анимацию заполнения

console.log("шкалы:", await page.$$eval("[data-gauge]", (els) => els.map((e) => e.getAttribute("data-gauge"))));
for (const g of ["Омега-3/нед", "Баланс жира"]) {
  console.log(` ${g}:`, (await page.locator(`[data-gauge="${g}"]`).innerText()).replace(/\s+/g, " "));
}
await page.locator('[data-testid="progress-widget"]').screenshot({
  path: process.env.SHOT || "/tmp/fats-highlight-widget.png",
});

// Полосой, а не всей карточкой: под этими кадрами длинный текст, и высокая
// карточка либо съёживается до нечитаемой, либо оказывается под ним. Полоса
// вокруг самих подсветок ставит их в середину свободного места (см. makeWidgetGif).
await makeWidgetGif(page, ['[data-ind="Омега-3"] > div', '[data-gauge="Омега-3/нед"]'], OUT_OMEGA,
  { band: true });
await makeWidgetGif(page, ['[data-ind="Баланс"] > div', '[data-gauge="Баланс жира"]'], OUT_RATIO,
  { band: true });

await context.close();
await b.close();
