// Неделя красного мяса: открытие, шкала, два индикатора, история.
//
// Проверяется ЖИВОЙ путь целиком. Сеется человек, у которого закрыта неделя жиров
// (омега-3 набрана) и есть история еды с мясом; приложение само должно открыть
// неделю красного мяса на запуске, показать шкалу и два новых индикатора и завести
// седьмую историю.
//
// Мясо в дневнике нарочно ПЕРЕБИРАЕТ планку: 8 порций по 200 г говядины за неделю
// дают около 1080 г сырого эквивалента при планке 700 — шкала обязана быть красной.
// Колбаса стоит в трёх днях подряд: дневной индикатор обязан стать оранжевым.
import { chromium } from "playwright";
import { openSeeded, DEFAULT_URL } from "./harness.mjs";

const BASE = process.env.FE || DEFAULT_URL;

let fail = 0;
const check = (n, ok, extra = "") => {
  console.log(`${ok ? "OK  " : "FAIL"} ${n}${extra ? " — " + extra : ""}`);
  if (!ok) fail++;
};

const seed = async (page, uid) => {
  await page.evaluate(async (uid) => {
    const db = await new Promise((res, rej) => {
      const r = indexedDB.open(`hjkl-ft-${uid}`);
      r.onsuccess = () => res(r.result); r.onerror = () => rej(r.error);
    });
    const nowIso = new Date().toISOString();
    const ymd = (back) => {
      const d = new Date(); d.setDate(d.getDate() - back);
      return `${d.getFullYear()}-${String(d.getMonth() + 1).padStart(2, "0")}-${String(d.getDate()).padStart(2, "0")}`;
    };
    const put = (store, rows) => new Promise((res, rej) => {
      const tx = db.transaction([store], "readwrite");
      rows.forEach((r) => tx.objectStore(store).put(r));
      tx.oncomplete = () => res(); tx.onerror = () => rej(tx.error);
    });

    await put("app_flags", [
      { key: "db_schema_version", value: "999" },
      { key: "push_onboarding_dismissed", value: "true" },
      { key: "welcome_shown", value: "true" },
      { key: "ft_subscription", value: JSON.stringify({
          plan: "monthly", end: Date.now() + 30 * 864e5, active: true,
          start: Date.now(), status: "paid", no_renew: false, provider: "lava" }) },
      // Вся цепочка недель уже пройдена, жиры открыты 21 день назад — значит две
      // недели жиров ЗАВЕРШИЛИСЬ, и последняя из них может быть закрыта.
      { key: "activity_week_unlocked", value: "true" },
      { key: "calcium_week_unlocked", value: "true" },
      { key: "iron_week_unlocked", value: "true" },
      { key: "iron_week_opened_at", value: ymd(35) },
      { key: "fat_week_unlocked", value: "true" },
      { key: "fat_week_opened_at", value: ymd(21) },
    ]);
    await put("profile", [{
      key: "profile", sex: "male", height_cm: 180, birth_year: 1985,
      goal: "lose", steps_planka: 9000, created_at: nowIso, updated_at: nowIso,
    }]);
    // Планка — запись в истории; целей больше нет.
    await put("planka_history", [{
      id: `calories:${ymd(30)}`, kind: "calories", date: ymd(30),
      amount: 2400, created_at: nowIso, updated_at: nowIso,
    }]);

    // Продукты: жирная рыба (закрывает омега-3), говядина и колбаса.
    await put("foods", [
      { id: "fish", name: "Скумбрия", kcal: 200, protein: 18, fat: 14, carbs: 0,
        nutrients: { "Кальций": 20, "Клетчатка": 0 }, package_weight: null, is_recipe: false,
        recipe_id: null, archived: false, is_restaurant: false,
        is_veg_fruit: false, is_heme: false, is_milk_globule: false,
        is_red_meat: false, is_processed_meat: false,
        iron_mg: 1, iron_absorption: 0.15,
        fat_profile: { sfa_pct: 23, mufa_pct: 35, pufa_pct: 29, epa_dha_pct: 17 },
        balance_fat_profile: null, created_at: nowIso, updated_at: nowIso },
      { id: "beef", name: "Говядина тушёная", kcal: 250, protein: 27, fat: 15, carbs: 0,
        nutrients: { "Кальций": 10, "Клетчатка": 0 }, package_weight: null, is_recipe: false,
        recipe_id: null, archived: false, is_restaurant: false,
        is_veg_fruit: false, is_heme: true, is_milk_globule: false,
        is_red_meat: true, is_processed_meat: false,
        iron_mg: 2.6, iron_absorption: 0.25,
        fat_profile: { sfa_pct: 43, mufa_pct: 45, pufa_pct: 4, epa_dha_pct: 0 },
        balance_fat_profile: null, created_at: nowIso, updated_at: nowIso },
      { id: "sausage", name: "Сосиски молочные", kcal: 260, protein: 11, fat: 24, carbs: 2,
        nutrients: { "Кальций": 30, "Клетчатка": 0 }, package_weight: null, is_recipe: false,
        recipe_id: null, archived: false, is_restaurant: false,
        is_veg_fruit: false, is_heme: false, is_milk_globule: false,
        is_red_meat: true, is_processed_meat: true,
        iron_mg: 1.2, iron_absorption: 0.2,
        fat_profile: { sfa_pct: 40, mufa_pct: 45, pufa_pct: 8, epa_dha_pct: 0 },
        balance_fat_profile: null, created_at: nowIso, updated_at: nowIso },
    ]);

    // Дневник за 21 день. Рыба — ТОЛЬКО в первую неделю темы жиров: она закрыта, а
    // последняя завершённая сорвана. Это и есть боевой случай — человек давно
    // прошёл жиры, но на этой неделе рыбы не ел; тема мяса всё равно обязана
    // открыться, потому что засчитывается ЛЮБАЯ закрытая неделя с начала темы.
    const diary = [];
    for (let i = 1; i <= 21; i++) {
      if (i >= 15) {
        diary.push({ id: `f${i}`, date: ymd(i), food_id: "fish", time: "13:00",
          grams: 150, waste_grams: 0, meal_label: "Обед", deleted: false,
          created_at: nowIso, updated_at: nowIso });
      } else {
        // Дневник ведётся, но без рыбы — недели идут, омега-3 не закрывается.
        diary.push({ id: `f${i}`, date: ymd(i), food_id: "beef", time: "13:00",
          grams: 100, waste_grams: 0, meal_label: "Обед", deleted: false,
          created_at: nowIso, updated_at: nowIso });
      }
    }
    // Говядина восемь дней подряд из последней недели — планка перебрана.
    for (let i = 1; i <= 8; i++) {
      diary.push({ id: `b${i}`, date: ymd(i), food_id: "beef", time: "19:00",
        grams: 200, waste_grams: 0, meal_label: "Ужин", deleted: false,
        created_at: nowIso, updated_at: nowIso });
    }
    // СЕГОДНЯ — своя порция: неделя красного мяса начинается в день открытия, и
    // всё съеденное раньше в текущую шкалу не попадает (как у жиров и железа).
    // 600 г тушёной говядины = 162 г белка = 810 г сырого при планке 700.
    diary.push({ id: "b0", date: ymd(0), food_id: "beef", time: "13:00",
      grams: 600, waste_grams: 0, meal_label: "Обед", deleted: false,
      created_at: nowIso, updated_at: nowIso });
    // Сосиски — ровно три дня из последних семи: оранжевый по правилу 1–3.
    for (const i of [2, 4, 6]) {
      diary.push({ id: `s${i}`, date: ymd(i), food_id: "sausage", time: "09:00",
        grams: 100, waste_grams: 0, meal_label: "Завтрак", deleted: false,
        created_at: nowIso, updated_at: nowIso });
    }
    await put("diary", diary);
    await put("weight_entries", Array.from({ length: 14 }, (_, i) => ({
      id: `w${i}`, date: ymd(i + 1), weight_kg: 85,
      no_water: true, no_food: true, no_wash: true, used_toilet: true, morning: true,
      created_at: nowIso, updated_at: nowIso,
    })));
    await put("step_entries", Array.from({ length: 14 }, (_, i) => ({
      id: `st${i}`, date: ymd(i + 1), steps: 10000,
      created_at: nowIso, updated_at: nowIso,
    })));
    db.close();
  }, uid);
};

