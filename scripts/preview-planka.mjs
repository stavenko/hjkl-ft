// One-off preview: seed 7 days of diary whose daily totals average ~2895 kcal,
// open ch3-fat, and screenshot the calorie-planka widget.
//
//   node scripts/preview-planka.mjs [baseUrl] [outPath]

import { chromium } from "playwright";
import { openSeeded, planFor } from "./harness.mjs";

const BASE = process.argv[2] || "https://hjkl-ft.pages.dev";
const OUT = process.argv[3] || "planka-preview.png";

// Daily totals → average exactly 2895 kcal over 7 days.
const TOTALS = [2700, 3100, 2850, 2950, 2800, 3000, 2865];

const browser = await chromium.launch({ headless: true });
const { context, page } = await openSeeded(browser, {
  baseUrl: BASE,
  plan: planFor("3.0"),
  landing: "/story/ch3-fat",
  // Custom seed: one food at 1000 kcal/100 g, grams = total/10 ⇒ daily total kcal.
  seed: async (page, uid) => {
    await page.evaluate(
      async ({ uid, totals }) => {
        const open = (name) =>
          new Promise((res, rej) => {
            const r = indexedDB.open(name);
            r.onsuccess = () => res(r.result);
            r.onerror = () => rej(r.error);
          });
        const db = await open(`hjkl-ft-${uid}`);
        const nowIso = new Date().toISOString();
        const ymd = (off) => {
          const d = new Date();
          d.setDate(d.getDate() - off);
          return `${d.getFullYear()}-${String(d.getMonth() + 1).padStart(2, "0")}-${String(d.getDate()).padStart(2, "0")}`;
        };
        const put = (store, rows) =>
          new Promise((res, rej) => {
            const tx = db.transaction([store], "readwrite");
            const os = tx.objectStore(store);
            rows.forEach((r) => os.put(r));
            tx.oncomplete = () => res();
            tx.onerror = () => rej(tx.error);
          });

        await put("foods", [{
          id: "preview-food", name: "Рацион дня", kcal: 1000, protein: 50, fat: 40, carbs: 100,
          nutrients: {}, package_weight: null, is_recipe: false, recipe_id: null,
          archived: false, is_restaurant: false, is_snack: false, created_at: nowIso, updated_at: nowIso,
        }]);
        await put("diary", totals.map((t, i) => ({
          id: `prev-d-${i}`, food_id: "preview-food", date: ymd(i), time: null,
          grams: t / 10, waste_grams: 0, meal_label: null, deleted: false,
          created_at: nowIso, updated_at: nowIso,
        })));
        db.close();
      },
      { uid, totals: TOTALS },
    );
  },
});

await page.getByRole("button", { name: /Принять планку/ }).waitFor({ timeout: 8000 });
await page.screenshot({ path: OUT, fullPage: true });
const txt = await page.locator("body").innerText();
const m = /Принять планку\s+(\d+)\s+ккал/.exec(txt);
console.log("planka button:", m ? m[0] : "(not found)");
await context.close();
await browser.close();
