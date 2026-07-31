// Four progress-widget states for design review, each in a fresh browser profile
// with the state planted directly in IndexedDB (sync worker is blocked so the
// shared dev account's server data can't leak in):
//   1. observation week, 3/7 days logged
//   2. first planka set, all plankas held 3 days (first gate: 4 days left)
//   3. steps planka (activity week) — steps gate 4 days left
//   4. calcium week — calcium gate 4 days left
import { chromium } from "playwright";
import { readFileSync } from "fs";

const FE = "https://renorma-fit-dev.pages.dev";
const uid = readFileSync("/tmp/repro_uid.txt", "utf8").trim();
const token = readFileSync("/tmp/repro_token.txt", "utf8").trim();

const iso = () => new Date().toISOString();
const day = (i) => { const t = new Date(); t.setDate(t.getDate() - i); return t.toISOString().slice(0, 10); };

// ── fixture builders (executed in the page) ──────────────────────────────────
const seedState = async (page, scenario) => {
  await page.evaluate(async ({ uid, scenario }) => {
    const now = new Date().toISOString();
    const day = (i) => { const t = new Date(); t.setDate(t.getDate() - i); return t.toISOString().slice(0, 10); };
    const db = await new Promise((res, rej) => { const q = indexedDB.open(`hjkl-ft-${uid}`); q.onsuccess = () => res(q.result); q.onerror = () => rej(q.error); });
    const stores = ["profile", "foods", "diary", "weight_entries", "step_entries", "goals", "app_flags",
                    "ind_calories", "ind_protein", "ind_veg_fruit", "ind_steps"];
    await new Promise((res) => {
      const tx = db.transaction(stores, "readwrite");
      const put = (s, r) => tx.objectStore(s).put(r);

      const food = (id, name, kcal, protein, fat, carbs, extra = {}) => ({
        id, name, kcal, protein, fat, carbs, nutrients: {}, package_weight: null,
        is_recipe: false, recipe_id: null, archived: false, created_at: now, updated_at: now, ...extra,
      });
      const entry = (id, foodId, date, grams) => ({
        id, food_id: foodId, date, time: null, grams, waste_grams: 0, meal_label: null,
        deleted: false, created_at: now, updated_at: now,
      });
      const flag = (key, value) => put("app_flags", { key, value, updated_at: now });
      const ind = (store, date, value, ratio) => put(store, { date, value, ratio, computed_at: now });

      put("profile", { key: "profile", sex: "male", height_cm: 180, birth_year: 1990, goal: "maintain",
        ...(scenario >= 3 ? { steps_planka: 8000 } : {}), updated_at: now });
      put("foods", food("f-main", "Курица с гречкой", 120, 9, 3, 14));
      put("foods", food("f-veg", "Овощной салат", 45, 1.5, 2, 5, { is_veg_fruit: true }));
      put("foods", food("f-milk", "Творог с молоком", 90, 12, 4, 5, { nutrients: { "Кальций": 300 } }));

      if (scenario === 1) {
        // 3 days of diary + weight + steps, nothing else.
        for (let i = 1; i <= 3; i++) {
          put("diary", entry(`e-m-${i}`, "f-main", day(i), 700));
          put("diary", entry(`e-v-${i}`, "f-veg", day(i), 400));
        }
        for (let i = 0; i <= 2; i++) {
          put("weight_entries", { id: `w-${i}`, date: day(i), weight_kg: 82 - i * 0.05, no_water: false,
            no_food: false, no_wash: false, used_toilet: false, morning: true, created_at: now, updated_at: now });
          put("step_entries", { id: `s-${i}`, date: day(i), steps: 8600, created_at: now, updated_at: now });
        }
      } else {
        // Planka exists; a full week of data behind us.
        put("goals", { id: "g-cal", nutrient: "Calories", key: "calories", direction: "AtMost",
          amount: 2000, unit: "Kcal", period: "Day", created_at: now, updated_at: now });
        flag("ind_opened_at", day(2)); // window counts d1..d3 -> 3 green days ("осталось 4")
        for (let i = 0; i <= 8; i++) {
          put("weight_entries", { id: `w-${i}`, date: day(i), weight_kg: 82 - i * 0.05, no_water: false,
            no_food: false, no_wash: false, used_toilet: false, morning: true, created_at: now, updated_at: now });
          put("step_entries", { id: `s-${i}`, date: day(i), steps: 8600, created_at: now, updated_at: now });
        }
        // Frozen GREEN days for the whole displayed week (indicators green),
        // the gate itself only counts from its open date (3 -> "осталось 4").
        for (let i = 1; i <= 7; i++) {
          ind("ind_calories", day(i), 1980, 1980 / 2000);
          ind("ind_protein", day(i), 110, 110 / 102);
          ind("ind_veg_fruit", day(i), 830, 830 / 800);
        }
        // Today's food for the gauges (calories/protein/veg under their targets).
        put("diary", entry("e-t-1", "f-main", day(0), 500));
        put("diary", entry("e-t-2", "f-veg", day(0), 300));

        if (scenario >= 3) {
          flag("activity_week_unlocked", "true");
          // Scenario 3: the steps gate itself is 3/7. Scenario 4: it is cleared
          // (opened 8 days ago, a full green week behind), calcium gate is 3/7.
          flag("steps_gate_opened_at", scenario === 3 ? day(3) : day(8));
          for (let i = 1; i <= 8; i++) ind("ind_steps", day(i), 8600, 8600 / 8000);
        }
        if (scenario === 4) {
          flag("calcium_week_unlocked", "true");
          flag("calcium_gate_opened_at", day(3));
          // Calcium is computed live from the diary: dairy every day incl today.
          for (let i = 0; i <= 7; i++) put("diary", entry(`e-ca-${i}`, "f-milk", day(i), 400));
        }
      }
      tx.oncomplete = res;
    });
    db.close();
  }, { uid, scenario });
};

const b = await chromium.launch({ headless: true });
const names = {
  1: "shot-widget-1-observation.png",
  2: "shot-widget-2-first-gate.png",
  3: "shot-widget-3-steps-gate.png",
  4: "shot-widget-4-calcium-gate.png",
};

for (const scenario of [1, 2, 3, 4]) {
  const ctx = await b.newContext({ viewport: { width: 430, height: 1200 }, serviceWorkers: "block", deviceScaleFactor: 2 });
  await ctx.route("**sync-worker**", (r) => r.abort());
  const page = await ctx.newPage();
  await page.goto(FE, { waitUntil: "domcontentloaded" });
  await page.evaluate(({ uid, token }) => {
    localStorage.clear();
    localStorage.setItem("user_id", uid); localStorage.setItem("auth_token", token);
    localStorage.setItem("token_id", "tok1"); localStorage.setItem("auth_ctx", "browser");
    localStorage.setItem("pwa_dismissed", "true");
    localStorage.setItem("paywall_skipped_date", new Date().toISOString().slice(0, 10));
  }, { uid, token });
  await page.goto(FE, { waitUntil: "domcontentloaded" });
  await page.waitForTimeout(4000);
  await seedState(page, scenario);
  await page.reload({ waitUntil: "domcontentloaded" });
  await page.waitForSelector('[data-testid="progress-widget"]', { timeout: 30000 });
  await page.waitForTimeout(4000);
  const el = page.locator('[data-testid="progress-widget"]');
  await el.screenshot({ path: `scripts/${names[scenario]}` });
  console.log(`[s${scenario}]`, (await el.innerText()).replace(/\n+/g, " | ").slice(0, 280));
  await ctx.close();
}
await b.close();
