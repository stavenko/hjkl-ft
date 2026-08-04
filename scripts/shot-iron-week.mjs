// Скриншоты недели железа: виджет с недельным gauge + индикатором и все кадры
// пятой истории. Состояние сеется той же функцией, что и в check-iron-week.mjs.
import { chromium } from "playwright";
import { openSeeded, DEFAULT_URL } from "./harness.mjs";

// Край CDN держит предыдущую сборку ощутимо дольше, чем распространяется деплой,
// поэтому сразу после выкатки снимать надо с ПРЯМОГО адреса деплоя
// (FE=https://<id>.renorma-fit-dev.pages.dev) — там кэша нет.
const BASE = process.env.FE || DEFAULT_URL;
const OUT = process.env.OUT || "/private/tmp/claude-501/-Users-vasilijstavenko-projects-hjkl-ft/56df53af-a1ed-4117-8e82-8a1f8aad90e8/scratchpad/iron";

function seed({ ironOpenDaysAgo }) {
  return async (page, uid) => {
    await page.evaluate(async ({ uid, ironOpenDaysAgo }) => {
      const db = await new Promise((res, rej) => {
        const r = indexedDB.open(`hjkl-ft-${uid}`);
        r.onsuccess = () => res(r.result); r.onerror = () => rej(r.error);
      });
      const nowIso = new Date().toISOString();
      const ymd = (back) => {
        const d = new Date(); d.setDate(d.getDate() - back);
        return `${d.getFullYear()}-${String(d.getMonth() + 1).padStart(2, "0")}-${String(d.getDate()).padStart(2, "0")}`;
      };
      const app_flags = [
        { key: "push_onboarding_dismissed", value: "true" },
        { key: "activity_week_unlocked", value: "true" },
        { key: "steps_gate_opened_at", value: ymd(30) },
        { key: "calcium_week_unlocked", value: "true" },
        { key: "calcium_gate_opened_at", value: ymd(9) },
        { key: "ind_opened_at", value: ymd(40) },
        { key: "iron_week_unlocked", value: "true" },
        { key: "iron_week_opened_at", value: ymd(ironOpenDaysAgo) },
        { key: "ft_subscription", value: JSON.stringify({
            plan: "monthly", end: Date.now() + 30 * 24 * 3600 * 1000, active: true,
            start: Date.now(), status: "paid", no_renew: false, provider: "lava" }) },
      ];
      const profile = [{ key: "profile", sex: "male", height_cm: 180,
        birth_year: new Date().getFullYear() - 45, steps_planka: 10000,
        created_at: nowIso, updated_at: nowIso }];
      const goals = [
        { id: "g-cal", nutrient: "Calories", key: "calories", direction: "AtMost",
          amount: 2500, unit: "Kcal", period: "Day", created_at: nowIso, updated_at: nowIso },
        { id: "g-ca", nutrient: "Кальций", key: "calcium", direction: "AtLeast",
          amount: 1000, unit: "Mg", period: "Day", created_at: nowIso, updated_at: nowIso },
      ];
      const foods = [{
        id: "f-liver", name: "Куриная печень", kcal: 137, protein: 20, fat: 6, carbs: 1,
        nutrients: { "Кальций": 1200, "Клетчатка": 30, "Железо": 9.9 },
        package_weight: null, is_recipe: false, recipe_id: null, archived: false,
        is_restaurant: false, is_snack: false, is_liquid_cal: false, is_veg_fruit: false,
        is_egg: false, is_red_meat: true, iron_mg: 9.0, iron_absorption: 0.25,
        created_at: nowIso, updated_at: nowIso,
      }];
      const diary = [];
      for (let i = 0; i < 10; i++) {
        diary.push({ id: `d-${i}`, food_id: "f-liver", date: ymd(i), time: null, grams: 200,
          waste_grams: 0, meal_label: null, deleted: false, created_at: nowIso, updated_at: nowIso });
      }
      for (const [store, rows] of Object.entries({ app_flags, profile, goals, foods, diary })) {
        await new Promise((res, rej) => {
          const tx = db.transaction([store], "readwrite");
          const os = tx.objectStore(store);
          for (const row of rows) os.put(row);
          tx.oncomplete = () => res(); tx.onerror = () => rej(tx.error);
        });
      }
      db.close();
    }, { uid, ironOpenDaysAgo });
  };
}

const b = await chromium.launch({ headless: true });
const { context, page } = await openSeeded(b, {
  baseUrl: BASE, context: { serviceWorkers: "block" },
  uid: `iron-shot-${Math.floor(Math.random() * 1e6)}`,
  seed: seed({ ironOpenDaysAgo: 2 }),
});
// Край CDN может держать ПРЕДЫДУЩУЮ версию картинки: запрос без параметра
// попадает в его кэш, и на снимке оказывается старое изображение. Дописываем
// cache-buster к каждой картинке истории.
await page.route("**/story-img/**", (route) => {
  const u = new URL(route.request().url());
  u.searchParams.set("cb", String(Date.now()));
  return route.continue({ url: u.toString() });
});
await page.setViewportSize({ width: 430, height: 932 });
await page.waitForTimeout(9000);

// 1. Дашборд с недельным gauge и индикатором.
await page.screenshot({ path: `${OUT}/dashboard.png` });
// 2. Только виджет — крупно.
await page.locator('[data-testid="progress-widget"]').screenshot({ path: `${OUT}/widget.png` });

// 3. Кадры пятой истории.
await page.locator('[data-story-id="week5"]').click();
await page.waitForTimeout(1500);
for (let i = 1; i <= 8; i++) {
  // Ждём, пока картинка кадра реально ДОГРУЗИТСЯ, иначе на снимке пустой фон.
  await page.waitForFunction(
    () => [...document.querySelectorAll("img")].every((im) => im.complete && im.naturalWidth > 0),
    null, { timeout: 15000 },
  ).catch(() => {});
  await page.waitForTimeout(400);
  await page.screenshot({ path: `${OUT}/story-${i}.png` });
  // Тап в правую часть экрана — следующий кадр.
  await page.mouse.click(380, 500);
  await page.waitForTimeout(900);
}

await context.close();
await b.close();
console.log("снято");
