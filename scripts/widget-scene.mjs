// СЦЕНА ВИДЖЕТА для кадров историй: всё зелёное, кроме ОДНОГО индикатора.
//
// Кадр истории показывает виджет, чтобы человек нашёл на нём свой новый индикатор.
// Значит на снимке должно быть видно ровно то, что открыто к этому моменту пути, а
// герой кадра — заметен. Ряд значков вмещает семь штук и сортирован по состоянию
// (красные, оранжевые, зелёные), поэтому приём простой: всё остальное держим
// зелёным, а героя делаем ОРАНЖЕВЫМ — он встаёт первым и попадает в кадр всегда.
//
// Оранжевый, а не красный: красный у недельных значит два провала подряд, то есть
// историю о запущенном деле. Кадр рассказывает о новом индикаторе, и «пока не
// набрано» здесь честнее.
//
//   import { sceneSeed, WEEKS } from "./widget-scene.mjs";
//   const seed = sceneSeed({ week: "fats", target: "epa_dha" });
//   await openSeeded(browser, { seed, … });

/// Темы по порядку открытия. Каждая следующая включает все предыдущие.
export const WEEKS = ["base", "activity", "calcium", "iron", "fats", "red_meat", "egg"];

/// Пороги — те же числа, что в коде приложения. Держать в согласии вручную:
/// расхождение сделает сцену бессмысленной (снимок покажет не то состояние).
const RULES = {
  caloriePlanka: 2600,        // planka_history + дневная планка
  proteinPerDay: 157,         // белковая планка мужчины 88 кг
  vegPerDay: 800,             // мужская планка овощей и фруктов
  calciumPerDay: 1000,        // indicators::CALCIUM_PER_DAY_MG
  fiberPerDay: 25,            // indicators::FIBER_PER_DAY_G
  stepsPerDay: 9000,          // profile.steps_planka
  ironPerWeek: 10.1,          // iron: недельная планка усвоенного
  hemePortionsPerWeek: 3,     // heme::WEEKLY_PORTIONS
  epaDhaPerWeek: 1.75,        // fats::EPA_DHA_PER_WEEK_G
  unsatToSatMin: 2.0,         // fats::UNSAT_TO_SAT_MIN
  redMeatLimitPerWeek: 700,   // red_meat::WEEKLY_LIMIT_RAW_G
  eggGramsPerWeek: 350,       // egg::weekly_min_grams()
};

/// Сколько дней истории сеять: восемь завершённых недель окна плюс текущая.
const DAYS = 9 * 7;

/**
 * Сев сцены.
 *
 * @param {object} opts
 * @param {string} opts.week   до какой темы открыто (см. [`WEEKS`])
 * @param {string|null} opts.target ключ индикатора-героя: он будет ОРАНЖЕВЫМ.
 *   Дневные: calories | protein | veg_fruit | calcium | fiber | steps.
 *   Недельные: iron | heme | epa_dha | fat_ratio | red_meat | egg.
 *   `null` — всё зелёное.
 * @param {number} [opts.dayOfWeek] какой сегодня день недели темы (1…7), для шкал
 *   с темпом. По умолчанию 4 — середина, шкала наполовину.
 * @param {number} [opts.days] сколько дней истории сеять, считая сегодняшний.
 * @param {number} [opts.openedDaysAgo] сколько дней назад ВЫДАНЫ индикаторы. По
 *   умолчанию вся история. Меньше — это НОВИЧОК: гейт следующей темы ещё идёт, и
 *   виджет пишет «осталось N дней»; на давно выданных индикаторах серия набрана и
 *   подпись гейта пропадает, а с ней и кадр второй недели.
 *
 *   Дневник при этом сеется ГЛУБЖЕ, чем выданы индикаторы, и это не украшательство:
 *   овощам цель известна всегда, поэтому день без дневника для них не «нет данных»,
 *   а 0/800 — промах. Семь дней еды под тремя днями индикаторов дают зелёный ряд
 *   при незакрытом гейте.
 */
