// Мигающая подсветка для кадра истории про КРАСНОЕ МЯСО. Тот же приём, что у
// железа, гема и жиров: сеем состояние, снимаем виджет и обводим нужную шкалу с её
// индикатором.
//
// Состояние подобрано так, чтобы кадр читался с одного взгляда: ВСЁ ОСТАЛЬНОЕ
// зелёное — человек уже прошёл предыдущие главы и держит их, — а красное мясо
// перебрано. Тогда единственное красное пятно на карточке и есть то, о чём кадр.
//
// Дневные индикаторы (калории, белок, овощи, кальций, шаги) закрываются через их
// КЭШ ВЕРДИКТОВ (`ind_*`): он и есть источник цвета для завершённых дней, а
// подбирать еду так, чтобы каждый день попал ровно в пять норм разом, — работа
// ради того же результата. Недельные (железо, гем, омега-3, баланс) считаются по
// дневнику на лету, поэтому им нужна настоящая еда: печень, скумбрия, масло.
import { chromium } from "playwright";
import { openSeeded, DEFAULT_URL } from "./harness.mjs";
import { makeWidgetGif } from "./highlight-gif.mjs";
import path from "node:path";

const BASE = process.env.FE || DEFAULT_URL;
const ROOT = path.resolve(import.meta.dirname, "..");
const OUT = process.env.OUT || path.join(ROOT, "frontend/story-img/red-meat-highlight.gif");
const OUT_PROCESSED = process.env.OUT_PROCESSED
  || path.join(ROOT, "frontend/story-img/processed-meat-highlight.gif");

const seed = async (page, uid, opts) => {
  await page.evaluate(async ({ uid, opts }) => {
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
      { key: "ind_opened_at", value: ymd(40) },
      { key: "activity_week_unlocked", value: "true" },
      { key: "steps_gate_opened_at", value: ymd(30) },
      { key: "calcium_week_unlocked", value: "true" },
      { key: "calcium_gate_opened_at", value: ymd(25) },
      { key: "iron_week_unlocked", value: "true" },
      { key: "iron_week_opened_at", value: ymd(26) },
      { key: "fat_week_unlocked", value: "true" },
      { key: "fat_week_opened_at", value: ymd(14) },
      // Неделя мяса идёт четвёртый день, и одна её неделя уже завершилась —
      // иначе индикатор серый и вытесняется из ряда, где помещаются семь.
      { key: "red_meat_week_unlocked", value: "true" },
      { key: "red_meat_week_opened_at", value: ymd(10) },
      { key: "ft_subscription", value: JSON.stringify({
          plan: "monthly", end: Date.now() + 30 * 864e5, active: true,
          start: Date.now(), status: "paid", no_renew: false, provider: "lava" }) },
    ];
    const profile = [{ key: "profile", sex: "male", height_cm: 180,
      birth_year: new Date().getFullYear() - 40, goal: "lose", steps_planka: 9000,
      created_at: nowIso, updated_at: nowIso }];
    const goals = [
      { id: "g-cal", nutrient: "Calories", key: "calories", direction: "AtMost",
        amount: 2400, unit: "Kcal", period: "Day", created_at: nowIso, updated_at: nowIso },
      { id: "g-ca", nutrient: "Кальций", key: "calcium", direction: "AtLeast",
        amount: 1000, unit: "Mg", period: "Day", created_at: nowIso, updated_at: nowIso },
    ];
    const food = (id, name, kcal, protein, fat, fp, extra) => ({
      id, name, kcal, protein, fat, carbs: 0, nutrients: { "Кальций": 200, "Клетчатка": 5 },
      package_weight: null, is_recipe: false, recipe_id: null, archived: false,
      is_restaurant: false, is_veg_fruit: false, is_heme: false, is_milk_globule: false,
      is_red_meat: false, is_processed_meat: false,
      iron_mg: 1.0, iron_absorption: 0.15, fat_profile: fp, balance_fat_profile: null,
      created_at: nowIso, updated_at: nowIso, ...extra,
    });
    const foods = [
      // Печень — железо и гем.
      food("liver", "Печень говяжья", 130, 20, 4,
        { sfa_pct: 35, mufa_pct: 45, pufa_pct: 15, epa_dha_pct: 0 },
        { is_heme: true, iron_mg: 6.5, iron_absorption: 0.25 }),
      // Скумбрия — морские омега-3.
      food("mack", "Скумбрия", 191, 18, 13.9,
        { sfa_pct: 24, mufa_pct: 38, pufa_pct: 26, epa_dha_pct: 18 }, {}),
      // Оливковое масло — вытягивает баланс, который портит мясо.
      food("oil", "Оливковое масло", 884, 0, 100,
        { sfa_pct: 15, mufa_pct: 73, pufa_pct: 11, epa_dha_pct: 0 }, {}),
      // Творог — добирает дневную норму белка; его жир в целой глобуле, поэтому
      // баланс он не портит.
      food("curd", "Творог 5%", 121, 18, 5,
        { sfa_pct: 63, mufa_pct: 26, pufa_pct: 4, epa_dha_pct: 0 },
        { is_milk_globule: true, nutrients: { "Кальций": 160, "Клетчатка": 0 } }),
      food("veg", "Овощи", 30, 1, 0.2,
        { sfa_pct: 0, mufa_pct: 0, pufa_pct: 0, epa_dha_pct: 0 }, { is_veg_fruit: true }),
      food("beef", "Говядина тушёная", 250, 27, 15,
        { sfa_pct: 43, mufa_pct: 45, pufa_pct: 4, epa_dha_pct: 0 },
        { is_red_meat: true, is_heme: true, iron_mg: 2.6, iron_absorption: 0.25 }),
      food("sausage", "Сосиски молочные", 260, 11, 24,
        { sfa_pct: 40, mufa_pct: 45, pufa_pct: 8, epa_dha_pct: 0 },
        { is_red_meat: true, is_processed_meat: true, iron_mg: 1.2, iron_absorption: 0.2 }),
    ];

    const diary = [], weight_entries = [], step_entries = [];
    for (let i = 0; i <= 32; i++) {
      weight_entries.push({ id: "w" + i, date: ymd(i), weight_kg: 88 - i * 0.03,
        no_water: false, no_food: false, no_wash: false, used_toilet: false, morning: true,
        created_at: nowIso, updated_at: nowIso });
      step_entries.push({ id: "s" + i, date: ymd(i), steps: 11000,
        created_at: nowIso, updated_at: nowIso });
      const add = (sfx, food_id, grams) => diary.push({ id: sfx + i, food_id, date: ymd(i),
        time: null, grams, waste_grams: 0, meal_label: "lunch", deleted: false,
        created_at: nowIso, updated_at: nowIso });
      add("dv", "veg", 850);
      add("dl", "liver", 120);
      add("do", "oil", 40);
      add("dc", "curd", 520);
      if (i % 3 === 0) add("dm", "mack", 200);
      // Мясо: «over» — в каждой неделе с перебором (иначе прошлые недели
      // закрываются и вердикт выходит оранжевым там, где кадр говорит «вы
      // рискуете»); «ok» — вдвое меньше, планка держится и шкала зелёная.
      if (i % 3 === 0) add("db", "beef", opts.meat === "over" ? 300 : 80);
      // Колбаса: «often» — четыре дня из последних семи, это красный по правилу
      // 1–3 оранжевый / 4+ красный. «none» — её нет вовсе, значок зелёный.
      if (opts.sausage === "often" && [1, 2, 4, 6, 8, 9, 11, 13].includes(i)) {
        add("ds", "sausage", 100);
      }
    }

    // Вердикты дневных индикаторов за последние 7 завершённых дней — закрытые.
    // `value === target` даёт зелёный и по коридору калорий, и по «не меньше нормы».
    const indDay = (date, value) => ({ date, value, ratio: 1.0, target: value,
      computed_at: nowIso });
    const ind_calories = [], ind_protein = [], ind_veg_fruit = [], ind_steps = [];
    for (let i = 1; i <= 7; i++) {
      ind_calories.push(indDay(ymd(i), 2400));
      ind_protein.push(indDay(ymd(i), 180));
      ind_veg_fruit.push(indDay(ymd(i), 800));
      ind_steps.push(indDay(ymd(i), 11000));
    }

    const avail = Array.from(db.objectStoreNames);
    for (const [store, rows] of Object.entries({ app_flags, profile, goals, foods, diary,
      weight_entries, step_entries, ind_calories, ind_protein, ind_veg_fruit, ind_steps })) {
      if (!avail.includes(store)) continue;
      await new Promise((res, rej) => {
        const tx = db.transaction([store], "readwrite");
        for (const row of rows) tx.objectStore(store).put(row);
        tx.oncomplete = () => res(); tx.onerror = () => rej(tx.error);
      });
    }
    db.close();
  }, { uid, opts });
};

