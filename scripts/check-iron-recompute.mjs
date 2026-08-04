// Железо ничего не замораживает: значение выводится из дневника и продуктов при
// каждом чтении. Значит появление железа у продукта задним числом обязано
// пересчитать и полосу, и индикатор — БЕЗ перезапуска приложения.
//
// Сеем восемь завершённых недель еды, у которой железа ещё нет, смотрим состояние,
// затем прописываем железо продукту прямо в базу и проверяем, что виджет ожил.
import { chromium } from "playwright";
import { openSeeded, DEFAULT_URL } from "./harness.mjs";

const BASE = process.env.FE || DEFAULT_URL;
let fail = 0;
const check = (name, ok, extra = "") => { console.log(`${ok ? "OK " : "FAIL"} ${name}${extra ? " — " + extra : ""}`); if (!ok) fail++; };

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
      { key: "activity_week_unlocked", value: "true" }, { key: "steps_gate_opened_at", value: ymd(70) },
      { key: "calcium_week_unlocked", value: "true" }, { key: "calcium_gate_opened_at", value: ymd(70) },
      { key: "ind_opened_at", value: ymd(70) },
      // Неделя железа открыта ТОЛЬКО ВЧЕРА — а история должна смотреть назад.
      { key: "iron_week_unlocked", value: "true" }, { key: "iron_week_opened_at", value: ymd(1) },
      { key: "ft_subscription", value: JSON.stringify({ plan: "monthly", end: Date.now() + 30 * 864e5,
          active: true, start: Date.now(), status: "paid", no_renew: false, provider: "lava" }) },
    ];
    const profile = [{ key: "profile", sex: "male", height_cm: 180,
      birth_year: new Date().getFullYear() - 45, goal: "lose", steps_planka: 9000,
      created_at: nowIso, updated_at: nowIso }];
    const goals = [{ id: "g-cal", nutrient: "Calories", key: "calories", direction: "AtMost",
      amount: 2600, unit: "Kcal", period: "Day", created_at: nowIso, updated_at: nowIso }];
    // Печень БЕЗ железа — фоновый проход её ещё не разобрал.
    const foods = [{ id: "liver", name: "Куриная печень", kcal: 137, protein: 20, fat: 6, carbs: 1,
      nutrients: {}, package_weight: null, is_recipe: false, recipe_id: null, archived: false,
      is_restaurant: false, is_snack: false, is_liquid_cal: false, is_veg_fruit: false,
      is_egg: false, is_red_meat: true, iron_mg: null, iron_absorption: null,
      created_at: nowIso, updated_at: nowIso }];
    // 63 дня еды — восемь завершённых недель плюс текущая.
    const diary = [];
    for (let i = 0; i < 63; i++) {
      diary.push({ id: "d" + i, food_id: "liver", date: ymd(i), time: null, grams: 200,
        waste_grams: 0, meal_label: "lunch", deleted: false, created_at: nowIso, updated_at: nowIso });
    }
    for (const [store, rows] of Object.entries({ app_flags, profile, goals, foods, diary })) {
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
  baseUrl: BASE, context: { serviceWorkers: "block", deviceScaleFactor: 2 },
  uid: `recompute-${Math.floor(Math.random() * 1e6)}`, seed,
});
await page.setViewportSize({ width: 430, height: 1000 });
await page.waitForTimeout(9000);

const gaugeText = () => page.locator('[data-gauge="Железо/нед"]').innerText().catch(() => "");
const indColor = () => page.locator('[data-ind="Железо"] > div')
  .evaluate((el) => getComputedStyle(el).backgroundColor).catch(() => "");
const name = (c) => c.includes("31, 164, 99") ? "зелёный"
  : c.includes("232, 133, 13") ? "оранжевый"
  : c.includes("224, 48, 79") ? "красный" : "серый";

const before = { g: (await gaugeText()).replace(/\s+/g, " "), c: name(await indColor()) };
console.log(`до появления данных: gauge «${before.g}», индикатор ${before.c}`);
check("до данных полоса пустая", /(^|\s)0(\.0)?\s*\//.test(before.g) || before.g.includes("0.0"), before.g);

// Железо приезжает задним числом — ровно так его пишет фоновый проход.
await page.evaluate(async () => {
  const uid = localStorage.getItem("user_id");
  const db = await new Promise((res, rej) => {
    const q = indexedDB.open(`hjkl-ft-${uid}`);
    q.onsuccess = () => res(q.result); q.onerror = () => rej(q.error);
  });
  const f = await new Promise((res) => {
    const rq = db.transaction(["foods"], "readonly").objectStore("foods").get("liver");
    rq.onsuccess = () => res(rq.result);
  });
  f.iron_mg = 9.0; f.iron_absorption = 0.25; f.updated_at = new Date().toISOString();
  await new Promise((res, rej) => {
    const tx = db.transaction(["foods"], "readwrite");
    tx.objectStore("foods").put(f);
    tx.oncomplete = () => res(); tx.onerror = () => rej(tx.error);
  });
  db.close();
});
// Запись мимо приложения не поднимает его сигнал, поэтому дёргаем тот же путь,
// которым это делает фоновый проход — перечитывание после возврата в приложение.
await page.evaluate(() => document.dispatchEvent(new Event("visibilitychange")));
await page.waitForTimeout(9000);

const after = { g: (await gaugeText()).replace(/\s+/g, " "), c: name(await indColor()) };
console.log(`после появления данных: gauge «${after.g}», индикатор ${after.c}`);
check("полоса пересчиталась", after.g !== before.g, `${before.g} → ${after.g}`);
check("индикатор больше не серый (история за 8 недель посчитана)", after.c !== "серый", after.c);

console.log(fail === 0 ? "\n=== ALL OK ===" : `\n=== FAILURES: ${fail} ===`);
await context.close();
await b.close();
process.exit(fail === 0 ? 0 : 1);