export function sceneSeed({ week, target = null, dayOfWeek = 4, days = DAYS, openedDaysAgo = null }) {
  if (!WEEKS.includes(week)) throw new Error(`неизвестная тема: ${week}`);
  return async (page, uid) => {
    await page.evaluate(
      async ({ uid, week, target, dayOfWeek, RULES, DAYS, OPENED, WEEKS }) => {
        const db = await new Promise((res, rej) => {
          const r = indexedDB.open(`hjkl-ft-${uid}`);
          r.onsuccess = () => res(r.result);
          r.onerror = () => rej(r.error);
        });
        const nowIso = new Date().toISOString();
        const midnight = (() => {
          const n = new Date();
          return new Date(n.getFullYear(), n.getMonth(), n.getDate());
        })();
        const ymd = (back) => {
          const d = new Date(midnight.getTime() - back * 864e5);
          return `${d.getFullYear()}-${String(d.getMonth() + 1).padStart(2, "0")}-${String(d.getDate()).padStart(2, "0")}`;
        };
        const upTo = (name) => WEEKS.indexOf(week) >= WEEKS.indexOf(name);

        // ── что открыто ────────────────────────────────────────────────────
        // Недели тем режутся ОТ ДНЯ ОТКРЫТИЯ, а не пн–вс. Открываем каждую так,
        // чтобы её сетка совпадала с недельной сеткой остальных: день открытия —
        // ровно `dayOfWeek - 1` дней назад плюс целые недели.
        const anchor = dayOfWeek - 1;
        // Якорь ПОСЛЕДНЕЙ открытой темы отдельный: гейт следующей темы смотрит,
        // была ли с её открытия хоть одна ЗАКРЫТАЯ неделя, и если была —
        // приложение откроет следующую неделю прямо во время съёмки. Так на кадре
        // про красное мясо всплывала шкала яиц.
        //
        // Поэтому у последней темы завершённых недель ровно столько, сколько нужно
        // её герою: недельному — одна, та самая испорченная (гейт видит провал и
        // молчит); дневному — ни одной (гейту нечего смотреть). Сами индикаторы от
        // этого не страдают: недели ДО открытия темы они судят наравне.
        const weeklyHero = {
          activity: [], calcium: [], iron: ["iron", "heme"],
          fats: ["epa_dha", "fat_ratio"], red_meat: ["red_meat"], egg: ["egg"],
        };
        const lastAnchor = (weeklyHero[week] || []).includes(target) ? anchor + 7 : anchor;
        /// Сколько дней назад открыта тема: у последней открытой — по правилу выше.
        const openedAt = (name, backDays) => ymd(name === week ? lastAnchor : backDays + anchor);
        const app_flags = [
          { key: "db_schema_version", value: "999" },
          { key: "welcome_shown", value: "true" },
          { key: "push_onboarding_dismissed", value: "true" },
          { key: "paywall_skipped_date", value: ymd(0) },
          { key: "ind_opened_at", value: ymd(OPENED) },
          { key: "ft_subscription", value: JSON.stringify({
              plan: "monthly", end: Date.now() + 30 * 864e5, active: true,
              start: Date.now(), status: "paid", no_renew: false, provider: "lava" }) },
        ];
        if (upTo("activity")) {
          app_flags.push({ key: "activity_week_unlocked", value: "true" });
          app_flags.push({ key: "steps_gate_opened_at", value: openedAt("activity", 56) });
        }
        if (upTo("calcium")) {
          app_flags.push({ key: "calcium_week_unlocked", value: "true" });
          app_flags.push({ key: "calcium_gate_opened_at", value: openedAt("calcium", 49) });
        }
        if (upTo("iron")) {
          app_flags.push({ key: "iron_week_unlocked", value: "true" });
          app_flags.push({ key: "iron_week_opened_at", value: openedAt("iron", 42) });
        }
        if (upTo("fats")) {
          app_flags.push({ key: "fat_week_unlocked", value: "true" });
          app_flags.push({ key: "fat_week_opened_at", value: openedAt("fats", 35) });
        }
        if (upTo("red_meat")) {
          app_flags.push({ key: "red_meat_week_unlocked", value: "true" });
          app_flags.push({ key: "red_meat_week_opened_at", value: openedAt("red_meat", 28) });
        }
        if (upTo("egg")) {
          app_flags.push({ key: "egg_week_unlocked", value: "true" });
          app_flags.push({ key: "egg_week_opened_at", value: openedAt("egg", 21) });
        }

        const profile = [{
          key: "profile", sex: "male", height_cm: 180,
          birth_year: new Date().getFullYear() - 45, goal: "lose",
          cycle_start: null, steps_planka: RULES.stepsPerDay,
          created_at: nowIso, updated_at: nowIso,
        }];
        const goals = [
          { id: "g-cal", nutrient: "Calories", key: "calories", direction: "AtMost",
            amount: RULES.caloriePlanka, unit: "Kcal", period: "Day",
            created_at: nowIso, updated_at: nowIso },
          { id: "g-ca", nutrient: "Кальций", key: "calcium", direction: "AtLeast",
            amount: RULES.calciumPerDay, unit: "Mg", period: "Day",
            created_at: nowIso, updated_at: nowIso },
        ];
        // История планки: индикатор калорий судит день по планке ТОГО дня.
        const planka_history = [{
          id: "ph-1", from_date: ymd(DAYS), kcal: RULES.caloriePlanka,
          created_at: nowIso, updated_at: nowIso,
        }];

        // ── продукты сцены ─────────────────────────────────────────────────
        const food = (id, name, extra) => ({
          id, name, kcal: 0, protein: 0, fat: 0, carbs: 0, nutrients: {},
          package_weight: null, is_recipe: false, recipe_id: null, archived: false,
          is_restaurant: false, is_veg_fruit: false, is_heme: false,
          is_milk_globule: false, is_red_meat: false, is_processed_meat: false,
          is_egg: false, iron_mg: 0, iron_absorption: 0.18,
          fat_profile: null, balance_fat_profile: null,
          created_at: nowIso, updated_at: nowIso, ...extra,
        });
        // У КАЖДОГО ИНДИКАТОРА СВОЙ ИСТОЧНИК — иначе порча одного роняет соседей.
        // Скумбрия давала разом гем, омегу и железо: убрать её ради оранжевой
        // омеги значило оранжевые гем и железо в придачу, а кадр обязан выделять
        // ровно одного героя.
        const base = {
          kcal: 1500, protein: 150, fat: 40, carbs: 140,
          nutrients: { "Кальций": 1100, "Клетчатка": 30 },
          iron_mg: 12, iron_absorption: 0.18,
          fat_profile: { sfa_pct: 12, mufa_pct: 40, pufa_pct: 44, epa_dha_pct: 0 },
        };
        const foods = [
          food("base", "Рацион дня", base),
          // Дубли основы с ослабленным ОДНИМ полем — ими и портится герой.
          food("base_low_iron", "Рацион дня", { ...base, iron_mg: 1.5 }),
          food("base_low_ca", "Рацион дня", { ...base, nutrients: { "Кальций": 150, "Клетчатка": 30 } }),
          food("base_low_protein", "Рацион дня", { ...base, protein: 40 }),
          food("base_low_fiber", "Рацион дня", { ...base, nutrients: { "Кальций": 1100, "Клетчатка": 2 } }),
          food("veg", "Овощи и фрукты", {
            kcal: 40, protein: 2, fat: 0.3, carbs: 8,
            nutrients: { "Клетчатка": 3 }, is_veg_fruit: true,
            fat_profile: { sfa_pct: 0, mufa_pct: 0, pufa_pct: 0, epa_dha_pct: 0 },
          }),
          // Гем — мидиями: моллюск, значит гемовый, но железа в нём немного. Печень
          // на этом месте не годилась: её 8.6 мг закрывали недельную планку железа
          // сами по себе, и «ослабленная основа» героя-железа ничего не меняла.
          food("liver", "Мидии", {
            // Жира почти нет, и морских омега-3 в нём тоже: иначе мидии закрывали
            // недельную омегу сами, и герой-«Омега-3» оставался зелёным.
            kcal: 86, protein: 25, fat: 1, carbs: 3, is_heme: true,
            iron_mg: 3.0, iron_absorption: 0.25,
            fat_profile: { sfa_pct: 20, mufa_pct: 30, pufa_pct: 45, epa_dha_pct: 2 },
          }),
          // Морские омега-3 — рыбьим жиром: гемового железа в нём нет.
          food("fishoil", "Рыбий жир", {
            kcal: 900, protein: 0, fat: 100, carbs: 0,
            fat_profile: { sfa_pct: 20, mufa_pct: 30, pufa_pct: 45, epa_dha_pct: 30 },
          }),
          food("egg", "Яйцо куриное", {
            kcal: 155, protein: 13, fat: 11, carbs: 1, is_egg: true,
            iron_mg: 1.2, iron_absorption: 0.2,
            fat_profile: { sfa_pct: 31, mufa_pct: 44, pufa_pct: 15, epa_dha_pct: 1 },
          }),
          food("beef", "Говядина", {
            kcal: 250, protein: 26, fat: 15, carbs: 0, is_red_meat: true, is_heme: true,
            iron_mg: 2.6, iron_absorption: 0.25,
            fat_profile: { sfa_pct: 40, mufa_pct: 45, pufa_pct: 4, epa_dha_pct: 0 },
          }),
          // Колбасы — из птицы: они переработанные, но НЕ красное мясо, и потому не
          // трогают недельный потолок мяса, когда герой кадра именно они.
          food("sausage", "Сосиски куриные", {
            kcal: 260, protein: 12, fat: 22, carbs: 3,
            is_processed_meat: true,
            iron_mg: 1.0, iron_absorption: 0.15,
            fat_profile: { sfa_pct: 35, mufa_pct: 45, pufa_pct: 15, epa_dha_pct: 0 },
          }),
          food("lard", "Сало", {
            kcal: 797, protein: 2, fat: 89, carbs: 0,
            fat_profile: { sfa_pct: 60, mufa_pct: 30, pufa_pct: 6, epa_dha_pct: 0 },
          }),
        ];

        // ── дневник ────────────────────────────────────────────────────────
        // Всё зелёное по умолчанию; «порча» героя — точечная, по его правилу.
        const diary = [], weight_entries = [], step_entries = [];
        const add = (id, food_id, back, grams) => diary.push({
          id, food_id, date: ymd(back), time: null, grams, waste_grams: 0,
          meal_label: "lunch", deleted: false, created_at: nowIso, updated_at: nowIso,
        });

        // Неделя героя-недельного индикатора: ПОСЛЕДНЯЯ ЗАВЕРШЁННАЯ, то есть дни
        // dayOfWeek…dayOfWeek+6 назад. Портим её — тогда индикатор оранжевый:
        // одна незакрытая неделя, предыдущая закрыта.
        const inSpoiledWeek = (back) => back >= anchor + 1 && back <= anchor + 7;
        // Дневные индикаторы судятся по последним семи ЗАВЕРШЁННЫМ дням; двух
        // промахов хватает на оранжевый (1–3 промаха), но не на красный.
        const spoiledDay = (back) => back === 1 || back === 3;

        for (let back = 0; back < DAYS; back++) {
          weight_entries.push({
            id: `w${back}`, date: ymd(back), weight_kg: 88 - back * 0.03,
            no_water: false, no_food: false, no_wash: false, used_toilet: false,
            morning: true, created_at: nowIso, updated_at: nowIso,
          });
          step_entries.push({
            id: `s${back}`, date: ymd(back),
            steps: target === "steps" && spoiledDay(back) ? 3000 : RULES.stepsPerDay + 2000,
            created_at: nowIso, updated_at: nowIso,
          });

          // ОСНОВА ДНЯ — она же носитель калорий, белка, кальция, клетчатки и
          // негемового железа. Порча героя подменяет её дублем с одним ослабленным
          // полем: калории при этом не съезжают, и день остаётся зелёным по всему
          // остальному.
          let baseId = "base";
          if (target === "iron" && inSpoiledWeek(back)) baseId = "base_low_iron";
          else if (target === "calcium" && spoiledDay(back)) baseId = "base_low_ca";
          else if (target === "protein" && spoiledDay(back)) baseId = "base_low_protein";
          else if (target === "fiber" && spoiledDay(back)) baseId = "base_low_fiber";
          add(`b${back}`, baseId, back, 130);

          add(`v${back}`, "veg", back, target === "veg_fruit" && spoiledDay(back) ? 150 : 900);

          // Печень дважды в неделю — три порции гема (25 г белка = порция).
          if (!(target === "heme" && inSpoiledWeek(back)) && (back % 7 === 2 || back % 7 === 5)) {
            add(`h${back}`, "liver", back, 200);
          }
          // Рыбий жир — 10 г в день: 3 г ЭПК+ДГК за неделю против планки 1.75.
          //
          // Дальше еда кладётся НЕЗАВИСИМО от того, открыта ли её тема: приложение
          // открывает следующую неделю само, как только закрыта предыдущая, и на
          // снимке появлялся красный значок темы, которую сцена не кормила. Флаги
          // решают, что ВИДНО; дневник — каким оно будет, когда станет видно.
          if (!(target === "epa_dha" && inSpoiledWeek(back))) {
            add(`o${back}`, "fishoil", back, 10);
          }

          if (!(target === "egg" && inSpoiledWeek(back))) {
            add(`e${back}`, "egg", back, 60);
          }

          // 50 г говядины в день — около 455 г сырого эквивалента за неделю против
          // потолка 700. Красное мясо считается ПО БЕЛКУ, и 80 г давали уже 728.
          // 30 г говядины в день. Больше нельзя: гем считается ПОРЦИЯМИ по 25 г
          // белка, и уже 50 г мяса давали три порции за неделю — норма гема
          // закрывалась мясом, а герой-«Гем» оставался зелёным без мидий.
          add(`m${back}`, "beef", back, target === "red_meat" && inSpoiledWeek(back) ? 200 : 30);

          // Колбасы судятся ЧАСТОТОЙ: промах — день, в который человек их ел.
          // Два дня из семи дают оранжевый, четыре — уже красный.
          if (target === "processed_meat" && spoiledDay(back)) {
            add(`p${back}`, "sausage", back, 80);
          }

          // Баланс жира роняется салом — насыщенных становится больше нормы.
          if (target === "fat_ratio" && inSpoiledWeek(back)) {
            add(`l${back}`, "lard", back, 150);
          }
          // Калории уводятся из коридора ±50 тем же салом, но в один-два дня.
          if (target === "calories" && spoiledDay(back)) {
            add(`x${back}`, "lard", back, 200);
          }
        }

        const rows = { app_flags, profile, goals, planka_history, foods, diary,
          weight_entries, step_entries };
        const avail = Array.from(db.objectStoreNames);
        for (const [store, list] of Object.entries(rows)) {
          if (!avail.includes(store) || !list.length) continue;
          await new Promise((res, rej) => {
            const tx = db.transaction([store], "readwrite");
            for (const row of list) tx.objectStore(store).put(row);
            tx.oncomplete = () => res();
            tx.onerror = () => rej(tx.error);
          });
        }
        db.close();
      },
      { uid, week, target, dayOfWeek, RULES, DAYS: days,
        OPENED: openedDaysAgo ?? days - 1, WEEKS },
    );
  };
}
