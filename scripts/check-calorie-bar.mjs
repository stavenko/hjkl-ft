// ЦВЕТ ПОЛОСЫ КАЛОРИЙ — на живом дашборде И В ДНЕВНИКЕ.
//
// Правило: коридор ±50 ккал от планки. Недобрал больше — серый (день не закрыт),
// попал — зелёный, перебрал больше — красный. Полоса обязана говорить то же, что и
// индикатор калорий: он судит день по тому же коридору, и расхождение уже случалось
// — перебор в семь ккал красил полосу красным, пока значок оставался зелёным.
//
// Полос две: на дашборде и в шапке дневника. Рисуются они разным кодом, и правило у
// них обязано быть одно — потому проверяются обе.
//
//   node scripts/check-calorie-bar.mjs
import { chromium } from "playwright";
import { openSeeded, DEFAULT_URL } from "./harness.mjs";

const BASE = process.env.FE || DEFAULT_URL;
const PLANKA = 2950;

/// Что должно получиться при съеденном `eaten` — по правилу коридора.
const CASES = [
  { eaten: PLANKA + 7, want: "зелёный", why: "перебор в семь ккал — это попадание" },
  { eaten: PLANKA - 40, want: "зелёный", why: "недобор внутри коридора" },
  { eaten: PLANKA + 80, want: "красный", why: "перебор больше коридора" },
  { eaten: PLANKA - 400, want: "серый", why: "день ещё не закрыт" },
];

/// Сев под один случай: планка `PLANKA` и ровно `eaten` ккал съеденного сегодня.
const seedFor = (eaten) => async (page, uid) => {
  await page.evaluate(
    async ({ uid, eaten, PLANKA }) => {
      const db = await new Promise((res, rej) => {
        const r = indexedDB.open(`hjkl-ft-${uid}`);
        r.onsuccess = () => res(r.result);
        r.onerror = () => rej(r.error);
      });
      const nowIso = new Date().toISOString();
      const d = new Date();
      const ymd = (back) => {
        const x = new Date(d.getFullYear(), d.getMonth(), d.getDate() - back);
        return `${x.getFullYear()}-${String(x.getMonth() + 1).padStart(2, "0")}-${String(x.getDate()).padStart(2, "0")}`;
      };
      const rows = {
        app_flags: [
          { key: "db_schema_version", value: "999" },
          { key: "welcome_shown", value: "true" },
          { key: "push_onboarding_dismissed", value: "true" },
          { key: "paywall_skipped_date", value: ymd(0) },
          { key: "ind_opened_at", value: ymd(9) },
        ],
        profile: [{ key: "profile", sex: "male", height_cm: 180,
          birth_year: new Date().getFullYear() - 40, goal: "lose", steps_planka: 9000,
          created_at: nowIso, updated_at: nowIso }],
        goals: [{ id: "g-cal", nutrient: "Calories", key: "calories", direction: "AtMost",
          amount: PLANKA, unit: "Kcal", period: "Day", created_at: nowIso, updated_at: nowIso }],
        planka_history: [{ id: "ph-1", kind: "calories", date: ymd(9), amount: PLANKA,
          created_at: nowIso, updated_at: nowIso }],
        // Один продукт в 1 ккал за грамм: сколько граммов — столько и ккал, и
        // подогнать съеденное до килокалории можно без арифметики в уме.
        foods: [{ id: "f-kcal", name: "Рацион дня", kcal: 100, protein: 5, fat: 3, carbs: 12,
          nutrients: {}, package_weight: null, is_recipe: false, recipe_id: null,
          archived: false, is_restaurant: false, is_veg_fruit: false, is_heme: false,
          is_milk_globule: false, is_red_meat: false, is_processed_meat: false, is_egg: false,
          iron_mg: 0, iron_absorption: 0.18, fat_profile: null,
          created_at: nowIso, updated_at: nowIso }],
        diary: [{ id: "d0", food_id: "f-kcal", date: ymd(0), time: null, grams: eaten,
          waste_grams: 0, meal_label: "lunch", deleted: false,
          created_at: nowIso, updated_at: nowIso }],
        weight_entries: [{ id: "w0", date: ymd(0), weight_kg: 88, no_water: false,
          no_food: false, no_wash: false, used_toilet: false, morning: true,
          created_at: nowIso, updated_at: nowIso }],
      };
      const avail = Array.from(db.objectStoreNames);
      for (const [store, list] of Object.entries(rows)) {
        if (!avail.includes(store)) continue;
        await new Promise((res, rej) => {
          const tx = db.transaction([store], "readwrite");
          for (const row of list) tx.objectStore(store).put(row);
          tx.oncomplete = () => res();
          tx.onerror = () => rej(tx.error);
        });
      }
      db.close();
    },
    { uid, eaten, PLANKA },
  );
};

/// Палитры у двух экранов РАЗНЫЕ: дашборд рисует своими цветами, дневник — темой
/// Bulma. Проверяется не оттенок, а смысл, поэтому распознаются оба набора.
const NAME = (rgb) =>
  /(224, *48)|(255, *102)/.test(rgb) ? "красный"
    : /(31, *164)|(72, *199)/.test(rgb) ? "зелёный"
    : /(154, *160)|(105, *116)/.test(rgb) ? "серый"
    : rgb;

let failed = 0;
const browser = await chromium.launch({ headless: true });
for (const c of CASES) {
  const { context, page } = await openSeeded(browser, {
    baseUrl: BASE,
    context: { serviceWorkers: "block", viewport: { width: 430, height: 932 } },
    uid: `calbar-${c.eaten}-${Math.floor(Math.random() * 1e6)}`,
    seed: seedFor(c.eaten),
  });
  await page.reload({ waitUntil: "domcontentloaded" });
  await page.waitForTimeout(7000);
  const dash = await page.evaluate(() => {
    const g = document.querySelector('[data-gauge="Калории"]');
    if (!g) return "шкалы нет";
    // Заливка — самый вложенный div с непрозрачным фоном.
    const fills = [...g.querySelectorAll("div")]
      .map((el) => getComputedStyle(el).backgroundColor)
      .filter((bg) => bg && bg !== "rgba(0, 0, 0, 0)");
    return fills[fills.length - 1] || "заливки нет";
  });

  // ДНЕВНИК: та же планка нарисована в шапке — своей вёрсткой, без data-gauge.
  await page.goto(`${BASE}/diary`, { waitUntil: "domcontentloaded" });
  await page.waitForTimeout(5000);
  const diary = await page.evaluate(() => {
    const label = [...document.querySelectorAll("span")]
      .find((el) => el.textContent.trim() === "Калории");
    if (!label) return "строки нет";
    // Заголовок и полоса лежат в одном блоке: поднимаемся к нему и берём заливку.
    const box = label.closest("div")?.parentElement;
    const fills = [...(box?.querySelectorAll("div") ?? [])]
      .map((el) => getComputedStyle(el).backgroundColor)
      .filter((bg) => bg && bg !== "rgba(0, 0, 0, 0)");
    return fills[fills.length - 1] || "заливки нет";
  });

  for (const [where, rgb] of [["дашборд", dash], ["дневник", diary]]) {
    const name = NAME(rgb);
    const ok = name === c.want;
    if (!ok) failed++;
    console.log(`${ok ? "✅" : "❌"} ${where}: ${c.eaten} при планке ${PLANKA} → ${name} ` +
      `(ждали ${c.want}: ${c.why})`);
  }
  await context.close();
}
await browser.close();
process.exit(failed ? 1 : 0);
