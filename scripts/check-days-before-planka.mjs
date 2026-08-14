// Дни ДО первой планки не судятся по ней.
//
// Боевой случай: человек неделю вёл дневник без планки и ел по 3850–3900 ккал. В
// конце недели ему выдали планку 3550 — и вся прошлая неделя разом окрасилась в
// перебор. Правила в те дни не было, нарушить его человек не мог.
//
// Воспроизводится ровно это: неделя переедания, планка выдана СЕГОДНЯ, а вердикты
// за прошлые дни уже заморожены по ней (как у пострадавшего). База сеется версией
// 12, чтобы прогналась миграция 13, снимающая такие вердикты.
//
// Проверяется то, что видит человек: за каждый день недели планкой стало съеденное,
// день зачтён, и в дневном ряду индикатора нет ни одного промаха.
import { chromium } from "playwright";
import { openSeeded, DEFAULT_URL } from "./harness.mjs";

const BASE = process.env.FE || DEFAULT_URL;

let fail = 0;
const check = (n, ok, extra = "") => {
  console.log(`${ok ? "OK  " : "FAIL"} ${n}${extra ? " — " + extra : ""}`);
  if (!ok) fail++;
};

const KCAL = [3850, 3900, 3700, 4100, 3800, 3950, 3600];
const PLANKA = 3550;

const seed = async (page, uid) => {
  await page.evaluate(async ({ uid, KCAL, PLANKA }) => {
    const db = await new Promise((res, rej) => {
      const r = indexedDB.open(`hjkl-ft-${uid}`);
      r.onsuccess = () => res(r.result); r.onerror = () => rej(r.error);
    });
    const nowIso = new Date().toISOString();
    // Дата ЛОКАЛЬНАЯ: приложение живёт по местному дню, а toISOString даёт UTC —
    // вечером это разные сутки, и сид разъезжается с тем, что считает приложение.
    const ymd = (back) => {
      const d = new Date(); d.setDate(d.getDate() - back);
      return `${d.getFullYear()}-${String(d.getMonth() + 1).padStart(2, "0")}-${String(d.getDate()).padStart(2, "0")}`;
    };
    const put = (store, rows) => new Promise((res, rej) => {
      const tx = db.transaction([store], "readwrite");
      rows.forEach((r) => tx.objectStore(store).put(r));
      tx.oncomplete = () => res(); tx.onerror = () => rej(tx.error);
    });

    // Версия 12: миграция 13 должна прогнаться на этой базе. Подписка обязательна —
    // без неё приложение уходит в Locked, а миграции идут только в Ready.
    await put("app_flags", [
      { key: "db_schema_version", value: "12" },
      { key: "push_onboarding_dismissed", value: "true" },
      { key: "welcome_shown", value: "true" },
      { key: "ft_subscription", value: JSON.stringify({
          plan: "monthly", end: Date.now() + 30 * 864e5, active: true,
          start: Date.now(), status: "paid", no_renew: false, provider: "lava" }) },
    ]);
    await put("profile", [{
      key: "profile", sex: "male", height_cm: 180, birth_year: 1985,
      goal: "lose", steps_planka: 9000, created_at: nowIso, updated_at: nowIso,
    }]);
    await put("foods", KCAL.map((kcal, i) => ({
      id: `f${i}`, name: `День ${i + 1}`, kcal, protein: 20, fat: 10, carbs: 30,
      nutrients: { "Кальций": 100, "Клетчатка": 5 }, package_weight: null,
      is_recipe: false, recipe_id: null, archived: false, is_restaurant: false,
      is_veg_fruit: false, is_heme: false, is_milk_globule: false,
      iron_mg: 1, iron_absorption: 0.15,
      fat_profile: { sfa_pct: 30, mufa_pct: 40, pufa_pct: 20, epa_dha_pct: 0 },
      balance_fat_profile: null, created_at: nowIso, updated_at: nowIso,
    })));
    await put("diary", KCAL.map((_, i) => ({
      id: `d${i}`, date: ymd(i + 1), food_id: `f${i}`, time: "13:00",
      grams: 100, waste_grams: 0, meal_label: "Обед", deleted: false,
      created_at: nowIso, updated_at: nowIso,
    })));
    await put("weight_entries", Array.from({ length: 7 }, (_, i) => ({
      id: `w${i}`, date: ymd(7 - i), weight_kg: 90 + i * 0.1,
      no_water: true, no_food: true, no_wash: true, used_toilet: true, morning: true,
      created_at: nowIso, updated_at: nowIso,
    })));
    await put("step_entries", Array.from({ length: 7 }, (_, i) => ({
      id: `s${i}`, date: ymd(i + 1), steps: 10000,
      created_at: nowIso, updated_at: nowIso,
    })));

    // Планка выдана СЕГОДНЯ — и в целях, и в истории.
    await put("goals", [{
      id: "g-cal", nutrient: "Calories", key: "calories", direction: "AtMost",
      amount: PLANKA, unit: "Kcal", period: "Day",
      created_at: nowIso, updated_at: nowIso,
    }]);
    await put("planka_history", [{
      id: `calories:${ymd(0)}`, kind: "calories", date: ymd(0), amount: PLANKA,
      created_at: nowIso, updated_at: nowIso,
    }]);

    // Вердикты за прошлые дни УЖЕ заморожены по новой планке — то, что человек и
    // увидел: «3850 из 3550».
    await put("ind_calories", KCAL.map((kcal, i) => ({
      date: ymd(i + 1), value: kcal, ratio: kcal / PLANKA, target: PLANKA,
      computed_at: nowIso,
    })));
    db.close();
  }, { uid, KCAL, PLANKA });
};

