// Дни ДО первой планки не судятся по ней.
//
// Боевой случай: человек неделю вёл дневник без планки и ел по 3850–3900 ккал. В
// конце недели ему выдали планку 3550 — и вся прошлая неделя разом окрасилась в
// перебор. Правила в те дни не было, нарушить его человек не мог.
//
// Воспроизводится ровно это: неделя переедания, планка выдана СЕГОДНЯ, а вердикты
// за прошлые дни уже заморожены по ней (как у пострадавшего). База сеется версией
// 12, чтобы прогнались миграции 13-15, снимающие такие вердикты.
//
// Правило одно для всех ДВИЖУЩИХСЯ планок — калории, шаги, белок, — поэтому
// проверяются все три сразу: у каждой планкой дня обязано стать то, что человек в
// этот день и сделал. Норма, не зависящая от веса (овощи и фрукты), проверяется
// заодно как контроль: она едина во времени и обязана остаться собой.
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
// Шаги нарочно НИЖЕ выданной позже планки, белок — тоже: по старому правилу это
// был бы недобор во все семь дней.
const STEPS = [4000, 5200, 3800, 6100, 4400, 5000, 4700];
const STEPS_PLANKA = 9000;
const PROTEIN_PLANKA = 150;

const seed = async (page, uid) => {
  await page.evaluate(async ({ uid, KCAL, PLANKA, STEPS, STEPS_PLANKA, PROTEIN_PLANKA }) => {
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
    await put("step_entries", STEPS.map((steps, i) => ({
      id: `s${i}`, date: ymd(i + 1), steps,
      created_at: nowIso, updated_at: nowIso,
    })));

    // Планка выдана СЕГОДНЯ. Живёт она в истории — она и есть цель.
    // Все три движущиеся планки выданы СЕГОДНЯ и раньше не существовали.
    await put("planka_history", [
      { id: `calories:${ymd(0)}`, kind: "calories", date: ymd(0), amount: PLANKA,
        created_at: nowIso, updated_at: nowIso },
      { id: `steps:${ymd(0)}`, kind: "steps", date: ymd(0), amount: STEPS_PLANKA,
        created_at: nowIso, updated_at: nowIso },
      { id: `protein:${ymd(0)}`, kind: "protein", date: ymd(0), amount: PROTEIN_PLANKA,
        created_at: nowIso, updated_at: nowIso },
    ]);

    // Вердикты за прошлые дни УЖЕ заморожены по новым планкам — то, что человек и
    // увидел: «3850 из 3550», недобор шагов, недобор белка.
    await put("ind_calories", KCAL.map((kcal, i) => ({
      date: ymd(i + 1), value: kcal, ratio: kcal / PLANKA, target: PLANKA,
      computed_at: nowIso,
    })));
    await put("ind_steps", STEPS.map((steps, i) => ({
      date: ymd(i + 1), value: steps, ratio: steps / STEPS_PLANKA, target: STEPS_PLANKA,
      computed_at: nowIso,
    })));
    // Белок за день: 20 г на 100 г продукта — по одной записи в день.
    await put("ind_protein", KCAL.map((_, i) => ({
      date: ymd(i + 1), value: 20, ratio: 20 / PROTEIN_PLANKA, target: PROTEIN_PLANKA,
      computed_at: nowIso,
    })));
    db.close();
  }, { uid, KCAL, PLANKA, STEPS, STEPS_PLANKA, PROTEIN_PLANKA });
};

const b = await chromium.launch({ headless: true });
const uid = `before-planka-${Date.now()}`;
const { page } = await openSeeded(b, {
  baseUrl: BASE, uid, seed, context: { serviceWorkers: "block" },
});

// Миграция + заморозка идут на запуске. Ждём, пока вердикты станут новыми.
const STORES = ["ind_calories", "ind_steps", "ind_protein", "ind_veg_fruit"];

