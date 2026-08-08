// У КАЖДОГО индикатора своё пояснение «?».
//
// Общий текст объясняет правило цвета и ничего не говорит о самом показателе: на
// гемовом железе это и вылезло — «все недели вы закрывали норму порций» и всё.
// Проверка обходит панели индикаторов в развёрнутом виде, открывает у каждой «?» и
// требует, чтобы текст был содержательным и НЕ повторял чужой.
import { chromium } from "playwright";
import { openSeeded, DEFAULT_URL } from "./harness.mjs";

const BASE = process.env.FE || DEFAULT_URL;

let fail = 0;
const check = (n, ok, extra = "") => {
  console.log(`${ok ? "OK " : "FAIL"} ${n}${extra ? " — " + extra : ""}`);
  if (!ok) fail++;
};

// Хвост вердикта о цвете, общий у всех: пояснение обязано содержать что-то СВЕРХ него.
const VERDICT_TAIL = /Зелёный:|Оранжевый:|Красный:|Пока/;

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
      { key: "iron_week_opened_at", value: ymd(14) },
      { key: "fat_week_unlocked", value: "true" },
      { key: "fat_week_opened_at", value: ymd(7) },
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
      { id: "g-steps", nutrient: "Steps", key: "steps", direction: "AtLeast",
        amount: 9000, unit: "Count", period: "Day", created_at: nowIso, updated_at: nowIso },
    ];
    const foods = [{
      id: "mack", name: "Скумбрия", kcal: 191, protein: 18, fat: 13.9, carbs: 0,
      nutrients: { "Кальций": 12 }, package_weight: null, is_recipe: false, recipe_id: null,
      archived: false, is_restaurant: false, is_snack: false, is_liquid_cal: false,
      is_veg_fruit: false, is_egg: false, is_red_meat: false, is_heme: true,
      iron_mg: 1.6, iron_absorption: 0.15,
      fat_profile: { sfa_pct: 24, mufa_pct: 38, pufa_pct: 26, ala_pct: 1, epa_dha_pct: 18 },
      created_at: nowIso, updated_at: nowIso,
    }];
    const diary = [], weight_entries = [], step_entries = [];
    for (let i = 0; i < 14; i++) {
      diary.push({ id: "d" + i, food_id: "mack", date: ymd(i), time: null, grams: 200,
        waste_grams: 0, meal_label: "lunch", deleted: false, created_at: nowIso, updated_at: nowIso });
      weight_entries.push({ id: "w" + i, date: ymd(i), weight_kg: 88, no_water: false,
        no_food: false, no_wash: false, used_toilet: false, morning: true,
        created_at: nowIso, updated_at: nowIso });
      step_entries.push({ id: "s" + i, date: ymd(i), steps: 11000,
        created_at: nowIso, updated_at: nowIso });
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
  baseUrl: BASE, context: { serviceWorkers: "block" },
  uid: `hints-${Math.floor(Math.random() * 1e6)}`, seed,
});
await page.reload({ waitUntil: "domcontentloaded" });
await page.waitForTimeout(6000);
await page.locator('[data-testid="progress-widget"]').dispatchEvent("pointerup");
await page.waitForTimeout(5000);

// Берём ИМЕННО панели индикаторов: у шкал «?» тоже есть, но там текст без абзаца
// про цвет, и мерить его этой меркой нельзя.
const panels = await page.$$eval("[data-ind-panel]", (els) =>
  els.map((e) => e.getAttribute("data-ind-panel")));
console.log("панели:", JSON.stringify(panels));

const seen = new Map();
for (let i = 0; i < panels.length; i++) {
  const name = panels[i];
  await page.locator(`[data-ind-panel="${name}"] button[aria-label="?"]`).click();
  await page.waitForTimeout(300);
  const text = await page.locator('[style*="z-index: 41"] span').innerText().catch(() => "");
  await page.mouse.click(10, 10);
  await page.waitForTimeout(200);
  // Само по себе правило цвета — не пояснение. Требуем текст СВЕРХ вердикта.
  const beyond = text.split("\n\n").slice(1).join("\n\n").trim();
  check(`«${name}»: есть текст про сам показатель`, beyond.length > 100,
    `${beyond.length} симв.: ${beyond.slice(0, 60)}…`);
  const dup = [...seen.entries()].find(([, t]) => t === beyond && beyond);
  check(`«${name}»: пояснение не повторяет чужое`, !dup, dup ? `совпало с «${dup[0]}»` : "");
  seen.set(name, beyond);
}
check("панели индикаторов найдены", panels.length >= 8, `${panels.length}`);

console.log(fail === 0 ? "\n=== ALL OK ===" : `\n=== FAILURES: ${fail} ===`);
await context.close();
await b.close();
process.exit(fail === 0 ? 0 : 1);
