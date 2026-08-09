// Гейт жиров и две недельные шкалы.
//
// Проверяется три вещи:
//   1. пока планка железа не закрыта, жиров на дашборде НЕТ вовсе;
//   2. закрытая недельная планка железа открывает жиры — флаг ставится на запуске,
//      неделя жира привязывается к сегодняшнему дню;
//   3. шкалы считают то, что съедено: граммы кислот = доля профиля × наш жир, а
//      отношение (МНЖК+ПНЖК)/НЖК — из СУММ за неделю.
//
// Числа взяты не с потолка: они посчитаны из засеянных профилей вручную и вписаны
// в ожидания — иначе проверка меряла бы саму себя.
import { chromium } from "playwright";
import { openSeeded, DEFAULT_URL } from "./harness.mjs";

const BASE = process.env.FE || DEFAULT_URL;

let fail = 0;
const check = (n, ok, extra = "") => {
  console.log(`${ok ? "OK " : "FAIL"} ${n}${extra ? " — " + extra : ""}`);
  if (!ok) fail++;
};
const near = (a, b, eps = 0.06) => a != null && Math.abs(a - b) < eps;

// ── что сеем ────────────────────────────────────────────────────────────────
// Профили — те же, что даёт замеренный промпт на этих продуктах.
const FOODS = [
  { id: "olive", name: "Оливковое масло", kcal: 884, fat: 100,
    fp: { sfa_pct: 15, mufa_pct: 70, pufa_pct: 15, epa_dha_pct: 0 } },
  { id: "mack", name: "Скумбрия", kcal: 191, fat: 13.9,
    fp: { sfa_pct: 24, mufa_pct: 38, pufa_pct: 26, epa_dha_pct: 18 } },
  { id: "butter", name: "Сливочное масло", kcal: 717, fat: 82,
    fp: { sfa_pct: 65, mufa_pct: 26, pufa_pct: 4, epa_dha_pct: 0 } },
  // Печень — чтобы закрыть недельную планку железа: 9 мг при усвоении 0.25.
  { id: "liver", name: "Печень куриная", kcal: 137, fat: 6, iron: [9.0, 0.25],
    fp: { sfa_pct: 29, mufa_pct: 44, pufa_pct: 27, epa_dha_pct: 0 } },
];
const EATEN_TODAY = [["olive", 30], ["mack", 150], ["butter", 20]];

// Ожидания, посчитанные вручную:
//   оливковое  30 г → жир 30.00 → НЖК 4.50  МНЖК 21.00 ПНЖК 4.50
//   скумбрия  150 г → жир 20.85 → НЖК 5.00  МНЖК 7.92  ПНЖК 5.42  EPA+DHA 3.75
//   сливочное  20 г → жир 16.40 → НЖК 10.66 МНЖК 4.26  ПНЖК 0.66
const WANT = { epa_dha: 3.75, ratio: (33.19 + 10.58) / 20.16 };

