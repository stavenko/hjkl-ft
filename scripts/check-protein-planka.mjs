// Планка по белку = кривая от калорийной планки, зажатая двумя границами.
//
// Правило: до 1800 ккал белок берётся долей 30 % (в грамме белка 4 ккал), выше —
// по степенной кривой, где доля падает, а граммы всё равно растут. И не ниже
// 1,6 г на кг БЕЗЖИРОВОЙ массы (Deurenberg), не выше 2,2 г на кг ПОЛНОГО веса.
// Пересчёт — вместе с калорийной планкой.
//
// Один и тот же человек (женщина 1984 г. р., 180 см, 64.5 кг) с тремя разными
// калорийными планками должен получить три разных ответа — ровно по трём ветвям:
//
//   ИМТ 19.91 → жир 28.15 % → безжировая 46.34 кг
//   пол  = 1.6 × 46.34 = 74.1 г      потолок = 2.2 × 64.5 = 141.9 г
//
//   1800 ккал → точка перегиба, 135 г   — внутри границ
//   2600 ккал → 157 г, выше потолка     — 142 г
//    900 ккал →  68 г, ниже пола        —  74 г
//
// Проверяется и то, что показывает виджет, и то, что записано в ИСТОРИЮ планок:
// день судится по истории, и разъехаться они не должны. База сеется версией 8,
// чтобы прогналась миграция 9, которая и пишет новое значение.
import { chromium } from "playwright";
import { openSeeded, DEFAULT_URL } from "./harness.mjs";

const BASE = process.env.FE || DEFAULT_URL;

let fail = 0;
const check = (n, ok, extra = "") => {
  console.log(`${ok ? "OK  " : "FAIL"} ${n}${extra ? " — " + extra : ""}`);
  if (!ok) fail++;
};

const CASES = [
  { kcal: 1800, want: 135, why: "точка перегиба, внутри границ" },
  { kcal: 2600, want: 142, why: "упирается в потолок 2,2 г/кг веса" },
  { kcal: 900, want: 74, why: "упирается в пол 1,6 г/кг безжировой массы" },
];

// Кривая из profile.rs, слово в слово: до перегиба — доля, после — степень.
const E0 = 1800, P0 = 135, K = -0.5850;
const proteinFromKcal = (kcal) =>
  kcal <= E0 ? P0 * kcal / E0 : P0 * Math.pow(kcal / E0, 1 + K);

const seed = (kcal) => async (page, uid) => {
  await page.evaluate(async ({ uid, kcal }) => {
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
      // Версия 8 — чтобы прогналась миграция 9 (она и пишет новую планку белка).
      { key: "db_schema_version", value: "8" },
      { key: "push_onboarding_dismissed", value: "true" },
      { key: "welcome_shown", value: "true" },
      { key: "ind_opened_at", value: ymd(40) },
      { key: "ft_subscription", value: JSON.stringify({
          plan: "monthly", end: Date.now() + 30 * 864e5, active: true,
          start: Date.now(), status: "paid", no_renew: false, provider: "lava" }) },
    ];
    const profile = [{ key: "profile", sex: "female", height_cm: 180,
      birth_year: 1984, goal: "lose", steps_planka: 9000,
      created_at: nowIso, updated_at: nowIso }];
    const goals = [{ id: "g-cal", nutrient: "Calories", key: "calories",
      direction: "AtMost", amount: kcal, unit: "Kcal", period: "Day",
      created_at: nowIso, updated_at: nowIso }];
    const weight_entries = [{ id: "w0", date: ymd(1), weight_kg: 64.5, no_water: true,
      no_food: true, no_wash: true, used_toilet: true, morning: true,
      created_at: nowIso, updated_at: nowIso }];
    // Старая запись в истории — ровно та, что оставило прежнее правило (1,6 г/кг
    // безжировой массы). Миграция обязана переписать её сегодняшним значением.
    const planka_history = [{ id: "protein:" + ymd(30), kind: "protein", date: ymd(30),
      amount: 74, created_at: nowIso, updated_at: nowIso }];
    const foods = [{ id: "chicken", name: "Курица", kcal: 165, protein: 31, fat: 3.6,
      carbs: 0, nutrients: {}, package_weight: null, is_recipe: false, recipe_id: null,
      archived: false, is_restaurant: false, is_snack: false, is_liquid_cal: false,
      is_veg_fruit: false, is_egg: false, is_red_meat: false, is_heme: false,
      iron_mg: 1.0, iron_absorption: 0.05, fat_profile: null,
      created_at: nowIso, updated_at: nowIso }];
    const diary = [{ id: "d0", food_id: "chicken", date: ymd(0), time: null, grams: 200,
      waste_grams: 0, meal_label: "lunch", deleted: false,
      created_at: nowIso, updated_at: nowIso }];
    const avail = Array.from(db.objectStoreNames);
    for (const [store, rows] of Object.entries({
      app_flags, profile, goals, weight_entries, planka_history, foods, diary })) {
      if (!avail.includes(store)) continue;
      await new Promise((res, rej) => {
        const tx = db.transaction([store], "readwrite");
        for (const row of rows) tx.objectStore(store).put(row);
        tx.oncomplete = () => res(); tx.onerror = () => rej(tx.error);
      });
    }
    db.close();
  }, { uid, kcal });
};