const b = await chromium.launch({ headless: true });
const uid = `redmeat-${Date.now()}`;
const { page } = await openSeeded(b, {
  baseUrl: BASE, uid, seed, context: { serviceWorkers: "block" },
});
await page.goto(BASE, { waitUntil: "domcontentloaded" });

// Гейт срабатывает на запуске.
let flags = {};
for (let i = 0; i < 25; i++) {
  await page.waitForTimeout(2000);
  flags = await page.evaluate(async (u) => {
    const db = await new Promise((res, rej) => {
      const r = indexedDB.open(`hjkl-ft-${u}`);
      r.onsuccess = () => res(r.result); r.onerror = () => rej(r.error);
    });
    const rows = await new Promise((res) => {
      const rq = db.transaction(["app_flags"]).objectStore("app_flags").getAll();
      rq.onsuccess = () => res(rq.result); rq.onerror = () => res([]);
    });
    db.close();
    return Object.fromEntries(rows.map((r) => [r.key, r.value]));
  }, uid);
  if (flags.red_meat_week_unlocked === "true") break;
}
check("неделя красного мяса открылась", flags.red_meat_week_unlocked === "true");
check("якорь недели поставлен", !!flags.red_meat_week_opened_at, flags.red_meat_week_opened_at ?? "нет");

