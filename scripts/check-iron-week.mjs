// Неделя железа — на реальном пути: приложение запускается с посеянной базой,
// само открывает неделю железа и рисует ДВА (и только два) места, где железо
// вообще существует в интерфейсе: недельный gauge и недельный индикатор.
//
// Проверяем заодно, что железо НЕ просочилось в формы и списки нутриентов —
// старые сборки писали «Железо» в карту nutrients, эти значения должны быть
// отфильтрованы.
import { chromium } from "playwright";
import { openSeeded, DEFAULT_URL } from "./harness.mjs";

const BASE = process.env.FE || DEFAULT_URL;

let fail = 0;
const check = (name, ok, extra = "") => { console.log(`${ok ? "OK " : "FAIL"} ${name}${extra ? " — " + extra : ""}`); if (!ok) fail++; };

// Сеем состояние: календарная планка есть, недели активности/кальция пройдены,
// кальциевые ворота закрыты (7 зелёных дней), так что запуск ОБЯЗАН открыть
// неделю железа. Плюс еда с уже проставленным железом — чтобы gauge был не нулевой.
function seed({ ironOpenDaysAgo, calciumGateDaysAgo = 9, withIronFood = true, legacyIronNutrient = true, gramsPerDay = 200 }) {
  return async (page, uid) => {
    await page.evaluate(
      async ({ uid, ironOpenDaysAgo, calciumGateDaysAgo, withIronFood, legacyIronNutrient, gramsPerDay }) => {
        const db = await new Promise((res, rej) => {
          const r = indexedDB.open(`hjkl-ft-${uid}`);
          r.onsuccess = () => res(r.result);
          r.onerror = () => rej(r.error);
        });
        const nowIso = new Date().toISOString();
        const ymd = (back) => {
          const d = new Date();
          d.setDate(d.getDate() - back);
          return `${d.getFullYear()}-${String(d.getMonth() + 1).padStart(2, "0")}-${String(d.getDate()).padStart(2, "0")}`;
        };

        const app_flags = [
          { key: "push_onboarding_dismissed", value: "true" },
          { key: "activity_week_unlocked", value: "true" },
          { key: "steps_gate_opened_at", value: ymd(30) },
          // Версия заведомо выше любой миграции: этот тест НЕ про них, а миграция 1
      // стёрла бы посеянный кальций — и неделя железа не открылась бы вовсе.
      { key: "db_schema_version", value: "999" },
      { key: "calcium_week_unlocked", value: "true" },
          { key: "calcium_gate_opened_at", value: ymd(calciumGateDaysAgo) },
          { key: "ind_opened_at", value: ymd(40) },
          // Без активной подписки поверх приложения висит экран проверки, который
          // перехватывает нажатия — до трея историй не добраться.
          { key: "ft_subscription", value: JSON.stringify({
              plan: "monthly", end: Date.now() + 30 * 24 * 3600 * 1000, active: true,
              start: Date.now(), status: "paid", no_renew: false, provider: "lava" }) },
        ];
        if (ironOpenDaysAgo != null) {
          app_flags.push({ key: "iron_week_unlocked", value: "true" });
          app_flags.push({ key: "iron_week_opened_at", value: ymd(ironOpenDaysAgo) });
        }

        // Профиль: мужчина 45 лет → норма 8 мг/сут → неделя 8×7×0.18 = 10.08 мг усвоенного.
        const profile = [{
          key: "profile", sex: "male", height_cm: 180, birth_year: new Date().getFullYear() - 45,
          steps_planka: 10000, created_at: nowIso, updated_at: nowIso,
        }];

        // Планка по калориям — без неё виджет не показывает ни gauge, ни индикаторы.
        const goals = [
          { id: "g-cal", nutrient: "Calories", key: "calories", direction: "AtMost",
            amount: 2500, unit: "Kcal", period: "Day", created_at: nowIso, updated_at: nowIso },
          { id: "g-ca", nutrient: "Кальций", key: "calcium", direction: "AtLeast",
            amount: 1000, unit: "Mg", period: "Day", created_at: nowIso, updated_at: nowIso },
        ];

        // Продукт с ЖЕЛЕЗОМ в собственных полях + (по желанию) унаследованное
        // «Железо» в карте нутриентов — оно не должно попасть ни в одну форму.
        const nutrients = { "Кальций": 1200, "Клетчатка": 30 };
        if (legacyIronNutrient) nutrients["Железо"] = 9.9;
        const foods = [{
          id: "f-liver", name: "Куриная печень", kcal: 137, protein: 20, fat: 6, carbs: 1,
          nutrients, package_weight: null, is_recipe: false, recipe_id: null, archived: false,
          is_restaurant: false, is_snack: false, is_liquid_cal: false, is_veg_fruit: false,
          is_egg: false, is_red_meat: true,
          iron_mg: withIronFood ? 9.0 : null,
          iron_absorption: withIronFood ? 0.25 : null,
          created_at: nowIso, updated_at: nowIso,
        }];

        // Дневник: 10 дней подряд по 200 г печени.
        // 200 г × 9 мг/100 г × 0.25 = 4.5 мг усвоенного в день.
        const diary = [];
        for (let i = 0; i < 10; i++) {
          diary.push({
            id: `d-${i}`, food_id: "f-liver", date: ymd(i), time: null, grams: gramsPerDay,
            waste_grams: 0, meal_label: null, deleted: false, created_at: nowIso, updated_at: nowIso,
          });
        }

        // Кальциевые ворота закрываются по РЕАЛЬНЫМ данным: у кальция нет
        // кэш-стора, состояние дня считается из еды. 200 г печени × 1200 мг/100 г
        // = 2400 мг в день, норма 1000 — все завершённые дни зелёные.
        const records = { app_flags, profile, goals, foods, diary };
        const available = Array.from(db.objectStoreNames);
        for (const [store, rows] of Object.entries(records)) {
          if (!rows.length) continue;
          if (!available.includes(store)) { db.close(); throw new Error(`нет стора "${store}"; есть: ${available.join(", ")}`); }
          await new Promise((res, rej) => {
            const tx = db.transaction([store], "readwrite");
            const os = tx.objectStore(store);
            for (const row of rows) os.put(row);
            tx.oncomplete = () => res();
            tx.onerror = () => rej(tx.error);
          });
        }
        db.close();
      },
      { uid, ironOpenDaysAgo, calciumGateDaysAgo, withIronFood, legacyIronNutrient, gramsPerDay },
    );
  };
}

