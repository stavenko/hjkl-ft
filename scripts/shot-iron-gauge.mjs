// Скриншоты недельного gauge по железу в двух состояниях темпа: когда набор
// обгоняет суточные точки (зелёная обводка) и когда отстаёт (оранжевая).
import { chromium } from "playwright";
import { openSeeded, DEFAULT_URL } from "./harness.mjs";

const BASE = process.env.FE || DEFAULT_URL;
const OUT = process.env.OUT || "/private/tmp/claude-501/-Users-vasilijstavenko-projects-hjkl-ft/56df53af-a1ed-4117-8e82-8a1f8aad90e8/scratchpad/iron";

function seed({ ironOpenDaysAgo, gramsPerDay }) {
  return async (page, uid) => {
    await page.evaluate(async ({ uid, ironOpenDaysAgo, gramsPerDay }) => {
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
        { key: "activity_week_unlocked", value: "true" },
        { key: "steps_gate_opened_at", value: ymd(30) },
        { key: "calcium_week_unlocked", value: "true" },
        { key: "calcium_gate_opened_at", value: ymd(9) },
        { key: "ind_opened_at", value: ymd(40) },
        { key: "iron_week_unlocked", value: "true" },
        { key: "iron_week_opened_at", value: ymd(ironOpenDaysAgo) },
        { key: "ft_subscription", value: JSON.stringify({
            plan: "monthly", end: Date.now() + 30 * 24 * 3600 * 1000, active: true,
            start: Date.now(), status: "paid", no_renew: false, provider: "lava" }) },
      ];
      const profile = [{ key: "profile", sex: "male", height_cm: 180,
        birth_year: new Date().getFullYear() - 45, steps_planka: 10000,
        created_at: nowIso, updated_at: nowIso }];
      const goals = [
        { id: "g-cal", nutrient: "Calories", key: "calories", direction: "AtMost",
          amount: 2500, unit: "Kcal", period: "Day", created_at: nowIso, updated_at: nowIso },
        { id: "g-ca", nutrient: "Кальций", key: "calcium", direction: "AtLeast",
          amount: 1000, unit: "Mg", period: "Day", created_at: nowIso, updated_at: nowIso },
      ];
      const foods = [{
        id: "f-liver", name: "Куриная печень", kcal: 137, protein: 20, fat: 6, carbs: 1,
        nutrients: { "Кальций": 1200, "Клетчатка": 30 },
        package_weight: null, is_recipe: false, recipe_id: null, archived: false,
        is_restaurant: false, is_snack: false, is_liquid_cal: false, is_veg_fruit: false,
        is_egg: false, is_red_meat: true, iron_mg: 9.0, iron_absorption: 0.25,
        created_at: nowIso, updated_at: nowIso,
      }];
      const diary = [];
      for (let i = 0; i < 12; i++) {
        diary.push({ id: `d-${i}`, food_id: "f-liver", date: ymd(i), time: null,
          grams: gramsPerDay, waste_grams: 0, meal_label: null, deleted: false,
          created_at: nowIso, updated_at: nowIso });
      }
      for (const [store, rows] of Object.entries({ app_flags, profile, goals, foods, diary })) {
        await new Promise((res, rej) => {
          const tx = db.transaction([store], "readwrite");
          const os = tx.objectStore(store);
          for (const row of rows) os.put(row);
          tx.oncomplete = () => res(); tx.onerror = () => rej(tx.error);
        });
      }
      db.close();
    }, { uid, ironOpenDaysAgo, gramsPerDay });
  };
}

const b = await chromium.launch({ headless: true });

async function shoot(name, opts) {
  const { context, page } = await openSeeded(b, {
    baseUrl: BASE, context: { serviceWorkers: "block" },
    uid: `iron-g-${name}-${Math.floor(Math.random() * 1e6)}`,
    seed: seed(opts),
  });
  await page.setViewportSize({ width: 430, height: 932 });
  await page.waitForTimeout(9000);
  const gauge = page.locator('[data-gauge="Железо за неделю"]');
  await gauge.screenshot({ path: `${OUT}/gauge-${name}.png` });
  const lit = await page.locator('[data-pace-dot="lit"]').count();
  const dim = await page.locator('[data-pace-dot="dim"]').count();
  const text = (await gauge.innerText()).replace(/\s+/g, " ");
  console.log(`${name}: ${text} | точек горит ${lit}, погашено ${dim}`);
  await context.close();
}

// День 5 недели, ест помногу → обгоняет темп (зелёная обводка).
await shoot("ahead", { ironOpenDaysAgo: 4, gramsPerDay: 200 });
// День 5 недели, ест мало → отстаёт от темпа (оранжевая обводка).
await shoot("behind", { ironOpenDaysAgo: 4, gramsPerDay: 15 });
// Первый день — должок ещё не набежал, ни одна точка не горит.
await shoot("day1", { ironOpenDaysAgo: 0, gramsPerDay: 15 });

await b.close();
console.log("снято");
