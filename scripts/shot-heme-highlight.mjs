// Мигающая подсветка для кадра про ГЕМОВОЕ железо: недельная шкала порций и
// иконка индикатора «Гем» в ряду индикаторов. Тот же приём, что у железа.
//
// Состояние сеем так, чтобы неделя ШЛА и цель была близка, но не закрыта: 4-й
// день, съедено две порции с небольшим — шкала показывает дробное «2,1 / 3».
// Дробность тут намеренная: она объясняет, что порция считается по белку, а не
// по числу приёмов.
import { chromium } from "playwright";
import { openSeeded, DEFAULT_URL } from "./harness.mjs";
import { makeWidgetGif } from "./highlight-gif.mjs";
import path from "node:path";

const BASE = process.env.FE || DEFAULT_URL;
const ROOT = path.resolve(import.meta.dirname, "..");
const OUT = process.env.OUT || path.join(ROOT, "frontend/story-img/heme-highlight.gif");

// Печень: 20 г белка на 100 г. Порция — 25 г белка, значит 125 г печени = 1 порция.
// Кладём её в дневник ДВА раза за неделю по 130 г: 2 × 26 / 25 = 2,08 порции из
// трёх. Шкала горит дробным «2,08», цель ещё не закрыта.
const GRAMS = 130;

const seed = async (page, uid) => {
  await page.evaluate(async ({ uid, GRAMS }) => {
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
      // Иначе миграция m001 сотрёт засеянные кальций и железо (она стирает всё,
      // набранное испорченными промптами), и шкалы покажут ноль.
      { key: "db_schema_version", value: "999" },
      { key: "push_onboarding_dismissed", value: "true" },
      { key: "welcome_shown", value: "true" },
      { key: "activity_week_unlocked", value: "true" },
      { key: "steps_gate_opened_at", value: ymd(30) },
      { key: "calcium_week_unlocked", value: "true" },
      { key: "calcium_gate_opened_at", value: ymd(9) },
      { key: "ind_opened_at", value: ymd(40) },
      { key: "iron_week_unlocked", value: "true" },
      { key: "iron_week_opened_at", value: ymd(3) }, // сегодня 4-й день недели
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
    // Три продукта: белок, овощи (чтобы остальные индикаторы были зелёными) и
    // печень — носитель железа.
    const food = (id, name, extra) => ({
      id, name, kcal: 120, protein: 0, fat: 0, carbs: 0, nutrients: {},
      package_weight: null, is_recipe: false, recipe_id: null, archived: false,
      is_restaurant: false, is_snack: false, is_liquid_cal: false, is_veg_fruit: false,
      is_egg: false, is_red_meat: false, is_heme: false, iron_mg: null, iron_absorption: null,
      created_at: nowIso, updated_at: nowIso, ...extra,
    });
    const foods = [
      food("prot", "Белок", { protein: 160 }),
      food("veg", "Овощи", { is_veg_fruit: true }),
      food("liver", "Куриная печень", {
        protein: 20, nutrients: { "Кальций": 1200 },
        is_red_meat: true, is_heme: true, iron_mg: 9.0, iron_absorption: 0.25 }),
    ];
    const weight_entries = [], step_entries = [], diary = [];
    for (let i = 0; i < 10; i++) {
      weight_entries.push({ id: "w" + i, date: ymd(i), weight_kg: 88 - i * 0.05,
        no_water: false, no_food: false, no_wash: false, used_toilet: false,
        morning: true, created_at: nowIso, updated_at: nowIso });
      step_entries.push({ id: "s" + i, date: ymd(i), steps: 11000, created_at: nowIso, updated_at: nowIso });
      diary.push({ id: "dp" + i, food_id: "prot", date: ymd(i), time: null, grams: 100,
        waste_grams: 0, meal_label: "lunch", deleted: false, created_at: nowIso, updated_at: nowIso });
      diary.push({ id: "dv" + i, food_id: "veg", date: ymd(i), time: null, grams: 850,
        waste_grams: 0, meal_label: "lunch", deleted: false, created_at: nowIso, updated_at: nowIso });
      // Печень — только в двух днях текущей недели: цель в 3 порции должна быть
      // видимо НЕ закрыта, иначе кадр не объясняет, к чему стремиться.
      if (i === 0 || i === 2) {
        diary.push({ id: "dl" + i, food_id: "liver", date: ymd(i), time: null, grams: GRAMS,
          waste_grams: 0, meal_label: "lunch", deleted: false, created_at: nowIso, updated_at: nowIso });
      }
    }
    const rec = { app_flags, profile, goals, foods, weight_entries, step_entries, diary };
    const avail = Array.from(db.objectStoreNames);
    for (const [store, rows] of Object.entries(rec)) {
      if (!rows.length || !avail.includes(store)) continue;
      await new Promise((res, rej) => {
        const tx = db.transaction([store], "readwrite");
        const os = tx.objectStore(store);
        for (const row of rows) os.put(row);
        tx.oncomplete = () => res(); tx.onerror = () => rej(tx.error);
      });
    }
    db.close();
  }, { uid, GRAMS });
};

const b = await chromium.launch({ headless: true });
const { context, page } = await openSeeded(b, {
  baseUrl: BASE,
  context: { serviceWorkers: "block", deviceScaleFactor: 2 },
  uid: `heme-hl-${Math.floor(Math.random() * 1e6)}`,
  seed,
});
await page.reload({ waitUntil: "domcontentloaded" });
await page.waitForTimeout(4000);
await page.setViewportSize({ width: 440, height: 1100 });
await page.waitForTimeout(3000); // дать полосам доиграть анимацию заполнения

console.log("шкалы на экране:", await page.$$eval('[data-gauge]', els => els.map(e => e.getAttribute('data-gauge'))));
console.log("индикаторы:", await page.$$eval('[data-ind]', els => els.map(e => e.getAttribute('data-ind'))));
const gauge = page.locator('[data-gauge="Гем/нед"]');
console.log("gauge:", (await gauge.innerText()).replace(/\s+/g, " "));
console.log("точек горит:", await page.locator('[data-pace-dot="lit"]').count(),
            "погашено:", await page.locator('[data-pace-dot="dim"]').count());

// Снимок виджета — для сверки состояния перед сборкой гифки.
await page.locator('[data-testid="progress-widget"]').screenshot({
  path: process.env.SHOT || "/tmp/heme-highlight-widget.png",
});

await makeWidgetGif(page, ['[data-ind="Гем"] > div', '[data-gauge="Гем/нед"]'], OUT);

await context.close();
await b.close();
