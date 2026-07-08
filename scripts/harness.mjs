// Story test/seed harness — reusable core.
//
// Seeds a fresh per-user IndexedDB so a given story section is unlocked (every
// earlier section complete + every chapter up to it open), with the target
// section's own task left undone. Used by both `manual-test.mjs` (headed,
// hands-on) and `story.test.mjs` (headless assertions).
//
// MUST stay in sync with story.yaml (section ids + completion rules) and
// story.rs (TASK_FLAG → flag strings).

export const DEFAULT_URL = "https://hjkl-ft.pages.dev";

// task id → the milestone flag string the engine reads (story.rs TASK_FLAG).
export const FLAG = {
  photos: "progress_photos_taken",
  sex: "sex_selected",
  lang: "language_configured",
  notif: "notification_received",
  weigh_in: "weigh_in_reminder_enabled",
  first_weigh: "first_measurement_done",
  first_food: "first_food_done",
  repeat_food: "food_repeated",
  steps_reminder: "steps_reminder_enabled",
  first_steps: "first_steps_done",
  dish_created: "cooking_dish_created",
  dish_in_diary: "cooking_dish_in_diary",
  bones: "bones_waste_entered",
  restaurant: "restaurant_food_entered",
};

// Per section: what makes it COMPLETE (besides being opened/seen).
//   flags:   milestone flags to set true.
//   sensors: data-backed conditions (see seedInPage): weightStreak | stepsStreak |
//            diaryStreak | diaryDays | diaryContinue | snack | drinks |
//            caloriePlanka | subActive.
// Chapter `open` lists the sensors of the chapter's own unlock condition.
export const CHAPTERS = [
  {
    n: 1,
    open: [], // always
    sections: [
      { id: "intro", flags: [FLAG.photos] },
      { id: "setup", flags: [FLAG.sex, FLAG.lang, FLAG.notif] },
      { id: "accounting", flags: [FLAG.weigh_in, FLAG.first_weigh] },
      { id: "first-food", flags: [FLAG.first_food] },
      { id: "activity", flags: [FLAG.steps_reminder, FLAG.first_steps] },
      { id: "cooking", flags: [FLAG.dish_created, FLAG.dish_in_diary] },
      { id: "bones", flags: [FLAG.bones] },
      { id: "restaurant", flags: [FLAG.restaurant] },
      { id: "diary", flags: [FLAG.repeat_food] },
    ],
  },
  {
    n: 2,
    open: ["weightStreak", "stepsStreak", "diaryStreak", "subActive"],
    sections: [
      { id: "ch2-mistake" },
      { id: "ch2-veg" },
      { id: "ch2-protein" },
      { id: "ch2-snack" },
      { id: "ch2-drinks" },
      { id: "ch2-meals" },
      { id: "ch2-night" },
    ],
  },
  {
    n: 3,
    open: ["diaryStreak"],
    sections: [
      { id: "ch3-deficit", sensors: ["caloriePlanka"] },
      { id: "ch3-no-loss" },
      { id: "ch3-calorie" },
      { id: "ch3-friend" },
      { id: "ch3-sleep" },
      { id: "ch3-walk" },
      { id: "ch3-habits" },
    ],
  },
];

export function parseTarget(arg) {
  const m = /^(\d+)\.(\d+)$/.exec(arg ?? "");
  if (!m) throw new Error(`bad target "${arg}" — use <chapter>.<section>, e.g. 2.0`);
  const chapter = Number(m[1]);
  const section = Number(m[2]);
  const ch = CHAPTERS[chapter - 1];
  if (!ch) throw new Error(`no chapter ${chapter} (have 1..${CHAPTERS.length})`);
  if (section < 0 || section >= ch.sections.length) {
    throw new Error(`chapter ${chapter} has sections 0..${ch.sections.length - 1}`);
  }
  return { chapter, section, ch };
}

// Build the seed plan: union of every requirement to OPEN the target, minus the
// target's own (so the target stays incomplete / testable).
export function buildPlan({ chapter, section, ch }) {
  const required = [];
  for (let ci = 0; ci < chapter - 1; ci++) required.push(...CHAPTERS[ci].sections);
  required.push(...ch.sections.slice(0, section));

  const flags = new Set();
  const seen = new Set();
  const sensors = new Set();
  for (const s of required) {
    (s.flags ?? []).forEach((f) => flags.add(f));
    (s.sensors ?? []).forEach((x) => sensors.add(x));
    seen.add(s.id);
  }
  for (let ci = 1; ci < chapter; ci++) CHAPTERS[ci].open.forEach((x) => sensors.add(x));

  return { targetId: ch.sections[section].id, flags: [...flags], seen: [...seen], sensors: [...sensors] };
}

