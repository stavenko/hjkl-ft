// Скриншоты графиков с историей планок: шаги (ступенчатая линия) и вес
// (нормированная линия калорийной планки).
import { chromium } from "playwright";
import { openSeeded, DEFAULT_URL } from "./harness.mjs";

const BASE = process.env.FE || DEFAULT_URL;

const seed = async (page, uid) => {
  await page.evaluate(async (uid) => {
    const db = await new Promise((res, rej) => {
      const q = indexedDB.open(`hjkl-ft-${uid}`);
      q.onsuccess = () => res(q.result); q.onerror = () => rej(q.error);
    });
    const nowIso = new Date().toISOString();
    const ymd = (o) => { const d = new Date(); d.setDate(d.getDate() - o);
      return `${d.getFullYear()}-${String(d.getMonth() + 1).padStart(2, "0")}-${String(d.getDate()).padStart(2, "0")}`; };
    const app_flags = [
      { key: "push_onboarding_dismissed", value: "true" }, { key: "welcome_shown", value: "true" },
      { key: "db_schema_version", value: "999" },
      { key: "activity_week_unlocked", value: "true" }, { key: "steps_gate_opened_at", value: ymd(40) },
      { key: "ind_opened_at", value: ymd(40) },
      { key: "ft_subscription", value: JSON.stringify({ plan: "monthly", end: Date.now() + 30 * 864e5,
          active: true, start: Date.now(), status: "paid", no_renew: false, provider: "lava" }) },
    ];
    const profile = [{ key: "profile", sex: "male", height_cm: 180, birth_year: 1985,
      goal: "lose", steps_planka: 11800, created_at: nowIso, updated_at: nowIso }];
    const goals = [{ id: "g-cal", nutrient: "Calories", key: "calories", direction: "AtMost",
      amount: 2500, unit: "Kcal", period: "Day", created_at: nowIso, updated_at: nowIso }];
    // История с ТРЕМЯ ступенями у каждой планки — чтобы ступеньки было видно.
    const planka_history = [
      { id: `steps:${ymd(28)}`, kind: "steps", date: ymd(28), amount: 7000, created_at: nowIso, updated_at: nowIso },
      { id: `steps:${ymd(18)}`, kind: "steps", date: ymd(18), amount: 10000, created_at: nowIso, updated_at: nowIso },
      { id: `steps:${ymd(8)}`,  kind: "steps", date: ymd(8),  amount: 11800, created_at: nowIso, updated_at: nowIso },
      { id: `calories:${ymd(28)}`, kind: "calories", date: ymd(28), amount: 2800, created_at: nowIso, updated_at: nowIso },
      { id: `calories:${ymd(18)}`, kind: "calories", date: ymd(18), amount: 2650, created_at: nowIso, updated_at: nowIso },
      { id: `calories:${ymd(8)}`,  kind: "calories", date: ymd(8),  amount: 2500, created_at: nowIso, updated_at: nowIso },
    ];
    const step_entries = [];
    const weight_entries = [];
    for (let i = 1; i <= 28; i++) {
      // Шаги гуляют вокруг планки.
      const steps = 6500 + Math.round(3000 * Math.sin(i / 3)) + (i < 10 ? 4500 : i < 20 ? 3000 : 800);
      step_entries.push({ id: "s" + i, date: ymd(i), steps, created_at: nowIso, updated_at: nowIso });
      // Вес плавно снижается.
      weight_entries.push({ id: "w" + i, date: ymd(i), weight_kg: 88 + i * 0.12,
        no_water: true, no_food: true, no_wash: true, used_toilet: true, morning: true,
        created_at: nowIso, updated_at: nowIso });
    }
    for (const [store, rows] of Object.entries({ app_flags, profile, goals, planka_history, step_entries, weight_entries })) {
      await new Promise((res, rej) => {
        const tx = db.transaction([store], "readwrite");
        for (const r of rows) tx.objectStore(store).put(r);
        tx.oncomplete = () => res(); tx.onerror = () => rej(tx.error);
      });
    }
    db.close();
  }, uid);
};

const b = await chromium.launch({ headless: true });
const { context, page } = await openSeeded(b, {
  baseUrl: BASE, context: { serviceWorkers: "block", deviceScaleFactor: 3 },
  uid: `charts-${Math.floor(Math.random() * 1e6)}`, seed,
});
await page.setViewportSize({ width: 430, height: 950 });
await page.waitForTimeout(9000);

const openWidget = async (label) => {
  const el = page.getByText(label, { exact: false }).first();
  await el.waitFor({ timeout: 15000 });
  await el.click();
  await page.waitForTimeout(2500);
};

// График ШАГОВ — ступенчатая линия планки.
await openWidget("Шаги");
await page.locator("svg").first().screenshot({ path: "planka-chart-steps.png" });
console.log("planka-chart-steps.png");
await page.keyboard.press("Escape").catch(() => {});
await page.waitForTimeout(1200);

// График ВЕСА — нормированная линия калорийной планки.
await page.goto(BASE, { waitUntil: "domcontentloaded" });
await page.waitForTimeout(4000);
await openWidget("Вес");
await page.locator("svg").first().screenshot({ path: "planka-chart-weight.png" });
console.log("planka-chart-weight.png");

await context.close();
await b.close();