const read = () => page.evaluate(async ({ u, STORES }) => {
  const db = await new Promise((res, rej) => {
    const r = indexedDB.open(`hjkl-ft-${u}`);
    r.onsuccess = () => res(r.result); r.onerror = () => rej(r.error);
  });
  const out = {};
  for (const s of STORES) {
    out[s] = await new Promise((res) => {
      if (!db.objectStoreNames.contains(s)) return res([]);
      const rq = db.transaction([s]).objectStore(s).getAll();
      rq.onsuccess = () => res(rq.result); rq.onerror = () => res([]);
    });
  }
  const ver = await new Promise((res) => {
    const rq = db.transaction(["app_flags"]).objectStore("app_flags").get("db_schema_version");
    rq.onsuccess = () => res(rq.result?.value ?? null); rq.onerror = () => res(null);
  });
  db.close();
  return { out, ver };
}, { u: uid, STORES });

// Миграция ЧИСТИТ кэш, а заморозка потом наполняет его заново — и не мгновенно.
// Поэтому ждём не «ушли старые вердикты» (сразу после чистки кэш пуст, и условие
// выполнится, ничего не доказав), а появления всех дней недели.
const eatenDays = (rows) => rows.filter((r) => r.value > 0);
let state = { out: {}, ver: null };
for (let i = 0; i < 40; i++) {
  await page.waitForTimeout(2000);
  state = await read();
  // У калорий и шагов есть заморозка последних двух недель, поэтому их дни
  // возвращаются в кэш сами. У белка её нет — его дни считаются по мере
  // отрисовки, и требовать от него полную неделю здесь нечестно.
  const ready = ["ind_calories", "ind_steps"]
    .every((s) => eatenDays(state.out[s] ?? []).length >= KCAL.length);
  if (state.ver === "15" && ready) break;
}

console.log(`\nверсия базы ${state.ver}`);
const MOVING = [
  ["ind_calories", "калории", PLANKA],
  ["ind_steps", "шаги", STEPS_PLANKA],
  ["ind_protein", "белок", PROTEIN_PLANKA],
];
for (const [store, label] of MOVING) {
  const rows = eatenDays(state.out[store] ?? []).sort((a, b) => a.date.localeCompare(b.date));
  console.log(`\n  ${label}:`);
  for (const r of rows) {
    console.log(`    ${r.date}  сделано ${Math.round(r.value)}  планка ${Math.round(r.target)}  ` +
      `доля ${r.ratio?.toFixed(2)}`);
  }
}
console.log("");

check("миграции прогнались", state.ver === "15", `версия ${state.ver}`);
for (const [store, label, planka] of MOVING) {
  const rows = eatenDays(state.out[store] ?? []);
  // Полную неделю обратно в кэш возвращает заморозка, а она есть только у калорий
  // и шагов. Белку хватает того, что посчиталось: правило одно на всех, и если оно
  // сработало на посчитанных днях, то сработает и на остальных.
  const want = store === "ind_protein" ? 1 : KCAL.length;
  check(`${label}: вердикты пересчитаны`, rows.length >= want,
    `${rows.length} из ${KCAL.length} дней`);
  check(`${label}: ни один день не судится по выданной позже планке`,
    rows.length > 0 && rows.every((r) => Math.round(r.target) !== planka),
    rows.filter((r) => Math.round(r.target) === planka).map((r) => r.date).join(", "));
  check(`${label}: планкой дня стал собственный результат`,
    rows.length > 0 && rows.every((r) => Math.abs(r.target - r.value) < 1));
  check(`${label}: доля ровно единица`,
    rows.length > 0 && rows.every((r) => Math.abs(r.ratio - 1) < 0.01));
}

// Контроль: норма, не зависящая от веса, во времени не движется — её дни судятся
// текущей нормой, и правило «планка = результат» к ней не применяется.
const veg = eatenDays(state.out.ind_veg_fruit ?? []);
if (veg.length) {
  check("овощи и фрукты судятся своей нормой, а не результатом дня",
    veg.some((r) => Math.abs(r.target - r.value) >= 1),
    veg.map((r) => `${r.date}: ${Math.round(r.value)}/${Math.round(r.target)}`).join(", "));
}

await b.close();
console.log(fail ? `\n${fail} провалов` : "\nвсё сошлось");
process.exit(fail ? 1 : 0);