const readFlag = (page, key) => page.evaluate(async (k) => {
  const uid = localStorage.getItem("user_id");
  const db = await new Promise((res, rej) => {
    const r = indexedDB.open(`hjkl-ft-${uid}`);
    r.onsuccess = () => res(r.result); r.onerror = () => rej(r.error);
  });
  const v = await new Promise((res) => {
    const rq = db.transaction(["app_flags"], "readonly").objectStore("app_flags").get(k);
    rq.onsuccess = () => res(rq.result); rq.onerror = () => res(undefined);
  });
  db.close();
  return v && v.value;
}, key);

const b = await chromium.launch({ headless: true });
const today = new Date().toISOString().slice(0, 10);

// ── 1. Неделя железа открывается сама, когда кальциевые ворота пройдены ──
{
  const { context, page } = await openSeeded(b, {
    baseUrl: BASE, uid: `iron-open-${Math.floor(Math.random() * 1e6)}`,
    seed: seed({ ironOpenDaysAgo: null }),
  });
  let unlocked = null;
  for (let i = 0; i < 40; i++) {
    unlocked = await readFlag(page, "iron_week_unlocked");
    if (unlocked === "true") break;
    await page.waitForTimeout(500);
  }
  check("неделя железа открылась сама", unlocked === "true", `флаг ${unlocked}`);
  const open = await readFlag(page, "iron_week_opened_at");
  check("день открытия — сегодня (он же день 1 недели)", open === today, `${open}`);
  await context.close();
}

// ── 2. Недельный gauge и недельный индикатор на дашборде ──
{
  const { context, page } = await openSeeded(b, {
    baseUrl: BASE, uid: `iron-ui-${Math.floor(Math.random() * 1e6)}`,
    seed: seed({ ironOpenDaysAgo: 2 }), // сегодня 3-й день недели железа
  });
  await page.waitForTimeout(9000);
  const body = (await page.locator('[data-testid="progress-widget"]').innerText().catch(() => "")).replace(/\s+/g, " ");
  check("виджет показывает недельный gauge по железу", body.includes("Железо/нед"), body.slice(0, 160));
  // Железо стоит В ТОЙ ЖЕ сетке, что дневные полосы, — напротив кальция.
  const sameRow = await page.evaluate(() => {
    const ca = document.querySelector('[data-gauge="Кальций"]');
    const fe = document.querySelector('[data-gauge="Железо/нед"]');
    if (!ca || !fe) return null;
    return { sameParent: ca.parentElement === fe.parentElement,
             dy: Math.abs(ca.getBoundingClientRect().top - fe.getBoundingClientRect().top) };
  });
  check("железо в одной сетке с кальцием", !!sameRow && sameRow.sameParent, JSON.stringify(sameRow));
  check("железо на одной высоте с кальцием (напротив)", !!sameRow && sameRow.dy < 2,
    sameRow ? `сдвиг ${sameRow.dy.toFixed(1)} px` : "нет элементов");
  check("подписи «День N из 7» больше нет", !/День \d+ из 7/.test(body), body.slice(0, 200));
  // 3 дня × 4.5 мг = 13.5 усвоенного; норма мужчины 45 лет — 10.08 мг.
  check("gauge считает УСВОЕННОЕ железо (13.5 мг за 3 дня)", /13[.,]5/.test(body), body.slice(0, 200));
  check("норма недели — 10 мг усвоенного (мужчина 45)", /10[.,]1|10 мг/.test(body), body.slice(0, 200));
  const ironInd = page.locator('[data-ind="Железо"]');
  check("индикатор железа появился", await ironInd.isVisible().catch(() => false));
  await context.close();
}

