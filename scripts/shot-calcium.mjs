// Live screenshot of Story 4 (calcium week), frame 1, on the REAL build.
// Seeds a realistic dashboard directly (the old-narrative `story` store is gone,
// so we don't use harness.seedInPage), flips the activity/calcium unlock flags,
// opens the week-4 story via the tray, and shoots the first frame.
import { chromium } from "playwright";
import { openSeeded } from "./harness.mjs";

const baseUrl = process.argv[2];
const b = await chromium.launch({ headless: true });

const { context, page } = await openSeeded(b, {
  baseUrl,
  landing: "/",
  seed: async (page, uid) => {
    await page.evaluate(async (uid) => {
      const open = (name) =>
        new Promise((res, rej) => {
          const r = indexedDB.open(name);
          r.onsuccess = () => res(r.result);
          r.onerror = () => rej(r.error);
        });
      const db = await open(`hjkl-ft-${uid}`);
      const now = new Date();
      const nowIso = now.toISOString();
      const ymd = (o) => {
        const d = new Date();
        d.setDate(d.getDate() - o);
        return `${d.getFullYear()}-${String(d.getMonth() + 1).padStart(2, "0")}-${String(d.getDate()).padStart(2, "0")}`;
      };
      const DAYS = 9;
      const end = now.getTime() + 30 * 24 * 60 * 60 * 1000;

      const records = {
        app_flags: [
          { key: "push_onboarding_dismissed", value: "true" },
          { key: "paywall_skipped_date", value: ymd(0) },
          { key: "ft_subscription", value: JSON.stringify({ plan: "monthly", end, active: true, start: now.getTime(), status: "paid", no_renew: false, provider: "lava" }) },
          { key: "activity_week_unlocked", value: "true" },
          { key: "calcium_week_unlocked", value: "true" },
        ],
        profile: [{ key: "profile", sex: "male", height_cm: 180, birth_year: 1990, goal: "lose", cycle_start: null, steps_planka: 8000, updated_at: nowIso }],
        weight_entries: [],
        step_entries: [],
        foods: [{
          id: "seed-food", name: "Рацион дня", kcal: 1500, protein: 120, fat: 45, carbs: 130,
          nutrients: {}, package_weight: null, is_recipe: false, recipe_id: null,
          archived: false, is_restaurant: false, is_snack: false, created_at: nowIso, updated_at: nowIso,
        }],
        diary: [],
        goals: [
          { id: "seed-cal-goal", nutrient: "Calories", key: "calories", direction: "AtMost", amount: 1500, unit: "Kcal", period: "Day", created_at: nowIso, updated_at: nowIso },
          { id: "seed-ca-goal", nutrient: "Кальций", key: "calcium", direction: "AtLeast", amount: 1000, unit: "Mg", period: "Day", created_at: nowIso, updated_at: nowIso },
        ],
      };
      for (let i = 0; i < DAYS; i++) {
        records.weight_entries.push({ id: `seed-w-${i}`, date: ymd(i), weight_kg: 80 - i * 0.1, no_water: false, no_food: false, no_wash: false, used_toilet: false, morning: true, created_at: nowIso, updated_at: nowIso });
        records.step_entries.push({ id: `seed-s-${i}`, date: ymd(i), steps: 8000, created_at: nowIso, updated_at: nowIso });
        records.diary.push({ id: `seed-d-${i}`, food_id: "seed-food", date: ymd(i), time: null, grams: 100, waste_grams: 0, meal_label: null, deleted: false, created_at: nowIso, updated_at: nowIso });
      }

      const available = Array.from(db.objectStoreNames);
      for (const [store, rows] of Object.entries(records)) {
        if (!rows.length || !available.includes(store)) continue;
        await new Promise((res, rej) => {
          const tx = db.transaction([store], "readwrite");
          const os = tx.objectStore(store);
          for (const row of rows) os.put(row);
          tx.oncomplete = () => res();
          tx.onerror = () => rej(tx.error);
        });
      }
      db.close();
    }, uid);
  },
});

await page.waitForTimeout(1500);
// Open the week-4 story: its tray circle carries the badge "4".
const circle = page.getByRole("button", { name: "4", exact: true });
await circle.waitFor({ state: "visible", timeout: 10000 });
await circle.click();
await page.waitForTimeout(1000); // let the Portal viewer mount + fonts settle
await page.screenshot({ path: "calcium-frame1-live.png" });
await context.close();
await b.close();
console.log("ok");
