// Live screenshot of the diary meal panels — verifying the footer now always
// shows the aggregate КБЖУ (expanded AND collapsed).
import { chromium } from "playwright";
import { openSeeded } from "./harness.mjs";

const baseUrl = process.argv[2];
const b = await chromium.launch({ headless: true });

const { context, page } = await openSeeded(b, {
  baseUrl,
  landing: "/",
  seed: async (page, uid) => {
    await page.evaluate(async (uid) => {
      const open = (name) => new Promise((res, rej) => { const r = indexedDB.open(name); r.onsuccess = () => res(r.result); r.onerror = () => rej(r.error); });
      const db = await open(`hjkl-ft-${uid}`);
      const now = new Date();
      const nowIso = now.toISOString();
      const ymd = (o) => { const d = new Date(); d.setDate(d.getDate() - o); return `${d.getFullYear()}-${String(d.getMonth() + 1).padStart(2, "0")}-${String(d.getDate()).padStart(2, "0")}`; };
      const end = now.getTime() + 30 * 24 * 60 * 60 * 1000;
      const today = ymd(0);
      const records = {
        app_flags: [
          { key: "push_onboarding_dismissed", value: "true" },
          { key: "paywall_skipped_date", value: ymd(0) },
          { key: "ft_subscription", value: JSON.stringify({ plan: "monthly", end, active: true, start: now.getTime(), status: "paid", no_renew: false, provider: "lava" }) },
        ],
        profile: [{ key: "profile", sex: "male", height_cm: 180, birth_year: 1990, goal: "lose", cycle_start: null, steps_planka: 8000, updated_at: nowIso }],
        foods: [
          { id: "f-oat", name: "Овсянка на молоке", kcal: 90, protein: 3.5, fat: 2.5, carbs: 13, nutrients: {}, package_weight: null, is_recipe: false, recipe_id: null, archived: false, is_restaurant: false, is_snack: false, created_at: nowIso, updated_at: nowIso },
          { id: "f-egg", name: "Яйцо варёное", kcal: 155, protein: 13, fat: 11, carbs: 1.1, nutrients: {}, package_weight: null, is_recipe: false, recipe_id: null, archived: false, is_restaurant: false, is_snack: false, created_at: nowIso, updated_at: nowIso },
          { id: "f-ban", name: "Банан", kcal: 89, protein: 1.1, fat: 0.3, carbs: 23, nutrients: {}, package_weight: null, is_recipe: false, recipe_id: null, archived: false, is_restaurant: false, is_snack: false, created_at: nowIso, updated_at: nowIso },
        ],
        diary: [
          { id: "d-oat", food_id: "f-oat", date: today, time: null, grams: 250, waste_grams: 0, meal_label: "breakfast", deleted: false, created_at: nowIso, updated_at: nowIso },
          { id: "d-egg", food_id: "f-egg", date: today, time: null, grams: 100, waste_grams: 0, meal_label: "breakfast", deleted: false, created_at: nowIso, updated_at: nowIso },
          { id: "d-ban", food_id: "f-ban", date: today, time: null, grams: 120, waste_grams: 0, meal_label: "breakfast", deleted: false, created_at: nowIso, updated_at: nowIso },
        ],
      };
      const available = Array.from(db.objectStoreNames);
      for (const [store, rows] of Object.entries(records)) {
        if (!rows.length || !available.includes(store)) continue;
        await new Promise((res, rej) => { const tx = db.transaction([store], "readwrite"); const os = tx.objectStore(store); for (const row of rows) os.put(row); tx.oncomplete = () => res(); tx.onerror = () => rej(tx.error); });
      }
      db.close();
    }, uid);
  },
});

await page.waitForTimeout(1200);
// Navigate to the diary via the in-app bottom nav (client-side routing).
await page.getByText("Дневник", { exact: true }).first().click();
await page.waitForTimeout(1500);
await page.screenshot({ path: "meal-expanded.png", fullPage: true });

// Collapse the breakfast panel via its toggle (chevron button in the footer).
const toggle = page.getByRole("button", { name: "Свернуть" }).first();
if (await toggle.count()) {
  await toggle.click();
  await page.waitForTimeout(500);
  await page.screenshot({ path: "meal-collapsed.png", fullPage: true });
  console.log("collapsed shot ok");
} else {
  console.log("toggle not found");
}
await context.close();
await b.close();
console.log("ok");
