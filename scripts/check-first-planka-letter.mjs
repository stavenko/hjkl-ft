// Письмо о первой планке: что в нём написано и сходятся ли числа.
//
// Меряется ЖИВОЙ ПУТЬ: заполняем неделю наблюдений (еда, вес, шаги — по 7 дней),
// открываем дашборд и НЕ трогаем ничего руками. Планка ставится сама, письмо
// пишется само, а мы читаем его из того же места, откуда его читает приложение.
//
// Дни еды нарочно РАЗНОЙ калорийности, а вес нарочно РАСТЁТ: письмо обязано
// назвать среднюю, максимум и минимум, и сказать про вес то же, что человек видит
// на своих весах.
import { chromium } from "playwright";
import { openSeeded, DEFAULT_URL } from "./harness.mjs";

const BASE = process.env.FE || DEFAULT_URL;

let fail = 0;
const check = (n, ok, extra = "") => {
  console.log(`${ok ? "OK  " : "FAIL"} ${n}${extra ? " — " + extra : ""}`);
  if (!ok) fail++;
};

// Калорийность дней недели наблюдений. Съеденное = ккал/100 г × 100 г, поэтому
// каждый день — одна запись с нужной калорийностью.
const KCAL = [2000, 2400, 1800, 2200, 3000, 1600, 2600];
const AVG = Math.round(KCAL.reduce((a, b) => a + b, 0) / KCAL.length); // 2229
const MAX = Math.max(...KCAL);
const MIN = Math.min(...KCAL);

const seed = async (page, uid) => {
  await page.evaluate(async ({ uid, KCAL }) => {
    const db = await new Promise((res, rej) => {
      const r = indexedDB.open(`hjkl-ft-${uid}`);
      r.onsuccess = () => res(r.result); r.onerror = () => rej(r.error);
    });
    const nowIso = new Date().toISOString();
    const ymd = (back) => {
      const d = new Date(); d.setDate(d.getDate() - back);
      return d.toISOString().slice(0, 10);
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
    ]);
    // Профиль нужен целиком: без роста, возраста и пола планка по белку не
    // считается и строки о ней в письме не будет.
    await put("profile", [{
      key: "profile", sex: "male", height_cm: 180, birth_year: 1985,
      goal: "lose", steps_planka: 9000, created_at: nowIso, updated_at: nowIso,
    }]);

    // По продукту на каждый день: калорийность на 100 г = калорийность дня.
    await put("foods", KCAL.map((kcal, i) => ({
      id: `f${i}`, name: `День ${i + 1}`, kcal, protein: 20, fat: 10, carbs: 30,
      nutrients: { "Кальций": 100, "Клетчатка": 5 }, package_weight: null,
      is_recipe: false, recipe_id: null, archived: false, is_restaurant: false,
      is_veg_fruit: false, is_heme: false, is_milk_globule: false,
      iron_mg: 1, iron_absorption: 0.15,
      fat_profile: { sfa_pct: 30, mufa_pct: 40, pufa_pct: 20, epa_dha_pct: 0 },
      balance_fat_profile: null, created_at: nowIso, updated_at: nowIso,
    })));
    // Поля записей — ровно те, что в api_types: без них запись не читается, и
    // счётчики недели молча остаются нулевыми.
    await put("diary", KCAL.map((_, i) => ({
      id: `d${i}`, date: ymd(i + 1), food_id: `f${i}`, time: "13:00",
      grams: 100, waste_grams: 0, meal_label: "Обед", deleted: false,
      created_at: nowIso, updated_at: nowIso,
    })));
    // Вес РАСТЁТ: 70.0 семь дней назад → 71.2 вчера.
    await put("weight_entries", Array.from({ length: 7 }, (_, i) => ({
      id: `w${i}`, date: ymd(7 - i), weight_kg: 70.0 + i * 0.2,
      no_water: true, no_food: true, no_wash: true, used_toilet: true, morning: true,
      created_at: nowIso, updated_at: nowIso,
    })));
    await put("step_entries", Array.from({ length: 7 }, (_, i) => ({
      id: `s${i}`, date: ymd(i + 1), steps: 10000,
      created_at: nowIso, updated_at: nowIso,
    })));
    db.close();
  }, { uid, KCAL });
};

const b = await chromium.launch({ headless: true });
// `openSeeded` возвращает только контекст и страницу, поэтому uid задаём сами —
// по нему потом открывается та же база, что и у приложения.
const uid = `planka-letter-${Date.now()}`;
const { page } = await openSeeded(b, {
  baseUrl: BASE, uid, seed,
  context: { serviceWorkers: "block" },
});

await page.goto(BASE, { waitUntil: "domcontentloaded" });

// Планка ставится сама, как только неделя закрыта. Ждём письмо, а не таймаут.
let letters = [];
for (let i = 0; i < 40; i++) {
  await page.waitForTimeout(3000);
  letters = await page.evaluate(async (u) => {
    const db = await new Promise((res, rej) => {
      const r = indexedDB.open(`hjkl-ft-${u}`);
      r.onsuccess = () => res(r.result); r.onerror = () => rej(r.error);
    });
    const row = await new Promise((res) => {
      const rq = db.transaction(["app_flags"]).objectStore("app_flags").get("letters_v1");
      rq.onsuccess = () => res(rq.result); rq.onerror = () => res(null);
    });
    db.close();
    try { return JSON.parse(row?.value ?? "[]"); } catch { return []; }
  }, uid);
  if (letters.length) break;
}

const letter = letters.find((l) => String(l.id).startsWith("planka-first"));
check("письмо о первой планке написано", !!letter, `писем ${letters.length}`);
if (!letter) {
  await b.close();
  console.log(`\n${fail} провалов`);
  process.exit(1);
}
console.log("\n────────────────────────────────────────\n" + letter.body +
  "\n────────────────────────────────────────\n");

const planka = await page.evaluate(async (u) => {
  const db = await new Promise((res, rej) => {
    const r = indexedDB.open(`hjkl-ft-${u}`);
    r.onsuccess = () => res(r.result); r.onerror = () => rej(r.error);
  });
  const all = await new Promise((res) => {
    const rq = db.transaction(["goals"]).objectStore("goals").getAll();
    rq.onsuccess = () => res(rq.result); rq.onerror = () => res([]);
  });
  db.close();
  // Калорийная цель опознаётся по `nutrient` (см. api_types::Goal).
  return all.find((g) => g.nutrient === "Calories")?.amount ?? null;
}, uid);

const t = letter.body;
check("поблагодарили за неделю", t.includes("вы хорошо поработали"));
check("названа средняя калорийность", t.includes(`${AVG} ккал`), `ждали ${AVG}`);
check("назван максимум", t.includes(`Максимальная калорийность: ${MAX} ккал`), `ждали ${MAX}`);
check("назван минимум", t.includes(`минимальная калорийность была ${MIN} ккал`), `ждали ${MIN}`);
check("сказано, что вес вырос", t.includes("ваш вес вырос"));
check("названа планка по калориям", planka !== null && t.includes(`${Math.round(planka)} ккал`),
  `планка ${planka}`);
check("названа планка по белку", /планку по белку: \d+ г/.test(t),
  (t.match(/планку по белку: \d+ г/) || ["нет"])[0]);
check("обещан перерасчёт через неделю", t.includes("Через неделю посмотрим"));
// Старого текста быть не должно: письмо заменено, а не дополнено.
check("старого текста нет", !t.includes("Это письмо — просто уведомление"));

await b.close();
console.log(fail ? `\n${fail} провалов` : "\nвсё сошлось");
process.exit(fail ? 1 : 0);
