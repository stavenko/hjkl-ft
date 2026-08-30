// Недели гема и железа обязаны совпадать.
//
// Гистограммы у них считают одни и те же недели, поэтому и сетка недель обязана
// быть одна: у человека с четырьмя неделями дневника железо рисовало восемь
// подписей (−8…−1, четыре без столбика), а гем — четыре. Разъезд заметен глазом и
// читается как «у гема другая неделя», хотя границы недель у них общие.
import { chromium } from "playwright";
import { openSeeded, tapWidget, DEFAULT_URL } from "./harness.mjs";

const BASE = process.env.FE || DEFAULT_URL;
// Дневник ведётся четыре недели — вдвое меньше окна в восемь недель. Ровно тот
// случай, на котором сетки разъезжались.
const DIARY_DAYS = 28;

let fail = 0;
const check = (n, ok, extra = "") => {
  console.log(`${ok ? "OK " : "FAIL"} ${n}${extra ? " — " + extra : ""}`);
  if (!ok) fail++;
};

const seed = async (page, uid) => {
  await page.evaluate(async ({ uid, DIARY_DAYS }) => {
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
      { key: "ind_opened_at", value: ymd(DIARY_DAYS) },
      { key: "iron_week_unlocked", value: "true" },
      { key: "iron_week_opened_at", value: ymd(3) },
      { key: "ft_subscription", value: JSON.stringify({
          plan: "monthly", end: Date.now() + 30 * 864e5, active: true,
          start: Date.now(), status: "paid", no_renew: false, provider: "lava" }) },
    ];
    const profile = [{ key: "profile", sex: "male", height_cm: 180,
      birth_year: new Date().getFullYear() - 45, goal: "lose", steps_planka: 9000,
      created_at: nowIso, updated_at: nowIso }];
    const goals = [{ id: "g-cal", nutrient: "Calories", key: "calories",
      direction: "AtMost", amount: 2600, unit: "Kcal", period: "Day",
      created_at: nowIso, updated_at: nowIso }];
    // Планка живёт в ИСТОРИИ — она и есть цель. Без неё приложение считает,
    // что планки нет вовсе, и не показывает ни одного индикатора.
    const planka_history = [{ id: `calories:${ymd(30)}`, kind: "calories",
      date: ymd(30), amount: 2600, created_at: nowIso, updated_at: nowIso }];
    const foods = [{
      id: "liver", name: "Куриная печень", kcal: 137, protein: 20, fat: 6, carbs: 1,
      nutrients: {}, package_weight: null, is_recipe: false, recipe_id: null,
      archived: false, is_restaurant: false, is_snack: false, is_liquid_cal: false,
      is_veg_fruit: false, is_egg: false, is_red_meat: true, is_heme: true,
      iron_mg: 9.0, iron_absorption: 0.25, created_at: nowIso, updated_at: nowIso,
    }];
    const diary = [];
    for (let i = 0; i < DIARY_DAYS; i++) {
      diary.push({ id: "d" + i, food_id: "liver", date: ymd(i), time: null, grams: 130,
        waste_grams: 0, meal_label: "lunch", deleted: false, created_at: nowIso, updated_at: nowIso });
    }
    const avail = Array.from(db.objectStoreNames);
    for (const [store, rows] of Object.entries({ app_flags, profile, goals, planka_history, foods, diary })) {
      if (!avail.includes(store)) continue;
      await new Promise((res, rej) => {
        const tx = db.transaction([store], "readwrite");
        for (const row of rows) tx.objectStore(store).put(row);
        tx.oncomplete = () => res(); tx.onerror = () => rej(tx.error);
      });
    }
    db.close();
  }, { uid, DIARY_DAYS });
};

const b = await chromium.launch({ headless: true });
const { context, page } = await openSeeded(b, {
  baseUrl: BASE, context: { serviceWorkers: "block" },
  uid: `heme-series-${Math.floor(Math.random() * 1e6)}`, seed,
});
await page.reload({ waitUntil: "domcontentloaded" });
await page.waitForTimeout(5000);

// Развёрнутый вид открывается ТАПОМ по виджету планки — см. `tapWidget`.
await tapWidget(page, '[data-testid="progress-widget"]');
await page.waitForTimeout(4000);

// Подписи недель внутри панели индикатора: ищем панель по её заголовку и считаем
// в ней элементы с текстом вида «−N».
const weeks = (name) => page.evaluate((n) => {
  const head = Array.from(document.querySelectorAll("span"))
    .find((s) => s.textContent.trim() === n);
  if (!head) return null;
  const panel = head.closest("div").parentElement;
  return Array.from(panel.querySelectorAll("*"))
    .map((e) => e.childElementCount === 0 ? e.textContent.trim() : "")
    .filter((t) => /^[−-]\d+$/.test(t));
}, name);

const fe = await weeks("Железо");
const heme = await weeks("Гем");
console.log("железо:", JSON.stringify(fe), "\nгем:   ", JSON.stringify(heme));
check("панель железа найдена", Array.isArray(fe) && fe.length > 0, String(fe));
check("панель гема найдена", Array.isArray(heme) && heme.length > 0, String(heme));
check("недели совпадают", JSON.stringify(fe) === JSON.stringify(heme),
  `${fe?.length} против ${heme?.length}`);

await page.screenshot({ path: process.env.SHOT || "/tmp/heme-series.png", fullPage: true });
console.log(fail === 0 ? "\n=== ALL OK ===" : `\n=== FAILURES: ${fail} ===`);
await context.close();
await b.close();
process.exit(fail === 0 ? 0 : 1);
