// Подпись-гейт показывает ТЕКУЩУЮ главу — ту, что ещё не открыта.
//
// Наблюдалось живьём: неделя железа давно открыта, а подпись зовёт «держите
// индикатор планки по шагам зелёным 7 дней». Шаговый гейт своё отработал (он и
// открыл кальциевую неделю), но индикатор шагов позеленел в оранжевый, счётчик
// зелёных дней сбросился — и гейт вылез заново. Гейты монотонны: пройденное не
// отменяется.
//
// Сеем ровно это: все три недели открыты, индикатор шагов НЕ зелёный. Подпись
// обязана звать по железу.
import { chromium } from "playwright";
import { openSeeded, DEFAULT_URL } from "./harness.mjs";

const BASE = process.env.FE || DEFAULT_URL;

let fail = 0;
const check = (n, ok, extra = "") => { console.log(`${ok ? "OK " : "FAIL"} ${n}${extra ? " — " + extra : ""}`); if (!ok) fail++; };

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
      { key: "ind_opened_at", value: ymd(60) },
      // ВСЕ три недели открыты — шаговый и кальциевый гейты своё отработали.
      { key: "activity_week_unlocked", value: "true" }, { key: "steps_gate_opened_at", value: ymd(50) },
      { key: "calcium_week_unlocked", value: "true" }, { key: "calcium_gate_opened_at", value: ymd(40) },
      { key: "iron_week_unlocked", value: "true" }, { key: "iron_week_opened_at", value: ymd(3) },
      { key: "ft_subscription", value: JSON.stringify({ plan: "monthly", end: Date.now() + 30 * 864e5,
          active: true, start: Date.now(), status: "paid", no_renew: false, provider: "lava" }) },
    ];
    const profile = [{ key: "profile", sex: "male", height_cm: 180,
      birth_year: new Date().getFullYear() - 40, goal: "lose", steps_planka: 11800,
      created_at: nowIso, updated_at: nowIso }];
    const goals = [{ id: "g-cal", nutrient: "Calories", key: "calories", direction: "AtMost",
      amount: 2600, unit: "Kcal", period: "Day", created_at: nowIso, updated_at: nowIso }];
    const foods = [{ id: "f1", name: "Гречка", kcal: 300, protein: 10, fat: 3, carbs: 60,
      nutrients: { "Кальций": 20, "Клетчатка": 3, "Омега-3": 100 }, package_weight: null,
      is_recipe: false, recipe_id: null, archived: false, is_restaurant: false, is_snack: false,
      is_liquid_cal: false, is_veg_fruit: false, is_egg: false, is_red_meat: false,
      iron_mg: 3, iron_absorption: 0.04, created_at: nowIso, updated_at: nowIso }];
    const diary = [];
    const step_entries = [];
    for (let i = 1; i <= 14; i++) {
      diary.push({ id: "d" + i, food_id: "f1", date: ymd(i), time: null, grams: 200,
        waste_grams: 0, meal_label: "lunch", deleted: false, created_at: nowIso, updated_at: nowIso });
      // Шагов МЕНЬШЕ планки — индикатор шагов не зелёный, его счётчик сброшен.
      step_entries.push({ id: "s" + i, date: ymd(i), steps: 6000,
        created_at: nowIso, updated_at: nowIso });
    }
    for (const [store, rows] of Object.entries({ app_flags, profile, goals, foods, diary, step_entries })) {
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
  baseUrl: BASE, context: { serviceWorkers: "block" },
  uid: `gate-${Math.floor(Math.random() * 1e6)}`, seed,
});
await page.waitForTimeout(10000);

const text = (await page.locator('[data-testid="progress-widget"]').innerText()).replace(/\s+/g, " ");
console.log(`подпись: ${(text.match(/(Держите[^.]*\.|Выполните[^.]*\.)/) || ["— нет —"])[0]}`);

check("подпись зовёт по ЖЕЛЕЗУ", text.includes("Выполните планку по железу"), text.slice(0, 200));
check("шаговый гейт не вернулся", !text.includes("индикатор планки по шагам"), text.slice(0, 200));
check("кальциевый гейт не вернулся", !text.includes("индикатор кальция"), text.slice(0, 200));

console.log(fail === 0 ? "\n=== ALL OK ===" : `\n=== FAILURES: ${fail} ===`);
await context.close();
await b.close();
process.exit(fail === 0 ? 0 : 1);