const b = await chromium.launch({ headless: true });
const uid = `before-planka-${Date.now()}`;
const { page } = await openSeeded(b, {
  baseUrl: BASE, uid, seed, context: { serviceWorkers: "block" },
});

// Миграция + заморозка идут на запуске. Ждём, пока вердикты станут новыми.
const read = () => page.evaluate(async (u) => {
  const db = await new Promise((res, rej) => {
    const r = indexedDB.open(`hjkl-ft-${u}`);
    r.onsuccess = () => res(r.result); r.onerror = () => rej(r.error);
  });
  const rows = await new Promise((res) => {
    const rq = db.transaction(["ind_calories"]).objectStore("ind_calories").getAll();
    rq.onsuccess = () => res(rq.result); rq.onerror = () => res([]);
  });
  const ver = await new Promise((res) => {
    const rq = db.transaction(["app_flags"]).objectStore("app_flags").get("db_schema_version");
    rq.onsuccess = () => res(rq.result?.value ?? null); rq.onerror = () => res(null);
  });
  db.close();
  return { rows, ver };
}, uid);

// Миграция ЧИСТИТ кэш, а заморозка потом наполняет его заново — и не мгновенно.
// Поэтому ждём не «ушли старые вердикты» (сразу после чистки кэш пуст, и условие
// выполнится, ничего не доказав), а появления всех дней недели.
let state = { rows: [], ver: null };
for (let i = 0; i < 40; i++) {
  await page.waitForTimeout(2000);
  state = await read();
  const eaten = state.rows.filter((r) => r.value > 0);
  if (state.ver === "13" && eaten.length >= KCAL.length) break;
}
state.rows = state.rows.filter((r) => r.value > 0); // дни без еды не судятся вовсе

console.log(`\nверсия базы ${state.ver}`);
for (const r of state.rows.sort((a, b) => a.date.localeCompare(b.date))) {
  console.log(`  ${r.date}  съедено ${Math.round(r.value)}  планка ${Math.round(r.target)}  ` +
    `доля ${r.ratio?.toFixed(2)}`);
}
console.log("");

check("миграция прогналась", state.ver === "13", `версия ${state.ver}`);
check("вердикты за всю неделю пересчитаны", state.rows.length === KCAL.length,
  `${state.rows.length} из ${KCAL.length} дней`);
check("ни один день не судится по выданной позже планке",
  state.rows.every((r) => Math.round(r.target) !== PLANKA),
  state.rows.filter((r) => Math.round(r.target) === PLANKA).map((r) => r.date).join(", "));
check("планкой дня стало съеденное",
  state.rows.every((r) => Math.abs(r.target - r.value) < 1));
check("доля ровно единица", state.rows.every((r) => Math.abs(r.ratio - 1) < 0.01));

// И то же самое глазами человека: дневной ряд индикатора без промахов.
const missed = await page.evaluate(async () => {
  const el = document.querySelector('[data-testid="ind-calories-days"]');
  return el ? el.getAttribute("data-missed") : null;
});
if (missed !== null) check("в дневном ряду нет промахов", missed === "0", `промахов ${missed}`);

await b.close();
console.log(fail ? `\n${fail} провалов` : "\nвсё сошлось");
process.exit(fail ? 1 : 0);
