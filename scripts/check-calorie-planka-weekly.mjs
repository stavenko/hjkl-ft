// Планка по калориям обязана пересчитываться раз в неделю: через семь дней после
// того, как её поставили (и каждую неделю дальше). Сеем состояние «планку
// поставили десять дней назад, человек всё это время ест и взвешивается» и ждём,
// что планка изменится, а во «входящих» появится письмо.
import { chromium } from "playwright";
import { openSeeded, DEFAULT_URL } from "./harness.mjs";

const BASE = process.env.FE || DEFAULT_URL;
const OLD_PLANKA = 2500;

let fail = 0;
const check = (n, ok, extra = "") => { console.log(`${ok ? "OK " : "FAIL"} ${n}${extra ? " — " + extra : ""}`); if (!ok) fail++; };

const seed = async (page, uid) => {
  await page.evaluate(async ({ uid, OLD_PLANKA }) => {
    const db = await new Promise((res, rej) => {
      const q = indexedDB.open(`hjkl-ft-${uid}`);
      q.onsuccess = () => res(q.result); q.onerror = () => rej(q.error);
    });
    const iso = (o) => { const d = new Date(); d.setDate(d.getDate() - o); return d.toISOString(); };
    const ymd = (o) => iso(o).slice(0, 10);
    const nowIso = new Date().toISOString();

    const app_flags = [
      { key: "push_onboarding_dismissed", value: "true" }, { key: "welcome_shown", value: "true" },
      // Планку поставили ДЕСЯТЬ дней назад — недельный срок давно прошёл.
      { key: "planka_weekly_anchor", value: ymd(10) },
      { key: "ft_subscription", value: JSON.stringify({ plan: "monthly", end: Date.now() + 30 * 864e5,
          active: true, start: Date.now(), status: "paid", no_renew: false, provider: "lava" }) },
    ];
    const profile = [{ key: "profile", sex: "male", height_cm: 180, birth_year: 1985,
      goal: "lose", steps_planka: 9000, created_at: nowIso, updated_at: nowIso }];
    const goals = [{ id: "g-cal", nutrient: "Calories", key: "calories", direction: "AtMost",
      amount: OLD_PLANKA, unit: "Kcal", period: "Day", created_at: iso(10), updated_at: iso(10) }];
    const foods = [{ id: "f1", name: "Овсяная каша", kcal: 100, protein: 3, fat: 2, carbs: 18,
      nutrients: {}, package_weight: null, is_recipe: false, recipe_id: null, archived: false,
      is_restaurant: false, is_snack: false, is_liquid_cal: false, is_veg_fruit: false,
      is_egg: false, is_red_meat: false, iron_mg: 0.5, iron_absorption: 0.05,
      created_at: nowIso, updated_at: nowIso }];
    // Еда за каждый из последних десяти дней — «активность» для гейта пересчёта.
    const diary = [];
    for (let i = 1; i <= 10; i++) {
      diary.push({ id: "d" + i, food_id: "f1", date: ymd(i), time: null, grams: 2000,
        waste_grams: 0, meal_label: "lunch", deleted: false, created_at: nowIso, updated_at: nowIso });
    }
    // Вес падает ~1 кг в неделю — тренд, который двигает планку.
    const weight_entries = [];
    for (let i = 0; i < 21; i++) {
      weight_entries.push({ id: "w" + i, date: ymd(i), weight_kg: 90 + i * 0.14,
        no_water: true, no_food: true, no_wash: true, used_toilet: true, morning: true,
        created_at: nowIso, updated_at: nowIso });
    }
    // Одна СТРОКА СТАРОГО ФОРМАТА — без пяти булевых полей, добавленных в схему
    // позже. Раньше она роняла WASM панкой посреди запуска, и до недельного
    // пересчёта дело не доходило. Теперь её обязаны пропустить, а работу доделать.
    weight_entries.push({ id: "w-legacy", date: ymd(30), weight_kg: 93,
      created_at: nowIso, updated_at: nowIso });
    for (const [store, rows] of Object.entries({ app_flags, profile, goals, foods, diary, weight_entries })) {
      await new Promise((res, rej) => {
        const tx = db.transaction([store], "readwrite");
        for (const r of rows) tx.objectStore(store).put(r);
        tx.oncomplete = () => res(); tx.onerror = () => rej(tx.error);
      });
    }
    db.close();
  }, { uid, OLD_PLANKA });
};

const b = await chromium.launch({ headless: true });
const { context, page } = await openSeeded(b, {
  baseUrl: BASE, context: { serviceWorkers: "block" },
  uid: `planka-${Math.floor(Math.random() * 1e6)}`, seed,
});
const panics = [];
page.on("console", (m) => {
  const t = m.text();
  if (/panicked at/.test(t)) panics.push(t.slice(0, 200));
  if (!/integrity|Service Worker/.test(t)) console.log(`  [${m.type()}] ${t.slice(0, 200)}`);
});
page.on("pageerror", (e) => { panics.push(e.message); console.log(`  [pageerror] ${e.message}`); });
await page.reload({ waitUntil: "domcontentloaded" });
await page.waitForTimeout(15000);

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
  const hist = await all("planka_history");
  const flags = await all("app_flags");
  db.close();
  const f = (k) => flags.find((x) => x.key === k)?.value;
  return {
    // Действующая планка — последняя запись в истории по калориям.
    planka: hist.filter((e) => e.kind === "calories")
      .sort((a, b) => a.date.localeCompare(b.date)).at(-1)?.amount,
    anchor: f("planka_weekly_anchor"),
    letters: f("letters_v1") || "[]",
  };
});

const today = new Date().toISOString().slice(0, 10);
const letters = JSON.parse(state.letters);
console.log(`планка ${OLD_PLANKA} → ${state.planka}; якорь ${state.anchor}; писем ${letters.length}`);
check("планка пересчиталась", state.planka !== OLD_PLANKA, `${OLD_PLANKA} → ${state.planka}`);
check("якорь переставлен на сегодня", state.anchor === today, `${state.anchor} (сегодня ${today})`);
check("письмо о новой планке пришло", letters.some((l) => l.id === `planka-${today}`),
  letters.map((l) => l.id).join(", ") || "писем нет");
check("строка старого формата не уронила запуск", panics.length === 0,
  panics[0] || "паник нет");

console.log(fail === 0 ? "\n=== ALL OK ===" : `\n=== FAILURES: ${fail} ===`);
await context.close();
await b.close();
process.exit(fail === 0 ? 0 : 1);