/** Convenience: `planFor("2.1")` → the seed plan for that target. */
export function planFor(spec) {
  return buildPlan(parseTarget(spec));
}

// Runs IN THE BROWSER: write the seed records into the per-user IndexedDB.
export async function seedInPage(page, uid, plan) {
  await page.evaluate(
    async ({ uid, plan }) => {
      const open = (name) =>
        new Promise((res, rej) => {
          const r = indexedDB.open(name);
          r.onsuccess = () => res(r.result);
          r.onerror = () => rej(r.error);
        });
      const db = await open(`hjkl-ft-${uid}`);

      const now = new Date();
      const nowIso = now.toISOString();
      const ymd = (offset) => {
        const d = new Date();
        d.setDate(d.getDate() - offset);
        return `${d.getFullYear()}-${String(d.getMonth() + 1).padStart(2, "0")}-${String(d.getDate()).padStart(2, "0")}`;
      };
      const yesterday = ymd(1);
      const DAYS = 9;

      const records = {
        story: [], app_flags: [], weight_entries: [], step_entries: [],
        diary: [], goals: [], foods: [], summaries: [],
      };

      records.app_flags.push({ key: "push_onboarding_dismissed", value: "true" });
      records.app_flags.push({ key: "paywall_skipped_date", value: ymd(0) });

      for (const f of plan.flags) records.story.push({ key: f, value: true, updated_at: nowIso });
      for (const id of plan.seen)
        records.story.push({ key: `seen:/story/${id}`, value: true, updated_at: nowIso });

      const want = (s) => plan.sensors.includes(s);

      if (want("weightStreak")) {
        for (let i = 0; i < DAYS; i++)
          records.weight_entries.push({
            id: `seed-w-${i}`, date: ymd(i), weight_kg: 80 - i * 0.1,
            no_water: false, no_food: false, no_wash: false, used_toilet: false,
            morning: true, created_at: nowIso, updated_at: nowIso,
          });
      }
      if (want("stepsStreak")) {
        for (let i = 0; i < DAYS; i++)
          records.step_entries.push({ id: `seed-s-${i}`, date: ymd(i), steps: 8000, created_at: nowIso, updated_at: nowIso });
      }

      const wantDiary =
        want("diaryStreak") || want("diaryDays") || want("snack") || want("drinks") || want("diaryContinue");
      if (wantDiary) {
        // The base diary food (2895 kcal / 100 g) — at grams:100 each day totals
        // 2895 kcal, so avg_daily_kcal over the last 7 days = 2895 (the ch3 planka).
        records.foods.push({
          id: "seed-food", name: "Рацион дня", kcal: 2895, protein: 120, fat: 95, carbs: 350,
          nutrients: {}, package_weight: null, is_recipe: false, recipe_id: null,
          archived: false, is_restaurant: false, is_snack: false, created_at: nowIso, updated_at: nowIso,
        });
        if (want("snack")) {
          records.foods.push({
            id: "seed-snack", name: "Морковь", kcal: 35, protein: 1, fat: 0, carbs: 8,
            nutrients: {}, package_weight: null, is_recipe: false, recipe_id: null,
            archived: false, is_restaurant: false, is_snack: true, created_at: nowIso, updated_at: nowIso,
          });
        }
        for (let i = 0; i < DAYS; i++) {
          records.diary.push({
            id: `seed-d-${i}`,
            food_id: want("snack") && ymd(i) === yesterday ? "seed-snack" : "seed-food",
            date: ymd(i), time: null, grams: 100, waste_grams: 0, meal_label: null,
            deleted: false, created_at: nowIso, updated_at: nowIso,
          });
        }
      }
      if (wantDiary) {
        const facts = {
          kcal: 1500, protein: 90, fat: 50, carbs: 150,
          veg_fruit_grams: 650, snack_logged: want("snack"), high_cal_drink: false,
          evening_protein_g: 35, meal_distribution: [], calorie_planka: 0,
        };
        const text = JSON.stringify({ facts, good: [], improve: [] });
        records.summaries.push({ id: `day:${yesterday}`, date: yesterday, text, error: null, created_at: nowIso });
      }
      if (want("caloriePlanka")) {
        records.goals.push({
          id: "seed-cal-goal", nutrient: "Calories", key: "calories",
          direction: "AtMost", amount: 1500, unit: "Kcal", period: "Day", created_at: nowIso, updated_at: nowIso,
        });
      }
      if (want("subActive")) {
        const end = now.getTime() + 30 * 24 * 60 * 60 * 1000;
        records.app_flags.push({
          key: "ft_subscription",
          value: JSON.stringify({ plan: "monthly", end, active: true, start: now.getTime(), status: "paid", no_renew: false, provider: "lava" }),
        });
      }

      const available = Array.from(db.objectStoreNames);
      for (const [store, rows] of Object.entries(records)) {
        if (!rows.length) continue;
        if (!available.includes(store)) {
          db.close();
          throw new Error(`store "${store}" missing; DB has: ${available.join(", ")}`);
        }
        await new Promise((res, rej) => {
          const tx = db.transaction([store], "readwrite");
          const os = tx.objectStore(store);
          for (const row of rows) os.put(row);
          tx.oncomplete = () => res();
          tx.onerror = () => rej(tx.error);
        });
      }
      db.close();
    },
    { uid, plan },
  );
}

