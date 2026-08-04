// Еженедельный пересчёт планки по шагам — на РЕАЛЬНОМ пути: приложение
// запускается с посеянной базой и само поднимает планку при старте.
//
// Сигнал — цвет индикатора шагов за 7 ЗАВЕРШЁННЫХ дней (то, что видно на
// виджете): красный — держим, жёлтый — +1000, зелёный — на следующую ступень
// (7000 → 10000, дальше по тысяче), потолок 15000.
import { chromium } from "playwright";
import { openSeeded, DEFAULT_URL } from "./harness.mjs";

const BASE = process.env.FE || DEFAULT_URL;

let fail = 0;
const check = (name, ok, extra = "") => { console.log(`${ok ? "OK " : "FAIL"} ${name}${extra ? " — " + extra : ""}`); if (!ok) fail++; };

// Сеем: неделя активности открыта, планка задана, ворота открыты `gateDaysAgo`
// дней назад (это же — якорь недельных часов), и заморожены дни индикатора:
// первые `misses` из 7 завершённых дней — недобор.
function seeder({ planka, misses, gateDaysAgo, anchorDaysAgo, noStepDays }) {
  return async (page, uid) => {
    await page.evaluate(
      async ({ uid, planka, misses, gateDaysAgo, anchorDaysAgo, noStepDays }) => {
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
          { key: "steps_gate_opened_at", value: ymd(gateDaysAgo) },
        ];
        if (anchorDaysAgo != null) {
          app_flags.push({ key: "steps_planka_weekly_anchor", value: ymd(anchorDaysAgo) });
        }

        const profile = [{
          key: "profile", steps_planka: planka, sex: "male", height_cm: 180,
          birth_year: 1980, created_at: nowIso, updated_at: nowIso,
        }];

        // 7 завершённых дней (вчера … 7 дней назад). Недобор — ratio < 1.0.
        const ind_steps = [];
        const step_entries = [];
        for (let i = 1; i <= 7; i++) {
          if (noStepDays) continue; // совсем нет данных → индикатор серый
          const missed = i <= misses;
          const value = missed ? planka * 0.5 : planka * 1.1;
          ind_steps.push({ date: ymd(i), value, ratio: value / planka, computed_at: nowIso });
          step_entries.push({ id: `seed-s-${i}`, date: ymd(i), steps: Math.round(value), created_at: nowIso, updated_at: nowIso });
        }

        const records = { app_flags, profile, ind_steps, step_entries };
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
      { uid, planka, misses, gateDaysAgo, anchorDaysAgo, noStepDays },
    );
  };
}

// Планка и письма ПОСЛЕ того, как приложение отработало запуск.
async function readState(page) {
  return await page.evaluate(async () => {
    const uid = localStorage.getItem("user_id");
    const db = await new Promise((res, rej) => {
      const r = indexedDB.open(`hjkl-ft-${uid}`);
      r.onsuccess = () => res(r.result);
      r.onerror = () => rej(r.error);
    });
    const get = (store, key) => new Promise((res) => {
      const rq = db.transaction([store], "readonly").objectStore(store).get(key);
      rq.onsuccess = () => res(rq.result);
      rq.onerror = () => res(undefined);
    });
    const profile = await get("profile", "profile");
    const letters = await get("app_flags", "letters_v1");
    const anchor = await get("app_flags", "steps_planka_weekly_anchor");
    db.close();
    return {
      planka: profile && profile.steps_planka,
      letters: letters && letters.value ? JSON.parse(letters.value) : [],
      anchor: anchor && anchor.value,
    };
  });
}

const b = await chromium.launch({ headless: true });

async function scenario(name, seed, expect) {
  const { context, page } = await openSeeded(b, {
    baseUrl: BASE,
    uid: `steps-${name}-${Math.floor(Math.random() * 1e6)}`,
    seed: seeder(seed),
  });
  // Пересчёт идёт в запускном хвосте. Ждём ПРИЗНАК его завершения (сдвинутый
  // якорь), а не фиксированную паузу — холодный старт с загрузкой wasm бывает
  // сильно дольше. Там, где решения быть не должно, ждём весь бюджет и потом
  // проверяем — это только усиливает отрицательную проверку.
  const today = new Date().toISOString().slice(0, 10);
  let st = null;
  for (let i = 0; i < 40; i++) {
    st = await readState(page);
    if (st.anchor === today) break;
    await page.waitForTimeout(500);
  }
  check(`${name}: планка ${expect.planka}`, st.planka === expect.planka, `получили ${st.planka}`);
  const letter = st.letters.find((l) => l.id.startsWith("steps-planka-"));
  check(`${name}: письмо ${expect.letter ? "есть" : "не приходило"}`, !!letter === expect.letter,
    letter ? letter.body.replace(/\s+/g, " ").slice(0, 70) : "письма нет");
  if (expect.letter && letter) {
    check(`${name}: в письме новая планка`, letter.body.includes(String(expect.planka).replace(/\B(?=(\d{3})+(?!\d))/g, " ")),
      letter.body.replace(/\s+/g, " ").slice(0, 90));
  }
  check(`${name}: якорь ${expect.anchorMoved ? "сдвинут на сегодня" : "не сдвинут"}`,
    (st.anchor === today) === expect.anchorMoved, `якорь ${st.anchor}`);
  await context.close();
}

// Зелёная неделя (0 недоборов) — на следующую ступень.
await scenario("зелёная-7000", { planka: 7000, misses: 0, gateDaysAgo: 8 }, { planka: 10000, letter: true, anchorMoved: true });
await scenario("зелёная-10000", { planka: 10000, misses: 0, gateDaysAgo: 8 }, { planka: 11000, letter: true, anchorMoved: true });
// Жёлтая (1–3 недобора) — ровно +1000.
await scenario("жёлтая-10000", { planka: 10000, misses: 2, gateDaysAgo: 8 }, { planka: 11000, letter: true, anchorMoved: true });
// Красная (≥4) — держим, письма нет, но неделя засчитана.
await scenario("красная-10000", { planka: 10000, misses: 5, gateDaysAgo: 8 }, { planka: 10000, letter: false, anchorMoved: true });
// Потолок — выше 15000 не поднимаем.
await scenario("потолок-15000", { planka: 15000, misses: 0, gateDaysAgo: 8 }, { planka: 15000, letter: false, anchorMoved: true });
await scenario("потолок-14500", { planka: 14500, misses: 0, gateDaysAgo: 8 }, { planka: 15000, letter: true, anchorMoved: true });
// Неделя ещё не прошла — не трогаем ничего.
await scenario("рано-3-дня", { planka: 10000, misses: 0, gateDaysAgo: 3 }, { planka: 10000, letter: false, anchorMoved: false });
// Ни одной записи шагов за неделю. По действующей семантике индикатора день без
// записи — это ПРОВАЛ (steps — «классификаторный» ключ, см. is_classifier), то
// есть неделя красная: планку не поднимаем, но неделю засчитываем.
await scenario("без-записей", { planka: 10000, misses: 0, gateDaysAgo: 8, noStepDays: true }, { planka: 10000, letter: false, anchorMoved: true });

console.log(fail === 0 ? "\n=== ALL OK ===" : `\n=== FAILURES: ${fail} ===`);
await b.close();
process.exit(fail === 0 ? 0 : 1);
