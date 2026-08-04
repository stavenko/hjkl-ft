// Как выглядит железо, когда человек ОПАЗДЫВАЕТ: середина недели, а железа не
// съедено. Два варианта, которые снаружи выглядят одинаково, но означают разное:
//   nofood  — продукты в дневнике ещё НЕ обработаны железным проходом (iron_mg
//             не заполнен) → мы про них ничего не знаем;
//   lowiron — продукты обработаны, железа в них почти нет → это настоящий провал.
import { chromium } from "playwright";
import { openSeeded, DEFAULT_URL } from "./harness.mjs";

const BASE = process.env.FE || DEFAULT_URL;
const OUT = process.env.OUT ||
  "/private/tmp/claude-501/-Users-vasilijstavenko-projects-hjkl-ft/56df53af-a1ed-4117-8e82-8a1f8aad90e8/scratchpad/iron";

// `iron` — какие железные поля проставить продукту в дневнике.
function seed({ ironOpenDaysAgo, iron }) {
  return async (page, uid) => {
    await page.evaluate(async ({ uid, ironOpenDaysAgo, iron }) => {
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
        { key: "push_onboarding_dismissed", value: "true" },
        { key: "welcome_shown", value: "true" },
        { key: "activity_week_unlocked", value: "true" },
        { key: "steps_gate_opened_at", value: ymd(30) },
        { key: "calcium_week_unlocked", value: "true" },
        { key: "calcium_gate_opened_at", value: ymd(9) },
        { key: "ind_opened_at", value: ymd(40) },
        { key: "iron_week_unlocked", value: "true" },
        { key: "iron_week_opened_at", value: ymd(ironOpenDaysAgo) },
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
      const base = {
        kcal: 120, protein: 0, fat: 0, carbs: 0, nutrients: {}, package_weight: null,
        is_recipe: false, recipe_id: null, archived: false, is_restaurant: false,
        is_snack: false, is_liquid_cal: false, is_veg_fruit: false, is_egg: false,
        is_red_meat: false, iron_mg: null, iron_absorption: null,
        created_at: nowIso, updated_at: nowIso,
      };
      const foods = [
        { ...base, id: "prot", name: "Белок", protein: 160, ...iron },
        { ...base, id: "veg", name: "Овощи", is_veg_fruit: true, nutrients: { "Кальций": 1400 }, ...iron },
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
    }, { uid, ironOpenDaysAgo, iron });
  };
}

const b = await chromium.launch({ headless: true });

async function shoot(name, opts) {
  const { context, page } = await openSeeded(b, {
    baseUrl: BASE, context: { serviceWorkers: "block", deviceScaleFactor: 2 },
    uid: `iron-behind-${name}-${Math.floor(Math.random() * 1e6)}`,
    seed: seed(opts),
  });
  await page.setViewportSize({ width: 440, height: 1100 });
  await page.waitForTimeout(6000);
  const gauge = page.locator('[data-gauge="Железо/нед"]');
  const text = (await gauge.innerText()).replace(/\s+/g, " ");
  const lit = await page.locator('[data-pace-dot="lit"]').count();
  const shadow = await gauge.locator("div").nth(1).evaluate((el) => getComputedStyle(el).boxShadow);
  // Цвет кружка под иконкой железа = состояние индикатора.
  const ind = await page.locator('[data-ind="Железо"] > div').evaluate((el) => getComputedStyle(el).backgroundColor);
  const glow = shadow.includes("232, 133, 13") ? "оранжевая"
    : shadow.includes("31, 164, 99") ? "зелёная" : "нет";
  const state = ind.includes("31, 164, 99") ? "зелёный"
    : ind.includes("232, 133, 13") ? "оранжевый"
    : ind.includes("224, 48, 79") ? "красный" : `серый (${ind})`;
  console.log(`${name}: ${text} | точек горит ${lit} | обводка ${glow} | индикатор ${state}`);
  await page.locator('[data-testid="progress-widget"]').screenshot({ path: `${OUT}/behind-${name}.png` });
  await context.close();
}

// День 4 из 7, железа не съедено.
// A. Продукты ещё не обработаны железным проходом — про них ничего не известно.
await shoot("nofood", { ironOpenDaysAgo: 3, iron: {} });
// B. Продукты обработаны, железа в них практически нет — настоящий провал.
await shoot("lowiron", { ironOpenDaysAgo: 3, iron: { iron_mg: 0.2, iron_absorption: 0.03 } });

await b.close();
