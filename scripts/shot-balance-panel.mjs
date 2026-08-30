// Панель «Баланс» в развёрнутом виде: столбики от середины, знак и цвет — те же,
// что на шкале в виджете.
//
// Сеем ВОСЕМЬ завершённых недель жира вперемешку: в чётные ел плохо (сало и сыр),
// в нечётные хорошо (оливковое масло и рыба). Так на одном снимке видны обе
// стороны — красные столбики вниз и зелёные вверх.
import { chromium } from "playwright";
import { openSeeded, tapWidget, DEFAULT_URL } from "./harness.mjs";

const BASE = process.env.FE || DEFAULT_URL;
const OUT = process.env.SHOT || "/tmp/balance-panel.png";

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
      { key: "calcium_week_unlocked", value: "true" },
      { key: "ind_opened_at", value: ymd(80) },
      { key: "iron_week_unlocked", value: "true" },
      { key: "iron_week_opened_at", value: ymd(70) },
      { key: "fat_week_unlocked", value: "true" },
      // Неделя жира открыта 63 дня назад → позади ровно девять недель.
      { key: "fat_week_opened_at", value: ymd(63) },
      { key: "fat_gate_anchor", value: ymd(70) },
      { key: "ft_subscription", value: JSON.stringify({
          plan: "monthly", end: Date.now() + 30 * 864e5, active: true,
          start: Date.now(), status: "paid", no_renew: false, provider: "lava" }) },
    ];
    const profile = [{ key: "profile", sex: "male", height_cm: 180,
      birth_year: new Date().getFullYear() - 45, goal: "lose", steps_planka: 9000,
      created_at: nowIso, updated_at: nowIso }];
    const goals = [{ id: "g-cal", nutrient: "Calories", key: "calories", direction: "AtMost",
      amount: 2600, unit: "Kcal", period: "Day", created_at: nowIso, updated_at: nowIso }];
    const food = (id, name, kcal, fat, fp) => ({
      id, name, kcal, protein: 10, fat, carbs: 0, nutrients: {},
      package_weight: null, is_recipe: false, recipe_id: null, archived: false,
      is_restaurant: false, is_snack: false, is_liquid_cal: false, is_veg_fruit: false,
      is_egg: false, is_red_meat: false, is_heme: false,
      iron_mg: 1.0, iron_absorption: 0.15, fat_profile: fp,
      created_at: nowIso, updated_at: nowIso,
    });
    const foods = [
      // Отношение 1.0 — вдвое хуже нормы.
      food("lard", "Сало", 797, 89, { sfa_pct: 45, mufa_pct: 38, pufa_pct: 7, epa_dha_pct: 0 }),
      // Отношение 5.7 — заметно лучше нормы.
      food("olive", "Оливковое масло", 884, 100,
        { sfa_pct: 15, mufa_pct: 70, pufa_pct: 15, epa_dha_pct: 0 }),
      food("mack", "Скумбрия", 191, 13.9,
        { sfa_pct: 24, mufa_pct: 38, pufa_pct: 26, epa_dha_pct: 18 }),
    ];
    const diary = [];
    for (let i = 0; i < 63; i++) {
      // Номер завершённой недели, считая от сегодняшнего дня назад.
      const week = Math.floor(i / 7);
      const bad = week % 2 === 0;
      diary.push({ id: "d" + i, food_id: bad ? "lard" : "olive", date: ymd(i), time: null,
        grams: bad ? 60 : 30, waste_grams: 0, meal_label: "lunch", deleted: false,
        created_at: nowIso, updated_at: nowIso });
      if (i % 3 === 0) {
        diary.push({ id: "dm" + i, food_id: "mack", date: ymd(i), time: null, grams: 150,
          waste_grams: 0, meal_label: "dinner", deleted: false,
          created_at: nowIso, updated_at: nowIso });
      }
    }
    const avail = Array.from(db.objectStoreNames);
    for (const [store, rows] of Object.entries({ app_flags, profile, goals, foods, diary })) {
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
  baseUrl: BASE, context: { serviceWorkers: "block", deviceScaleFactor: 2 },
  uid: `balance-${Math.floor(Math.random() * 1e6)}`, seed,
});
await page.reload({ waitUntil: "domcontentloaded" });
await page.waitForTimeout(6000);
await tapWidget(page, '[data-testid="progress-widget"]');
await page.waitForTimeout(6000);

const panel = page.locator('[data-ind-panel="fat_ratio"]');
await panel.scrollIntoViewIfNeeded();
await page.waitForTimeout(600);
await panel.screenshot({ path: OUT });
console.log("снято:", OUT);

await context.close();
await b.close();