// Две гифки за прогон, по кадру истории на каждую, и у каждой СВОЁ состояние:
// на кадре подсвечено ровно одно тревожное место, всё остальное зелёное.
//
//   red-meat-highlight.gif       — шкала недели и значок «Кр. мясо»: планка
//                                  перебрана (810 из 700), колбас человек не ел;
//   processed-meat-highlight.gif — значок «Колбасы»: шкалы у него нет и быть не
//                                  может, он про частоту. Мясо здесь в норме,
//                                  а колбаса была четыре дня из семи.
const b = await chromium.launch({ headless: true });

const run = async (label, opts, sparks, out, shot) => {
  const { context, page } = await openSeeded(b, {
    baseUrl: BASE,
    context: { serviceWorkers: "block", deviceScaleFactor: 2 },
    uid: `${label}-hl-${Math.floor(Math.random() * 1e6)}`,
    seed: (p, u) => seed(p, u, opts),
  });
  await page.reload({ waitUntil: "domcontentloaded" });
  await page.waitForTimeout(4000);
  await page.setViewportSize({ width: 440, height: 1200 });
  await page.waitForTimeout(3000); // дать полосам доиграть анимацию заполнения

  console.log(`\n[${label}] индикаторы:`,
    await page.$$eval("[data-ind]", (els) => els.map((e) => e.getAttribute("data-ind"))));
  console.log(`[${label}] мясо:`,
    (await page.locator('[data-gauge="Кр. мясо/нед"]').innerText()).replace(/\s+/g, " "));
  await page.locator('[data-testid="progress-widget"]').screenshot({ path: shot });
  await makeWidgetGif(page, sparks, out, {});
  console.log(`[${label}] готово:`, out);
  await context.close();
};

await run("redmeat", { meat: "over", sausage: "none" },
  ['[data-ind="Кр. мясо"] > div', '[data-gauge="Кр. мясо/нед"]'],
  OUT, "/tmp/red-meat-highlight-widget.png");

await run("processed", { meat: "ok", sausage: "often" },
  ['[data-ind="Колбасы"] > div'],
  OUT_PROCESSED, "/tmp/processed-meat-highlight-widget.png");

await b.close();
