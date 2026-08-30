// Недельный индикатор железа берёт окно в восемь недель, но НЕДОСТУПНЫЕ недели в
// него не входят: сколько есть, столько и судим.
//
// Наблюдалось живьём: дневник чуть больше четырёх недель, все четыре недели закрыты,
// индикатор КРАСНЫЙ — потому что цикл безусловно брал восемь недель, и четыре
// несуществующие считались незакрытыми, то есть ровно половиной окна.
//
// Два случая на двух аккаунтах:
//   A. четыре недели, все закрыты            → зелёный
//   B. четыре недели, две закрыты (половина) → красный
import { chromium } from "playwright";
import { openSeeded, DEFAULT_URL } from "./harness.mjs";

const BASE = process.env.FE || DEFAULT_URL;

let fail = 0;
const check = (n, ok, extra = "") => { console.log(`${ok ? "OK " : "FAIL"} ${n}${extra ? " — " + extra : ""}`); if (!ok) fail++; };

// closedWeeks — сколько из четырёх недель набирают недельную норму железа.
const makeSeed = (closedWeeks) => async (page, uid) => {
  await page.evaluate(async ({ uid, closedWeeks }) => {
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
      // Неделя железа открыта 29 дней назад: сетка недель ложится ровно на дневник.
      { key: "iron_week_unlocked", value: "true" }, { key: "iron_week_opened_at", value: ymd(29) },
      { key: "ft_subscription", value: JSON.stringify({ plan: "monthly", end: Date.now() + 30 * 864e5,
          active: true, start: Date.now(), status: "paid", no_renew: false, provider: "lava" }) },
    ];
    const profile = [{ key: "profile", sex: "male", height_cm: 180,
      birth_year: new Date().getFullYear() - 40, goal: "lose", steps_planka: 9000,
      created_at: nowIso, updated_at: nowIso }];
    // Без калорийной планки виджет не рисует строку индикаторов вовсе.
    const goals = [{ id: "g-cal", nutrient: "Calories", key: "calories", direction: "AtMost",
      amount: 2600, unit: "Kcal", period: "Day", created_at: nowIso, updated_at: nowIso }];
    // Печень: 9 мг железа, усвоение 0.25 → 200 г дают 4.5 мг усвоенного в день,
    // за неделю 31.5 мг при норме ~10.1. «Пустой» день — вода, дневник ведётся,
    // но железа нет.
    // Планка живёт в ИСТОРИИ — она и есть цель. Без неё приложение считает,
    // что планки нет вовсе, и не показывает ни одного индикатора.
    const planka_history = [{ id: `calories:${ymd(30)}`, kind: "calories",
      date: ymd(30), amount: 2600, created_at: nowIso, updated_at: nowIso }];
    const foods = [
      { id: "liver", name: "Куриная печень", kcal: 137, protein: 20, fat: 6, carbs: 1,
        nutrients: {}, package_weight: null, is_recipe: false, recipe_id: null, archived: false,
        is_restaurant: false, is_snack: false, is_liquid_cal: false, is_veg_fruit: false,
        is_egg: false, is_red_meat: true, iron_mg: 9, iron_absorption: 0.25,
        created_at: nowIso, updated_at: nowIso },
      { id: "water", name: "Вода", kcal: 0, protein: 0, fat: 0, carbs: 0,
        nutrients: {}, package_weight: null, is_recipe: false, recipe_id: null, archived: false,
        is_restaurant: false, is_snack: false, is_liquid_cal: false, is_veg_fruit: false,
        is_egg: false, is_red_meat: false, iron_mg: 0, iron_absorption: 0.02,
        created_at: nowIso, updated_at: nowIso },
    ];
    // Ровно ЧЕТЫРЕ завершённые недели дневника: дни 1…28 назад. Дальше — пусто.
    const diary = [];
    for (let i = 1; i <= 28; i++) {
      // Неделя 1 — самая свежая (дни 1–7), неделя 4 — самая старая (дни 22–28).
      const week = Math.ceil(i / 7);
      const closed = week <= closedWeeks;
      diary.push({ id: "d" + i, food_id: closed ? "liver" : "water", date: ymd(i), time: null,
        grams: 200, waste_grams: 0, meal_label: "lunch", deleted: false,
        created_at: nowIso, updated_at: nowIso });
    }
    for (const [store, rows] of Object.entries({ app_flags, profile, goals, planka_history, foods, diary })) {
      await new Promise((res, rej) => {
        const tx = db.transaction([store], "readwrite");
        for (const r of rows) tx.objectStore(store).put(r);
        tx.oncomplete = () => res(); tx.onerror = () => rej(tx.error);
      });
    }
    db.close();
  }, { uid, closedWeeks });
};

const colorName = (c) => c.includes("31, 164, 99") ? "зелёный"
  : c.includes("232, 133, 13") ? "оранжевый"
  : c.includes("224, 48, 79") ? "красный" : `неизвестно (${c})`;

const run = async (b, closedWeeks) => {
  const { context, page } = await openSeeded(b, {
    baseUrl: BASE, context: { serviceWorkers: "block" },
    uid: `ironhist-${closedWeeks}-${Math.floor(Math.random() * 1e6)}`, seed: makeSeed(closedWeeks),
  });
  await page.waitForTimeout(9000);
  const c = await page.locator('[data-ind="Железо"] > div')
    .evaluate((el) => getComputedStyle(el).backgroundColor).catch(() => "");
  await context.close();
  return colorName(c);
};

const b = await chromium.launch({ headless: true });

const all4 = await run(b, 4);
console.log(`четыре недели, все закрыты → ${all4}`);
check("все доступные недели закрыты — зелёный", all4 === "зелёный", all4);

const half = await run(b, 2);
console.log(`четыре недели, закрыты две → ${half}`);
check("половина доступных недель не закрыта — красный", half === "красный", half);

console.log(fail === 0 ? "\n=== ALL OK ===" : `\n=== FAILURES: ${fail} ===`);
await b.close();
process.exit(fail === 0 ? 0 : 1);
