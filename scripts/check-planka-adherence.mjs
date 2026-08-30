// Недельный пересчёт планки НЕ поднимает её тому, кто планку не доедает.
//
// Боевой случай: тревожная неделя. Человек ест заметно меньше планки и от этого
// быстро худеет. Правило по весу читает быстрое похудение как «поберечь мышцы» и
// зовёт планку ВВЕРХ. Если поддаться, разрыв между съеденным и предписанным будет
// расти каждую неделю: есть больше человек не станет, вес продолжит падать, планка
// продолжит подниматься.
//
// Проверяется на ЖИВОМ пути: сеются неделя недоедания и падающий вес, срок
// пересчёта наступил (якорь на 8 дней назад) — приложение само пересчитывает планку
// на запуске. Ожидание: планка НЕ выросла, а письмо объясняет почему.
import { chromium } from "playwright";
import { openSeeded, DEFAULT_URL } from "./harness.mjs";

const BASE = process.env.FE || DEFAULT_URL;

let fail = 0;
const check = (n, ok, extra = "") => {
  console.log(`${ok ? "OK  " : "FAIL"} ${n}${extra ? " — " + extra : ""}`);
  if (!ok) fail++;
};

const PLANKA = 2400;
// Ест на 700 ккал меньше планки каждый день — недобор, ни с какой погрешностью не
// спутать.
const EATEN = 1700;

const seed = async (page, uid) => {
  await page.evaluate(async ({ uid, PLANKA, EATEN }) => {
    const db = await new Promise((res, rej) => {
      const r = indexedDB.open(`hjkl-ft-${uid}`);
      r.onsuccess = () => res(r.result); r.onerror = () => rej(r.error);
    });
    const nowIso = new Date().toISOString();
    const ymd = (b) => { const d = new Date(); d.setDate(d.getDate() - b);
      return `${d.getFullYear()}-${String(d.getMonth()+1).padStart(2,"0")}-${String(d.getDate()).padStart(2,"0")}`; };
    const put = (store, rows) => new Promise((res, rej) => {
      const tx = db.transaction([store], "readwrite");
      rows.forEach((r) => tx.objectStore(store).put(r));
      tx.oncomplete = () => res(); tx.onerror = () => rej(tx.error);
    });

    await put("app_flags", [
      { key: "db_schema_version", value: "999" },
      { key: "push_onboarding_dismissed", value: "true" },
      { key: "welcome_shown", value: "true" },
      { key: "ft_subscription", value: JSON.stringify({ plan: "monthly", end: Date.now()+30*864e5,
          active: true, start: Date.now(), status: "paid", no_renew: false, provider: "lava" }) },
      // Срок пересчёта вышел: якорь на 8 дней назад.
      { key: "planka_weekly_anchor", value: ymd(8) },
    ]);
    await put("profile", [{ key: "profile", sex: "male", height_cm: 180, birth_year: 1985,
      goal: "lose", steps_planka: 9000, created_at: nowIso, updated_at: nowIso }]);
    // Планка живёт в ИСТОРИИ — это и есть цель. Раньше здесь сеялась строка в
    // `goals`; целей больше нет.
    await put("planka_history", [{ id: `calories:${ymd(30)}`, kind: "calories", date: ymd(30),
      amount: PLANKA, created_at: ymd(30) + "T09:00:00Z", updated_at: nowIso }]);
    await put("foods", [{ id: "f", name: "Еда", kcal: EATEN, protein: 20, fat: 10, carbs: 40,
      nutrients: {}, package_weight: null, is_recipe: false, recipe_id: null, archived: false,
      is_restaurant: false, is_veg_fruit: false, is_heme: false, is_milk_globule: false,
      is_red_meat: false, is_processed_meat: false, iron_mg: 1, iron_absorption: 0.15,
      fat_profile: null, balance_fat_profile: null, created_at: nowIso, updated_at: nowIso }]);
    // Неделя недоедания.
    await put("diary", Array.from({ length: 8 }, (_, i) => ({
      id: `d${i}`, date: ymd(i + 1), food_id: "f", time: "13:00", grams: 100, waste_grams: 0,
      meal_label: "Обед", deleted: false, created_at: nowIso, updated_at: nowIso })));
    // Вес падает быстро: 1.2 кг за две недели при 80 кг — заметно выше комфортной
    // полосы, то есть правило по весу зовёт планку ВВЕРХ.
    await put("weight_entries", Array.from({ length: 14 }, (_, i) => ({
      id: `w${i}`, date: ymd(14 - i), weight_kg: 80.0 - i * 0.12,
      no_water: true, no_food: true, no_wash: true, used_toilet: true, morning: true,
      created_at: nowIso, updated_at: nowIso })));
    await put("step_entries", Array.from({ length: 14 }, (_, i) => ({
      id: `s${i}`, date: ymd(i + 1), steps: 9000, created_at: nowIso, updated_at: nowIso })));
    db.close();
  }, { uid, PLANKA, EATEN });
};

const b = await chromium.launch({ headless: true });
const uid = `adher-${Date.now()}`;
const { page } = await openSeeded(b, { baseUrl: BASE, uid, seed,
  context: { serviceWorkers: "block" } });
await page.goto(BASE, { waitUntil: "domcontentloaded" });

// База открывается ПОСЛЕ перезагрузки страницы, и первые мгновения сторов в ней
// может не быть: читаем терпимо к этому, иначе гонка роняет проверку.
const read = () => page.evaluate(async (u) => {
  const db = await new Promise((res, rej) => {
    const r = indexedDB.open(`hjkl-ft-${u}`);
    r.onsuccess = () => res(r.result); r.onerror = () => rej(r.error);
  });
  if (!db.objectStoreNames.contains("planka_history") || !db.objectStoreNames.contains("app_flags")) {
    db.close();
    return { planka: null, letters: [] };
  }
  const hist = await new Promise((res) => {
    const rq = db.transaction(["planka_history"]).objectStore("planka_history").getAll();
    rq.onsuccess = () => res(rq.result); rq.onerror = () => res([]);
  });
  const flags = await new Promise((res) => {
    const rq = db.transaction(["app_flags"]).objectStore("app_flags").get("letters_v1");
    rq.onsuccess = () => res(rq.result?.value ?? "[]"); rq.onerror = () => res("[]");
  });
  db.close();
  let letters = [];
  try { letters = JSON.parse(flags); } catch {}
  const g = hist.filter((x) => x.kind === "calories")
    .sort((a, b) => a.date.localeCompare(b.date)).at(-1);
  return { planka: g ? g.amount : null, letters: letters.map((l) => l.body) };
}, uid);

let state = { planka: null, letters: [] };
for (let i = 0; i < 30; i++) {
  await page.waitForTimeout(2000);
  state = await read();
  if (state.letters.some((l) => l.includes("Недельное обновление планки"))) break;
}
console.log(`\nпланка: ${state.planka}`);
const letter = state.letters.find((l) => l.includes("Недельное обновление планки")) ?? "";
console.log("письмо:\n" + letter.split("\n").filter(Boolean).map((s) => "  " + s).join("\n"));

check("пересчёт состоялся", !!letter);
check("планка НЕ выросла", state.planka !== null && state.planka <= PLANKA,
  `было ${PLANKA}, стало ${state.planka}`);
check("письмо называет причину", letter.includes("вы недоедали"));
check("письмо называет, куда звал расчёт", letter.includes("необходимо поднять планку"));
check("письмо требует держаться планки", letter.includes("чётко следовать вашей планке"));

await b.close();
console.log(fail ? `\n${fail} провалов` : "\nвсё сошлось");
process.exit(fail ? 1 : 0);