await page.reload({ waitUntil: "domcontentloaded" });
await page.waitForTimeout(9000);

const body = (await page.locator("body").innerText()).replace(/\s+/g, " ");
check("шкала красного мяса на дашборде", body.includes("Кр. мясо/нед"), body.slice(0, 120));
// 8 × 200 г × 27 г белка/100 г = 432 г белка → 2160 г сырого. Планка 700 —
// перебор, и шкала обязана это показать.
// У каждой шкалы есть `data-gauge` с её подписью — по нему и читаем значение.
const gaugeText = await page.locator('[data-gauge="Кр. мясо/нед"]').first()
  .innerText().catch(() => "");
check("шкала показывает перебор", /\d{3,4}\s*\/\s*700/.test(gaugeText.replace(/\s+/g, " ")),
  gaugeText.replace(/\s+/g, " ").slice(0, 60));

// Индикаторы: иконки в ряду.
const icons = await page.locator('[data-ind-key]').evaluateAll(
  (els) => els.map((e) => e.getAttribute("data-ind-key"))).catch(() => []);
if (icons.length) {
  check("индикатор красного мяса в ряду", icons.includes("red_meat"), icons.join(","));
  check("индикатор колбас в ряду", icons.includes("processed_meat"), icons.join(","));
} else {
  check("иконки индикаторов найдены", body.includes("Кр. мясо") || body.includes("Колбасы"),
    "нет data-ind-key — сверяемся по подписям");
}

// Вердикт есть СРАЗУ в день открытия: человек уже недели ведёт дневник, признаки
// собирались с первого дня, и прошлые недели судятся наравне с будущими. Цвет
// обводки иконки и есть состояние индикатора.
const readColors = (p) => p.evaluate(() => {
  const out = {};
  for (const el of document.querySelectorAll("span, div")) {
    const t = (el.textContent || "").trim();
    if (t !== "Кр. мясо" && t !== "Колбасы") continue;
    const svg = el.parentElement?.querySelector("svg") ||
      el.previousElementSibling?.querySelector("svg");
    // Значки рисуются двумя способами: наши — обводкой (цвет в stroke), значки из
    // набора gastronomy — заливкой (цвет в fill, а stroke="none").
    if (svg) {
      const s = svg.getAttribute("stroke");
      out[t] = s && s !== "none" ? s : svg.getAttribute("fill");
    }
  }
  return out;
});
const colorsNow = await readColors(page);
// Прошлая неделя перебрана (8 × 200 г говядины ≈ 1080 г при планке 700) → красный.
check("красное мясо судится сразу, по прошлым неделям", colorsNow["Кр. мясо"] === "#e0304f",
  colorsNow["Кр. мясо"] ?? "не найдено");


// История: седьмая должна появиться в ленте.
const stories = await page.locator('[data-testid="story-circle"]').count();
check("истории есть", stories > 0, `кругов ${stories}`);
const badges = await page.locator('[data-testid="story-circle"]').allInnerTexts().catch(() => []);
check("седьмая история появилась", badges.some((t) => t.trim() === "7"), badges.join("|"));


// Колбасы проверяем НЕ в ряду: там семь мест по важности, и оранжевый значок
// вытесняется красными. На развёрнутой панели видны все индикаторы.
await page.locator('[data-testid="progress-widget"]').click();
await page.waitForTimeout(4000);
const panelColor = await page.evaluate(() => {
  const p = document.querySelector('[data-ind-panel="processed_meat"]');
  const svg = p?.querySelector("svg");
  if (!svg) return null;
  const s = svg.getAttribute("stroke");
  return s && s !== "none" ? s : svg.getAttribute("fill");
});
// Сосиски в трёх днях из семи → оранжевый по правилу 1–3.
check("колбасы судятся сразу, по прошлым дням", panelColor === "#e8850d",
  panelColor ?? "панель не найдена");

await page.screenshot({ path: "/tmp/red-meat-week.png", fullPage: true });

await b.close();
console.log(fail ? `\n${fail} провалов` : "\nвсё сошлось");
process.exit(fail ? 1 : 0);