// ── 3. Железа нет ни в одной форме и ни в одном списке нутриентов ──
{
  const { context, page } = await openSeeded(b, {
    baseUrl: BASE, uid: `iron-hidden-${Math.floor(Math.random() * 1e6)}`,
    seed: seed({ ironOpenDaysAgo: 2 }), landing: "/diary",
  });
  await page.waitForTimeout(8000);
  const diaryText = (await page.locator("body").innerText().catch(() => "")).replace(/\s+/g, " ");
  check("в дневнике железа нет", !/Железо(?! за неделю)/.test(diaryText), diaryText.slice(0, 120));

  // Форма редактирования продукта: унаследованное «Железо» не должно быть полем.
  await page.goto(`${BASE}/foods`, { waitUntil: "domcontentloaded" });
  await page.waitForTimeout(6000);
  const foodsText = (await page.locator("body").innerText().catch(() => "")).replace(/\s+/g, " ");
  check("в списке продуктов железа нет", !/Железо/.test(foodsText), foodsText.slice(0, 120));
  await context.close();
}

// ── 4. Пятая история появилась и несёт текст пользователя ──
{
  const { context, page } = await openSeeded(b, {
    baseUrl: BASE, uid: `iron-story-${Math.floor(Math.random() * 1e6)}`,
    seed: seed({ ironOpenDaysAgo: 2 }),
  });
  await page.waitForTimeout(9000);
  const badge = page.locator('[data-story-id="week5"]');
  const visible = await badge.isVisible().catch(() => false);
  check("в трее есть кружок 5-й истории", visible);
  if (visible) await badge.click();
  await page.waitForTimeout(2500);
  const story = (await page.locator("body").innerText().catch(() => "")).replace(/\s+/g, " ");
  check("первый кадр — про завершённую неделю кальция",
    story.includes("неделя кальция закончилась"), story.slice(0, 140));
  await context.close();
}

// ── 5. Точки суточного темпа и обводка «успеваем / отстаём» ──
// Шесть точек делят полосу на семь суточных отрезков. Горят те, чьи дни прошли:
// на N-м дне недели — N−1 точка. Обводка зелёная, пока набор обгоняет эти точки,
// и оранжевая, когда отстаёт.
async function paceCase(name, { ironOpenDaysAgo, gramsPerDay }, want) {
  const { context, page } = await openSeeded(b, {
    baseUrl: BASE, uid: `iron-pace-${Math.floor(Math.random() * 1e6)}`,
    seed: seed({ ironOpenDaysAgo, gramsPerDay }),
  });
  await page.waitForTimeout(9000);
  const gauge = page.locator('[data-gauge="Железо/нед"]');
  const lit = await page.locator('[data-pace-dot="lit"]').count();
  const dim = await page.locator('[data-pace-dot="dim"]').count();
  check(`${name}: точек ровно 6 (семь отрезков)`, lit + dim === 6, `горит ${lit}, погашено ${dim}`);
  check(`${name}: горит ${want.lit} (день ${want.day})`, lit === want.lit, `горит ${lit}`);
  // Тень берём с самой полосы — это второй потомок обёртки gauge.
  const shadow = await gauge.locator("div").nth(1).evaluate((el) => getComputedStyle(el).boxShadow);
  const green = shadow.includes("31, 164, 99");
  const orange = shadow.includes("232, 133, 13");
  check(`${name}: обводка ${want.glow}`,
    want.glow === "зелёная" ? green && !orange : orange && !green, shadow.slice(0, 70));
  await context.close();
}

// День 5, ест помногу → обгоняет темп.
await paceCase("обгоняем", { ironOpenDaysAgo: 4, gramsPerDay: 200 }, { day: 5, lit: 4, glow: "зелёная" });
// День 5, ест мало → отстаёт.
await paceCase("отстаём", { ironOpenDaysAgo: 4, gramsPerDay: 15 }, { day: 5, lit: 4, glow: "оранжевая" });
// Первый день: должок ещё не набежал — отставания быть не может.
await paceCase("первый день", { ironOpenDaysAgo: 0, gramsPerDay: 15 }, { day: 1, lit: 0, glow: "зелёная" });

console.log(fail === 0 ? "\n=== ALL OK ===" : `\n=== FAILURES: ${fail} ===`);
await b.close();
process.exit(fail === 0 ? 0 : 1);
