// Снимки всех кадров истории шестой недели — чтобы смотреть текст глазами
// человека, а не в исходнике.
//
// Открывает кружок «6» в трее и щёлкает по правой зоне, снимая каждый кадр.
// Файлы: OUTDIR/story6-01.png … story6-08.png.
import { chromium } from "playwright";
import { openSeeded, DEFAULT_URL } from "./harness.mjs";
import { mkdirSync } from "node:fs";
import path from "node:path";

const BASE = process.env.FE || DEFAULT_URL;
const OUTDIR = process.env.OUTDIR || "/tmp/story6";
mkdirSync(OUTDIR, { recursive: true });

const seed = async (page, uid) => {
  await page.evaluate(async ({ uid }) => {
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
      { key: "db_schema_version", value: "999" },
      { key: "push_onboarding_dismissed", value: "true" },
      { key: "welcome_shown", value: "true" },
      { key: "activity_week_unlocked", value: "true" },
      { key: "calcium_week_unlocked", value: "true" },
      { key: "ind_opened_at", value: ymd(40) },
      { key: "iron_week_unlocked", value: "true" },
      { key: "iron_week_opened_at", value: ymd(10) },
      { key: "fat_week_unlocked", value: "true" },
      { key: "fat_week_opened_at", value: ymd(3) },
      { key: "ft_subscription", value: JSON.stringify({
          plan: "monthly", end: Date.now() + 30 * 864e5, active: true,
          start: Date.now(), status: "paid", no_renew: false, provider: "lava" }) },
    ];
    const profile = [{ key: "profile", sex: "male", height_cm: 180,
      birth_year: new Date().getFullYear() - 45, goal: "lose", steps_planka: 9000,
      created_at: nowIso, updated_at: nowIso }];
    const goals = [{ id: "g-cal", nutrient: "Calories", key: "calories", direction: "AtMost",
      amount: 2600, unit: "Kcal", period: "Day", created_at: nowIso, updated_at: nowIso }];
    const avail = Array.from(db.objectStoreNames);
    for (const [store, rows] of Object.entries({ app_flags, profile, goals })) {
      if (!avail.includes(store)) continue;
      await new Promise((res, rej) => {
        const tx = db.transaction([store], "readwrite");
        for (const row of rows) tx.objectStore(store).put(row);
        tx.oncomplete = () => res(); tx.onerror = () => rej(tx.error);
      });
    }
    db.close();
  }, { uid });
};

const b = await chromium.launch({ headless: true });
const { context, page } = await openSeeded(b, {
  baseUrl: BASE,
  context: { serviceWorkers: "block", viewport: { width: 430, height: 932 }, deviceScaleFactor: 2 },
  uid: `story6-${Math.floor(Math.random() * 1e6)}`,
  seed,
});
await page.reload({ waitUntil: "domcontentloaded" });
await page.waitForTimeout(6000);

await page.locator('[data-story-id="week6"]').click();
await page.waitForTimeout(1500);

const FRAMES = 8;
for (let i = 1; i <= FRAMES; i++) {
  // Обложкам дать ЗАГРУЗИТЬСЯ, а не только отрисоваться: они по 100–300 КБ и с
  // паузой в 900 мс кадр снимался пустым — это выглядело как «картинка не встала».
  await page.waitForTimeout(2500);
  await page.evaluate(() => Promise.all(
    Array.from(document.images).filter((i) => !i.complete)
      .map((i) => new Promise((r) => { i.onload = i.onerror = r; }))));
  await page.waitForTimeout(400);
  const file = path.join(OUTDIR, `story6-${String(i).padStart(2, "0")}.png`);
  await page.screenshot({ path: file });
  const text = (await page.locator("body").innerText()).replace(/\s+/g, " ").slice(0, 90);
  console.log(`${i}: ${text}`);
  if (i < FRAMES) {
    // Правая зона — «дальше». Тапаем ближе к краю, чтобы не попасть в левую треть.
    await page.mouse.click(380, 500);
  }
}

await context.close();
await b.close();
console.log(`\nснимки в ${OUTDIR}`);