const seed = (opts) => async (page, uid) => {
  await page.evaluate(async ({ uid, FOODS, EATEN_TODAY, opts }) => {
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
      { key: "calcium_week_unlocked", value: "true" },
      { key: "ind_opened_at", value: ymd(40) },
      { key: "iron_week_unlocked", value: "true" },
      // Неделя железа открылась 7 дней назад → последняя ЗАВЕРШЁННАЯ неделя это
      // дни −7…−1, и именно её закрывает засеянная печень.
      { key: "iron_week_opened_at", value: ymd(7) },
      { key: "ft_subscription", value: JSON.stringify({
          plan: "monthly", end: Date.now() + 30 * 864e5, active: true,
          start: Date.now(), status: "paid", no_renew: false, provider: "lava" }) },
    ];
    if (opts.fatsAlreadyOpen) {
      app_flags.push({ key: "fat_week_unlocked", value: "true" });
      app_flags.push({ key: "fat_week_opened_at", value: ymd(0) });
    }
    const profile = [{ key: "profile", sex: "male", height_cm: 180,
      birth_year: new Date().getFullYear() - 45, goal: "lose", steps_planka: 9000,
      created_at: nowIso, updated_at: nowIso }];
    const goals = [{ id: "g-cal", nutrient: "Calories", key: "calories", direction: "AtMost",
      amount: 2600, unit: "Kcal", period: "Day", created_at: nowIso, updated_at: nowIso }];
    const foods = FOODS.map((f) => ({
      id: f.id, name: f.name, kcal: f.kcal, protein: 10, fat: f.fat, carbs: 0,
      nutrients: {}, package_weight: null, is_recipe: false, recipe_id: null,
      archived: false, is_restaurant: false, is_snack: false, is_liquid_cal: false,
      is_veg_fruit: false, is_egg: false, is_red_meat: false, is_heme: false,
      iron_mg: f.iron ? f.iron[0] : 0, iron_absorption: f.iron ? f.iron[1] : 0.05,
      fat_profile: opts.withProfiles ? f.fp : null,
      created_at: nowIso, updated_at: nowIso,
    }));
    const diary = [];
    // Печень каждый день ЗАВЕРШЁННОЙ недели железа: 60 г × 9 мг × 0.25 = 1.35 мг
    // усвоенного в день, за семь дней 9.45 мг — выше мужской нормы 10.1? нет, ниже.
    // Поэтому кладём по 100 г: 2.25 мг/день, 15.75 мг за неделю — планка закрыта.
    for (let i = 1; i <= 7; i++) {
      diary.push({ id: "dl" + i, food_id: "liver", date: ymd(i), time: null,
        grams: opts.closeIronWeek ? 100 : 5, waste_grams: 0, meal_label: "lunch",
        deleted: false, created_at: nowIso, updated_at: nowIso });
    }
    for (const [food_id, grams] of EATEN_TODAY) {
      diary.push({ id: "dt-" + food_id, food_id, date: ymd(0), time: null, grams,
        waste_grams: 0, meal_label: "lunch", deleted: false,
        created_at: nowIso, updated_at: nowIso });
    }
    const avail = Array.from(db.objectStoreNames);
    for (const [store, rows] of Object.entries({ app_flags, profile, goals, foods, diary })) {
      if (!avail.includes(store)) continue;
      await new Promise((res, rej) => {
        const tx = db.transaction([store], "readwrite");
        for (const row of rows) tx.objectStore(store).put(row);
        tx.oncomplete = () => res(); tx.onerror = () => rej(tx.error);
      });
    }
    db.close();
  }, { uid, FOODS, EATEN_TODAY, opts });
};

const gauges = (page) =>
  page.$$eval("[data-gauge]", (els) => els.map((e) => e.getAttribute("data-gauge")));
const indicators = (page) =>
  page.$$eval("[data-ind]", (els) => els.map((e) => e.getAttribute("data-ind")));
const gaugeValue = async (page, label) => {
  const t = await page.locator(`[data-gauge="${label}"]`).innerText().catch(() => "");
  const m = /([\d.,]+)\s*\/\s*([\d.,]+)/.exec(t.replace(/\s+/g, " "));
  return m ? Number(m[1].replace(",", ".")) : null;
};
const flags = (page, uid) => page.evaluate(async (u) => {
  const db = await new Promise((res, rej) => {
    const r = indexedDB.open(`hjkl-ft-${u}`);
    r.onsuccess = () => res(r.result); r.onerror = () => rej(r.error);
  });
  const all = await new Promise((res) => {
    const rq = db.transaction(["app_flags"]).objectStore("app_flags").getAll();
    rq.onsuccess = () => res(rq.result); rq.onerror = () => res([]);
  });
  db.close();
  return Object.fromEntries(all.map((f) => [f.key, f.value]));
}, uid);

const b = await chromium.launch({ headless: true });

