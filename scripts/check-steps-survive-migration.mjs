// Изменение ПРОДУКТА не должно сбрасывать замороженные дни ШАГОВ.
//
// Ровно боевой случай: индикатор шагов был ЗЕЛЁНЫМ (дни заморожены по планке 10800),
// вышло обновление, миграция стирания кальция и железа прошлась по всем продуктам и
// через invalidate_food снесла кэш ВСЕХ индикаторов за каждый день, где что-то ели —
// включая шаги. Дни пересчитались уже по новой планке 11800, и пятница с 11500
// шагами из выполненной стала недобором: индикатор позеленел в оранжевый.
//
// Сеем уже замороженные дни шагов (как у человека, у которого индикатор зелёный),
// планку, которая на них не тянет, и продукты с испорченным железом — чтобы
// миграция сработала. Дни обязаны уцелеть.
import { chromium } from "playwright";
import { openSeeded, DEFAULT_URL } from "./harness.mjs";

const BASE = process.env.FE || DEFAULT_URL;
const OLD_PLANKA = 10800;
const NEW_PLANKA = 11800;
const WALKED = 11500;

let fail = 0;
const check = (n, ok, extra = "") => { console.log(`${ok ? "OK " : "FAIL"} ${n}${extra ? " — " + extra : ""}`); if (!ok) fail++; };

const seed = async (page, uid) => {
  await page.evaluate(async ({ uid, OLD_PLANKA, NEW_PLANKA, WALKED }) => {
    const db = await new Promise((res, rej) => {
      const q = indexedDB.open(`hjkl-ft-${uid}`);
      q.onsuccess = () => res(q.result); q.onerror = () => rej(q.error);
    });
    const nowIso = new Date().toISOString();
    const ymd = (o) => { const d = new Date(); d.setDate(d.getDate() - o);
      return `${d.getFullYear()}-${String(d.getMonth() + 1).padStart(2, "0")}-${String(d.getDate()).padStart(2, "0")}`; };
    const app_flags = [
      { key: "push_onboarding_dismissed", value: "true" }, { key: "welcome_shown", value: "true" },
      { key: "activity_week_unlocked", value: "true" }, { key: "steps_gate_opened_at", value: ymd(30) },
      { key: "ind_opened_at", value: ymd(30) },
      // Якорь свежий — недельный пересчёт планки НЕ должен вмешаться в опыт.
      { key: "steps_planka_weekly_anchor", value: ymd(1) },
      { key: "ft_subscription", value: JSON.stringify({ plan: "monthly", end: Date.now() + 30 * 864e5,
          active: true, start: Date.now(), status: "paid", no_renew: false, provider: "lava" }) },
      // Версия базы 18: пройдут миграции, которые ТРОГАЮТ ПРОДУКТЫ (19 — кальций,
      // 24 — овощ/фрукт) и дёргают за собой `invalidate_food`. Ради этого проверка
      // и написана.
      //
      // Не 0: по дороге стоит миграция 15, которая сносит вердикты ЦЕЛИКОМ — один
      // раз и намеренно, чтобы свести накопившиеся правила в одно состояние. С
      // нулевой версии посеянная заморозка гибнет об неё, а не о то, что здесь
      // проверяется.
      { key: "db_schema_version", value: "18" },
    ];
    // Планка УЖЕ новая — как после вчерашнего пересчёта.
    const profile = [{ key: "profile", sex: "male", height_cm: 180, birth_year: 1985,
      goal: "lose", steps_planka: NEW_PLANKA, created_at: nowIso, updated_at: nowIso }];
    // Планка живёт в ИСТОРИИ, и записана НОВАЯ — как у человека после вчерашнего
    // пересчёта. Если заморозку кто-то снесёт, дни пересчитаются по ней (11500 из
    // 11800 — недобор), и подмена станет видна. Без записи в истории день судился бы
    // собственным результатом и выглядел бы выполненным при любом раскладе.
    const planka_history = [
      { id: `calories:${ymd(30)}`, kind: "calories",
        date: ymd(30), amount: 2600, created_at: nowIso, updated_at: nowIso },
      { id: `steps:${ymd(30)}`, kind: "steps",
        date: ymd(30), amount: NEW_PLANKA, created_at: nowIso, updated_at: nowIso },
    ];
    // Продукт с испорченным железом — миграции будет что стирать, и она дёрнет
    // invalidate_food за все дни, где его ели.
    const foods = [{ id: "liver", name: "Куриная печень", kcal: 137, protein: 20, fat: 6, carbs: 1,
      nutrients: { "Кальций": 999 }, package_weight: null, is_recipe: false, recipe_id: null,
      archived: false, is_restaurant: false, is_snack: false, is_liquid_cal: false,
      is_veg_fruit: false, is_egg: false, is_red_meat: true, iron_mg: 0.25, iron_absorption: 0.25,
      created_at: nowIso, updated_at: nowIso }];
    const diary = [];
    const step_entries = [];
    const ind_steps = [];
    for (let i = 1; i <= 10; i++) {
      diary.push({ id: "d" + i, food_id: "liver", date: ymd(i), time: null, grams: 200,
        waste_grams: 0, meal_label: "lunch", deleted: false, created_at: nowIso, updated_at: nowIso });
      step_entries.push({ id: "s" + i, date: ymd(i), steps: WALKED,
        created_at: nowIso, updated_at: nowIso });
      // Дни УЖЕ заморожены по СТАРОЙ планке — индикатор зелёный.
      ind_steps.push({ date: ymd(i), value: WALKED, ratio: WALKED / OLD_PLANKA,
        target: OLD_PLANKA, computed_at: nowIso });
    }
    for (const [store, rows] of Object.entries({ app_flags, profile, planka_history, foods, diary, step_entries, ind_steps })) {
      await new Promise((res, rej) => {
        const tx = db.transaction([store], "readwrite");
        for (const r of rows) tx.objectStore(store).put(r);
        tx.oncomplete = () => res(); tx.onerror = () => rej(tx.error);
      });
    }
    db.close();
  }, { uid, OLD_PLANKA, NEW_PLANKA, WALKED });
};