// Цель со шкалы «Белок»: «съедено / ЦЕЛЬ г».
const gaugeTarget = async (page) => {
  const t = await page.locator('[data-gauge="Белок"]').first().innerText().catch(() => "");
  const m = /([\d.,]+)\s*\/\s*([\d.,]+)/.exec(t.replace(/\s+/g, " "));
  return m ? Number(m[2].replace(",", ".")) : null;
};

const plankaHistory = (page, uid) => page.evaluate(async (u) => {
  const db = await new Promise((res, rej) => {
    const r = indexedDB.open(`hjkl-ft-${u}`);
    r.onsuccess = () => res(r.result); r.onerror = () => rej(r.error);
  });
  const all = await new Promise((res) => {
    const rq = db.transaction(["planka_history"]).objectStore("planka_history").getAll();
    rq.onsuccess = () => res(rq.result); rq.onerror = () => res([]);
  });
  db.close();
  return all.filter((e) => e.kind === "protein").map((e) => [e.date, e.amount]);
}, uid);

const today = new Date();
const YMD0 = `${today.getFullYear()}-${String(today.getMonth() + 1).padStart(2, "0")}-${String(today.getDate()).padStart(2, "0")}`;

const b = await chromium.launch({ headless: true });
for (const c of CASES) {
  const uid = `protein-${c.kcal}-${Math.floor(Math.random() * 1e6)}`;
  const { context, page } = await openSeeded(b, {
    baseUrl: BASE, context: { serviceWorkers: "block" }, uid, seed: seed(c.kcal),
  });
  await page.reload({ waitUntil: "domcontentloaded" });
  await page.waitForTimeout(12000);

  const shown = await gaugeTarget(page);
  check(`${c.kcal} ккал → шкала показывает ${c.want} г (${c.why})`, shown === c.want,
    `показано ${shown}`);

  // На сегодня действует ПОСЛЕДНЯЯ запись с датой ≤ сегодня. Отдельной строки за
  // сегодня может и не быть: `record_planka` не пишет дубль, если величина не
  // изменилась (случай 900 ккал — там новая планка совпадает со старой).
  const hist = await plankaHistory(page, uid);
  const effective = hist.filter(([d]) => d <= YMD0).sort((a, b) => (a[0] < b[0] ? -1 : 1)).pop();
  check(`${c.kcal} ккал → в истории планок на сегодня действует ${c.want}`,
    effective && effective[1] === c.want, JSON.stringify(hist));
  check(`${c.kcal} ккал → прошлые дни не переписаны`,
    hist.some(([d, a]) => d !== YMD0 && a === 74), JSON.stringify(hist));

  // Пояснение «?» обязано назвать ту же цифру и ту ветвь, по которой она вышла:
  // иначе человек видит планку, которую нечем объяснить.
  await page.locator('[data-testid="progress-widget"]').dispatchEvent("pointerup");
  await page.waitForTimeout(3000);
  await page.locator('[data-ind-panel="protein"] button[aria-label="?"]').click();
  await page.waitForTimeout(500);
  const hint = await page.locator('[style*="z-index: 41"] span').innerText().catch(() => "");
  check(`${c.kcal} ккал → пояснение называет планку ${c.want} г`,
    hint.includes(`Ваша планка по белку: ${c.want} г`), hint.slice(0, 120));
  check(`${c.kcal} ккал → пояснение объясняет, откуда взялась именно эта цифра`,
    c.kcal === 1800
      ? hint.includes("30 % калорийной планки")
      : hint.includes(c.kcal === 2600 ? "2,2 г на кг веса" : "ниже физиологического минимума"),
    hint.slice(0, 200));

  await context.close();
}
// ── 4. планка по белку пересчитывается ВМЕСТЕ с калорийной ──────────────────
// Здесь никакой миграции: база свежая (версия 999), калорийной планки нет вовсе.
// Приложение само выставляет первую планку, когда неделя наблюдения набрана
// (7 дней еды + 7 взвешиваний + 7 дней шагов) — то есть срабатывает боевой путь
// `set_calorie_goal`. Проверяем, что белок поехал следом и ровно по правилу.
{
  const uid = `protein-auto-${Math.floor(Math.random() * 1e6)}`;
  const { context, page } = await openSeeded(b, {
    baseUrl: BASE, context: { serviceWorkers: "block" }, uid,
    seed: async (page, uid) => {
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
          { key: "ft_subscription", value: JSON.stringify({
              plan: "monthly", end: Date.now() + 30 * 864e5, active: true,
              start: Date.now(), status: "paid", no_renew: false, provider: "lava" }) },
        ];
        const profile = [{ key: "profile", sex: "female", height_cm: 180,
          birth_year: 1984, goal: "lose", steps_planka: 9000,
          created_at: nowIso, updated_at: nowIso }];
        const foods = [{ id: "meal", name: "Обед", kcal: 500, protein: 30, fat: 20,
          carbs: 50, nutrients: {}, package_weight: null, is_recipe: false,
          recipe_id: null, archived: false, is_restaurant: false, is_snack: false,
          is_liquid_cal: false, is_veg_fruit: false, is_egg: false, is_red_meat: false,
          is_heme: false, iron_mg: 1.0, iron_absorption: 0.05, fat_profile: null,
          created_at: nowIso, updated_at: nowIso }];
        const diary = [], weight_entries = [], step_entries = [];
        for (let i = 0; i < 8; i++) {
          diary.push({ id: "d" + i, food_id: "meal", date: ymd(i), time: null, grams: 360,
            waste_grams: 0, meal_label: "lunch", deleted: false,
            created_at: nowIso, updated_at: nowIso });
          weight_entries.push({ id: "w" + i, date: ymd(i), weight_kg: 64.5,
            no_water: true, no_food: true, no_wash: true, used_toilet: true,
            morning: true, created_at: nowIso, updated_at: nowIso });
          step_entries.push({ id: "s" + i, date: ymd(i), steps: 9500,
            created_at: nowIso, updated_at: nowIso });
        }
        const avail = Array.from(db.objectStoreNames);
        for (const [store, rows] of Object.entries({
          app_flags, profile, foods, diary, weight_entries, step_entries })) {
          if (!avail.includes(store)) continue;
          await new Promise((res, rej) => {
            const tx = db.transaction([store], "readwrite");
            for (const row of rows) tx.objectStore(store).put(row);
            tx.oncomplete = () => res(); tx.onerror = () => rej(tx.error);
          });
        }
        db.close();
      }, { uid });
    },
  });
  await page.reload({ waitUntil: "domcontentloaded" });
  await page.waitForTimeout(15000);

  const kcal = await page.evaluate(async (u) => {
    const db = await new Promise((res, rej) => {
      const r = indexedDB.open(`hjkl-ft-${u}`);
      r.onsuccess = () => res(r.result); r.onerror = () => rej(r.error);
    });
    const all = await new Promise((res) => {
      const rq = db.transaction(["goals"]).objectStore("goals").getAll();
      rq.onsuccess = () => res(rq.result); rq.onerror = () => res([]);
    });
    db.close();
    return (all.find((g) => g.nutrient === "Calories") || {}).amount ?? null;
  }, uid);
  check("приложение выставило первую калорийную планку", kcal > 0, String(kcal));

  // Те же границы: пол 74 г, потолок 142 г.
  const want = Math.round(Math.min(Math.max(proteinFromKcal(kcal ?? 0), 74.15), 141.9));
  const shown = await gaugeTarget(page);
  check(`планка ${kcal} ккал → белок ${want} г, без всякой миграции`, shown === want,
    `показано ${shown}`);
  const hist = await plankaHistory(page, uid);
  const effective = hist.filter(([d]) => d <= YMD0).sort((a, b) => (a[0] < b[0] ? -1 : 1)).pop();
  check("новая планка по белку записана в историю", effective && effective[1] === want,
    JSON.stringify(hist));

  await context.close();
}
await b.close();

console.log(fail ? `\n${fail} провалов` : "\nвсё сошлось");
process.exit(fail ? 1 : 0);
