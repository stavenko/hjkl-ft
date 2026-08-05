// История планок: запись, чтение «какая планка была в тот день», и суд по ней.
//
// Планка движется — калорийную пересчитывает недельный цикл, шаговую поднимает цвет
// индикатора. Раньше хранилось только текущее значение, и день судился по нему: день,
// выполненный по 10800 шагов, под сегодняшними 11800 становился недобором.
//
// Сеем историю с ДВУМЯ ступенями и дни по обе стороны от смены. Проверяем, что
// каждый день засужен по своей планке.
import { chromium } from "playwright";
import { openSeeded, DEFAULT_URL } from "./harness.mjs";

const BASE = process.env.FE || DEFAULT_URL;
const OLD = 10800;
const NEW = 11800;
const WALKED = 11500; // выше старой планки, ниже новой

let fail = 0;
const check = (n, ok, extra = "") => { console.log(`${ok ? "OK " : "FAIL"} ${n}${extra ? " — " + extra : ""}`); if (!ok) fail++; };

const seed = async (page, uid) => {
  await page.evaluate(async ({ uid, OLD, NEW, WALKED }) => {
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
      // Якорь свежий — недельный пересчёт не вмешается.
      { key: "steps_planka_weekly_anchor", value: ymd(1) },
      { key: "ft_subscription", value: JSON.stringify({ plan: "monthly", end: Date.now() + 30 * 864e5,
          active: true, start: Date.now(), status: "paid", no_renew: false, provider: "lava" }) },
      // Версия выше всех миграций: сеем историю сами, m003 не нужна.
      { key: "db_schema_version", value: "999" },
    ];
    const profile = [{ key: "profile", sex: "male", height_cm: 180, birth_year: 1985,
      goal: "lose", steps_planka: NEW, created_at: nowIso, updated_at: nowIso }];
    const goals = [{ id: "g-cal", nutrient: "Calories", key: "calories", direction: "AtMost",
      amount: 2600, unit: "Kcal", period: "Day", created_at: nowIso, updated_at: nowIso }];
    // ДВЕ ступени: старая планка с −20 дней, новая с −3.
    const planka_history = [
      { id: `steps:${ymd(20)}`, kind: "steps", date: ymd(20), amount: OLD,
        created_at: nowIso, updated_at: nowIso },
      { id: `steps:${ymd(3)}`, kind: "steps", date: ymd(3), amount: NEW,
        created_at: nowIso, updated_at: nowIso },
    ];
    // Дни по обе стороны от смены, все с одинаковыми шагами.
    const step_entries = [];
    for (let i = 1; i <= 10; i++) {
      step_entries.push({ id: "s" + i, date: ymd(i), steps: WALKED,
        created_at: nowIso, updated_at: nowIso });
    }
    for (const [store, rows] of Object.entries({ app_flags, profile, goals, planka_history, step_entries })) {
      await new Promise((res, rej) => {
        const tx = db.transaction([store], "readwrite");
        for (const r of rows) tx.objectStore(store).put(r);
        tx.oncomplete = () => res(); tx.onerror = () => rej(tx.error);
      });
    }
    db.close();
  }, { uid, OLD, NEW, WALKED });
};

const b = await chromium.launch({ headless: true });
const { context, page } = await openSeeded(b, {
  baseUrl: BASE, context: { serviceWorkers: "block" },
  uid: `phist-${Math.floor(Math.random() * 1e6)}`, seed,
});
await page.waitForTimeout(12000);

const days = await page.evaluate(async () => {
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
  return rows.filter((r) => r.value > 0)
    .map((r) => ({ date: r.date, target: r.target, ratio: r.ratio }))
    .sort((a, b) => a.date.localeCompare(b.date));
});

for (const d of days) console.log(`  ${d.date}: планка ${d.target}, ratio ${(d.ratio || 0).toFixed(3)}`);

const cut = new Date(); cut.setDate(cut.getDate() - 3);
const cutStr = `${cut.getFullYear()}-${String(cut.getMonth() + 1).padStart(2, "0")}-${String(cut.getDate()).padStart(2, "0")}`;
const before = days.filter((d) => d.date < cutStr);
const after = days.filter((d) => d.date >= cutStr);

check("дни ДО смены засужены по старой планке",
  before.length > 0 && before.every((d) => d.target === OLD),
  `${before.length} дн.: ${[...new Set(before.map((d) => d.target))].join(", ")}`);
check("дни ПОСЛЕ смены засужены по новой планке",
  after.length > 0 && after.every((d) => d.target === NEW),
  `${after.length} дн.: ${[...new Set(after.map((d) => d.target))].join(", ")}`);
check("до смены день выполнен, после — нет",
  before.every((d) => d.ratio >= 1) && after.every((d) => d.ratio < 1),
  `до ${before.map((d) => d.ratio?.toFixed(2)).join(",")} / после ${after.map((d) => d.ratio?.toFixed(2)).join(",")}`);

console.log(fail === 0 ? "\n=== ALL OK ===" : `\n=== FAILURES: ${fail} ===`);
await context.close();
await b.close();
process.exit(fail === 0 ? 0 : 1);