/**
 * Full flow in a fresh browser context: set identity → let the app create the
 * per-user DB → seed → navigate to `landing`. Returns { context, page }; the
 * caller asserts, then closes the context.
 *
 * Provide either `plan` (seeded via seedInPage) or a custom `seed(page, uid)`.
 */
export async function openSeeded(browser, opts = {}) {
  const baseUrl = (opts.baseUrl ?? DEFAULT_URL).replace(/\/$/, "");
  const uid = opts.uid ?? `harness-${Math.abs(hash(JSON.stringify(opts.plan ?? opts.landing ?? "")) )}`;
  const sex = opts.sex ?? "male";
  const landing = opts.landing ?? "/";

  const context = await browser.newContext({ viewport: { width: 390, height: 844 } });
  const page = await context.newPage();

  await page.goto(baseUrl, { waitUntil: "domcontentloaded" });
  await page.evaluate(
    async ({ uid, sex }) => {
      const del = (n) =>
        new Promise((r) => {
          const req = indexedDB.deleteDatabase(n);
          req.onsuccess = req.onerror = req.onblocked = () => r();
        });
      await del("hjkl-ft");
      await del(`hjkl-ft-${uid}`);
      localStorage.clear();
      localStorage.setItem("user_id", uid);
      localStorage.setItem("auth_token", "harness.test.token");
      localStorage.setItem("pwa_dismissed", "true");
      if (sex) localStorage.setItem("profile_sex", sex);
    },
    { uid, sex },
  );
  await page.goto(baseUrl, { waitUntil: "domcontentloaded" });

  let ready = false;
  for (let i = 0; i < 40 && !ready; i++) {
    try {
      ready = await page.evaluate(async (uid) => {
        const dbs = await indexedDB.databases();
        if (!dbs.some((d) => d.name === `hjkl-ft-${uid}`)) return false;
        return await new Promise((res) => {
          const r = indexedDB.open(`hjkl-ft-${uid}`);
          r.onsuccess = () => { const ok = r.result.objectStoreNames.contains("app_flags"); r.result.close(); res(ok); };
          r.onerror = () => res(false);
        });
      }, uid);
    } catch {
      // The SPA router can client-side navigate mid-poll, destroying the execution
      // context — treat as not-ready and retry rather than failing the test.
      ready = false;
    }
    if (!ready) await page.waitForTimeout(500);
  }
  if (!ready) throw new Error("per-user DB never appeared — is the target build current (v10)?");

  if (opts.seed) await opts.seed(page, uid);
  else if (opts.plan) await seedInPage(page, uid, opts.plan);

  await page.goto(`${baseUrl}${landing}`, { waitUntil: "domcontentloaded" });
  await page.waitForSelector("#splash", { state: "detached", timeout: 15000 }).catch(() => {});
  await page.waitForTimeout(500); // let reactive effects (DB reads) settle
  return { context, page };
}

function hash(s) {
  let h = 0;
  for (let i = 0; i < s.length; i++) h = (h * 31 + s.charCodeAt(i)) | 0;
  return h;
}