const b = await chromium.launch({ headless: true });
const { context, page } = await openSeeded(b, {
  baseUrl: BASE, context: { serviceWorkers: "block" },
  uid: `smig-${Math.floor(Math.random() * 1e6)}`, seed,
});
// Промежуточный замер: посеялись ли дни ВООБЩЕ. Иначе «переписаны» и «не записаны»
// неотличимы.
const peek = () => page.evaluate(async () => {
  const uid = localStorage.getItem("user_id");
  const db = await new Promise((res, rej) => {
    const q = indexedDB.open(`hjkl-ft-${uid}`);
    q.onsuccess = () => res(q.result); q.onerror = () => rej(q.error);
  });
  const rows = await new Promise((res) => {
    const rq = db.transaction(["ind_steps"], "readonly").objectStore("ind_steps").getAll();
    rq.onsuccess = () => res(rq.result); rq.onerror = () => res([]);
  });
  db.close();
  return rows.map((r) => r.target);
});
console.log(`сразу после загрузки в ind_steps: ${JSON.stringify(await peek())}`);
await page.waitForTimeout(14000);

const state = await page.evaluate(async () => {
  const uid = localStorage.getItem("user_id");
  const db = await new Promise((res, rej) => {
    const q = indexedDB.open(`hjkl-ft-${uid}`);
    q.onsuccess = () => res(q.result); q.onerror = () => rej(q.error);
  });
  const all = (s) => new Promise((res) => {
    const rq = db.transaction([s], "readonly").objectStore(s).getAll();
    rq.onsuccess = () => res(rq.result); rq.onerror = () => res([]);
  });
  const days = await all("ind_steps");
  const foods = await all("foods");
  const flags = await all("app_flags");
  db.close();
  const f = foods.find((x) => x.id === "liver");
  const n = f?.nutrients instanceof Map ? Object.fromEntries(f.nutrients) : (f?.nutrients || {});
  return {
    version: flags.find((x) => x.key === "db_schema_version")?.value,
    calcium: n["Кальций"],
    days: days.map((d) => ({ date: d.date, target: d.target, ratio: d.ratio }))
      .sort((a, b) => a.date.localeCompare(b.date)),
  };
});

console.log(`версия базы: ${state.version}, кальций у продукта: ${state.calcium}`);
for (const d of state.days.slice(-3)) console.log(`  ${d.date}: планка ${d.target}, ratio ${d.ratio}`);

check("миграция прошла (кальций 999 стёрт)", state.calcium !== 999, String(state.calcium));
// Дней стало больше десяти — заморозка дописала пустые дни из своего окна в две
// недели, и это правильно. Смотрим ИМЕННО посеянные: у них шаги есть.
const seeded = state.days.filter((d) => d.ratio > 0);
check("посеянные дни уцелели", seeded.length === 10, `${seeded.length} из 10`);
check("уцелевшие дни остались заморожены по СТАРОЙ планке",
  seeded.every((d) => d.target === OLD_PLANKA),
  [...new Set(seeded.map((d) => d.target))].join(", "));

const color = await page.locator('[data-ind="Шаги"] > div')
  .evaluate((el) => getComputedStyle(el).backgroundColor).catch(() => "");
const name = color.includes("31, 164, 99") ? "зелёный"
  : color.includes("232, 133, 13") ? "оранжевый"
  : color.includes("224, 48, 79") ? "красный" : `неизвестно (${color})`;
console.log(`индикатор шагов после обновления: ${name}`);
check("индикатор остался зелёным", name === "зелёный", name);

console.log(fail === 0 ? "\n=== ALL OK ===" : `\n=== FAILURES: ${fail} ===`);
await context.close();
await b.close();
process.exit(fail === 0 ? 0 : 1);
