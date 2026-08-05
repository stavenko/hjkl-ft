// Поднятая планка по шагам не должна судить задним числом уже прошедшие дни.
//
// Наблюдалось живьём: планка выросла с 10800 до 11800, а пятница с 11500 шагами
// замёрзла уже по новой — 11500/11800 = 0.97, недобор, индикатор оранжевый. День был
// выполнен по той планке, что тогда действовала.
//
// Сеем ровно это: планка 10800, семь завершённых дней по 11500 шагов (все выполнены),
// якорь недельного пересчёта старше семи дней — значит на запуске планка поднимется.
// Индикатор обязан остаться ЗЕЛЁНЫМ.
import { chromium } from "playwright";
import { openSeeded, DEFAULT_URL } from "./harness.mjs";

const BASE = process.env.FE || DEFAULT_URL;
const OLD_PLANKA = 10800;
const WALKED = 11500;

let fail = 0;
const check = (n, ok, extra = "") => { console.log(`${ok ? "OK " : "FAIL"} ${n}${extra ? " — " + extra : ""}`); if (!ok) fail++; };

const seed = async (page, uid) => {
  await page.evaluate(async ({ uid, OLD_PLANKA, WALKED }) => {
    const db = await new Promise((res, rej) => {
      const q = indexedDB.open(`hjkl-ft-${uid}`);
      q.onsuccess = () => res(q.result); q.onerror = () => rej(q.error);
    });
    const nowIso = new Date().toISOString();
    const ymd = (o) => { const d = new Date(); d.setDate(d.getDate() - o);
      return `${d.getFullYear()}-${String(d.getMonth() + 1).padStart(2, "0")}-${String(d.getDate()).padStart(2, "0")}`; };
    const app_flags = [
      { key: "push_onboarding_dismissed", value: "true" }, { key: "welcome_shown", value: "true" },
      { key: "db_schema_version", value: "999" },
      { key: "activity_week_unlocked", value: "true" }, { key: "steps_gate_opened_at", value: ymd(30) },
      { key: "ind_opened_at", value: ymd(30) },
      // Якорь старше семи дней — на запуске планка поднимется.
      { key: "steps_planka_weekly_anchor", value: ymd(9) },
      { key: "ft_subscription", value: JSON.stringify({ plan: "monthly", end: Date.now() + 30 * 864e5,
          active: true, start: Date.now(), status: "paid", no_renew: false, provider: "lava" }) },
    ];
    const profile = [{ key: "profile", sex: "male", height_cm: 180, birth_year: 1985,
      goal: "lose", steps_planka: OLD_PLANKA, created_at: nowIso, updated_at: nowIso }];
    // Шестнадцать завершённых дней (окно заморозки — 14), каждый ВЫШЕ старой планки,
    // но НИЖЕ будущей (11800): ровно случай «выполнил тогда, не выполнил бы сейчас».
    const step_entries = [];
    for (let i = 1; i <= 16; i++) {
      step_entries.push({ id: "s" + i, date: ymd(i), steps: WALKED,
        created_at: nowIso, updated_at: nowIso });
    }
    for (const [store, rows] of Object.entries({ app_flags, profile, step_entries })) {
      await new Promise((res, rej) => {
        const tx = db.transaction([store], "readwrite");
        for (const r of rows) tx.objectStore(store).put(r);
        tx.oncomplete = () => res(); tx.onerror = () => rej(tx.error);
      });
    }
    db.close();
  }, { uid, OLD_PLANKA, WALKED });
};

const b = await chromium.launch({ headless: true });
const { context, page } = await openSeeded(b, {
  baseUrl: BASE, context: { serviceWorkers: "block" },
  uid: `sfreeze-${Math.floor(Math.random() * 1e6)}`, seed,
});
await page.waitForTimeout(12000);

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
  const profile = await all("profile");
  const days = await all("ind_steps");
  db.close();
  return {
    planka: profile.find((p) => p.key === "profile")?.steps_planka,
    days: days.map((d) => ({ date: d.date, value: d.value, ratio: d.ratio })).sort((a, b) => a.date.localeCompare(b.date)),
  };
});

console.log(`планка: ${OLD_PLANKA} → ${state.planka}`);
for (const d of state.days) console.log(`  ${d.date}: ${d.value} шагов, ratio ${d.ratio}`);

check("планка поднялась (пересчёт сработал)", state.planka > OLD_PLANKA, `${state.planka}`);
const judged = state.days.filter((d) => d.ratio != null);
check("завершённые дни заморожены", judged.length > 0, `${judged.length} из ${state.days.length}`);
const met = judged.filter((d) => d.ratio >= 1);
check("дни, выполненные по СТАРОЙ планке, остались выполненными",
  met.length === judged.length, `${met.length} из ${judged.length} с ratio ≥ 1`);

const color = await page.locator('[data-ind="Шаги"] > div')
  .evaluate((el) => getComputedStyle(el).backgroundColor).catch(() => "");
const name = color.includes("31, 164, 99") ? "зелёный"
  : color.includes("232, 133, 13") ? "оранжевый"
  : color.includes("224, 48, 79") ? "красный" : `неизвестно (${color})`;
console.log(`индикатор шагов: ${name}`);
check("индикатор шагов зелёный", name === "зелёный", name);

console.log(fail === 0 ? "\n=== ALL OK ===" : `\n=== FAILURES: ${fail} ===`);
await context.close();
await b.close();
process.exit(fail === 0 ? 0 : 1);