// ── 1. планка железа НЕ закрыта → жиров нет ──────────────────────────────────
{
  const uid = `fats-shut-${Math.floor(Math.random() * 1e6)}`;
  const { context, page } = await openSeeded(b, {
    baseUrl: BASE, context: { serviceWorkers: "block" }, uid,
    seed: seed({ closeIronWeek: false, withProfiles: false, fatsAlreadyOpen: false }),
  });
  await page.reload({ waitUntil: "domcontentloaded" });
  await page.waitForTimeout(9000);
  const g = await gauges(page);
  const i = await indicators(page);
  const f = await flags(page, uid);
  check("планка железа не закрыта: флага жиров нет", !f.fat_week_unlocked,
    String(f.fat_week_unlocked));
  check("планка железа не закрыта: шкал по жирам нет",
    !g.some((x) => /Омега-3\/нед|Баланс жира/.test(x)), JSON.stringify(g));
  check("планка железа не закрыта: индикаторов по жирам нет",
    !i.some((x) => ["Омега-3", "Баланс"].includes(x)), JSON.stringify(i));
  const tray = await page.$$eval("[data-story-id]", (els) =>
    els.map((e) => e.getAttribute("data-story-id")));
  check("планка железа не закрыта: истории шестой недели нет", !tray.includes("week6"),
    JSON.stringify(tray));
  await context.close();
}

// ── 2. планка железа закрыта → жиры открываются сами ────────────────────────
{
  const uid = `fats-open-${Math.floor(Math.random() * 1e6)}`;
  const { context, page } = await openSeeded(b, {
    baseUrl: BASE, context: { serviceWorkers: "block" }, uid,
    seed: seed({ closeIronWeek: true, withProfiles: false, fatsAlreadyOpen: false }),
  });
  await page.reload({ waitUntil: "domcontentloaded" });
  await page.waitForTimeout(12000);
  const f = await flags(page, uid);
  const today = new Date();
  const ymd0 = `${today.getFullYear()}-${String(today.getMonth() + 1).padStart(2, "0")}-${String(today.getDate()).padStart(2, "0")}`;
  check("закрытая планка железа открыла жиры", f.fat_week_unlocked === "true",
    String(f.fat_week_unlocked));
  check("неделя жира привязана к сегодня", f.fat_week_opened_at === ymd0,
    String(f.fat_week_opened_at));
  await context.close();
}

// ── 3. открытые жиры: три шкалы и верные числа ──────────────────────────────
{
  const uid = `fats-calc-${Math.floor(Math.random() * 1e6)}`;
  const { context, page } = await openSeeded(b, {
    baseUrl: BASE, context: { serviceWorkers: "block" }, uid,
    seed: seed({ closeIronWeek: true, withProfiles: true, fatsAlreadyOpen: true }),
  });
  await page.reload({ waitUntil: "domcontentloaded" });
  await page.waitForTimeout(9000);
  const g = await gauges(page);
  const i = await indicators(page);
  check("шкалы по жирам на экране",
    ["Омега-3/нед", "Баланс жира"].every((x) => g.includes(x)), JSON.stringify(g));
  check("индикаторы по жирам в ряду",
    ["Омега-3", "Баланс"].every((x) => i.includes(x)), JSON.stringify(i));
  // История шестой недели появляется вместе с жирами — и только с ними.
  const tray = await page.$$eval("[data-story-id]", (els) =>
    els.map((e) => e.getAttribute("data-story-id")));
  check("в трее появилась история шестой недели", tray.includes("week6"), JSON.stringify(tray));
  const epa = await gaugeValue(page, "Омега-3/нед");
  check(`EPA+DHA = ${WANT.epa_dha.toFixed(2)} г`, near(epa, WANT.epa_dha), `${epa}`);
  // Баланс показывает ОТКЛОНЕНИЕ от нормы со знаком, а не само отношение, и рисуется
  // от середины: сторона полосы обязана совпадать со знаком.
  const bal = await page.locator('[data-gauge="Баланс жира"]').innerText();
  const shown = Number((/[−+-]?[\d.]+/.exec(bal.replace("−", "-")) || [""])[0]);
  const side = await page.locator('[data-gauge="Баланс жира"]').getAttribute("data-balance-side");
  const want_dev = WANT.ratio - 2.0;
  check(`отклонение = ${want_dev.toFixed(2)}`, near(shown, want_dev), `${shown}`);
  check("полоса вправо, раз отклонение положительное", side === "right", String(side));
  await page.screenshot({ path: process.env.SHOT || "/tmp/fats-gauges.png", fullPage: false });
  await context.close();
}

console.log(fail === 0 ? "\n=== ALL OK ===" : `\n=== FAILURES: ${fail} ===`);
await b.close();
process.exit(fail === 0 ? 0 : 1);
