import { test, expect, type Page } from '@playwright/test';
import { registerAccount } from './helpers';

/**
 * Diary uses document scroll (not a fixed shell + inner overflow) with a sticky
 * date row. Desktop sanity: with enough entries the WINDOW scrolls (the old fixed
 * shell would keep window.scrollY == 0), and the date row stays pinned near the
 * top. (The iOS PiP/resume freeze itself is iOS-WebKit only — device test.)
 */

const TS = '2026-06-17T10:00:00Z';

async function seedManyEntries(page: Page, userId: string) {
  await page.evaluate(async ({ ts, userId }) => {
    const d = new Date();
    const today = `${d.getFullYear()}-${String(d.getMonth() + 1).padStart(2, '0')}-${String(d.getDate()).padStart(2, '0')}`;
    // The diary now reads from the PER-USER database (`hjkl-ft-<user_id>`); the
    // legacy shared `hjkl-ft` DB is no longer the active store after login.
    const open = indexedDB.open(`hjkl-ft-${userId}`);
    const db: IDBDatabase = await new Promise((res, rej) => { open.onsuccess = () => res(open.result); open.onerror = () => rej(open.error); });
    const ftx = db.transaction('foods', 'readwrite');
    ftx.objectStore('foods').put({
      id: 'f-scroll', name: 'Курица', kcal: 100, protein: 10, fat: 5, carbs: 20, nutrients: {},
      package_weight: null, is_recipe: false, recipe_id: null, archived: false, is_restaurant: false,
      created_at: ts, updated_at: ts,
    });
    await new Promise<void>((res, rej) => { ftx.oncomplete = () => res(); ftx.onerror = () => rej(ftx.error); });
    const dtx = db.transaction('diary', 'readwrite');
    for (let i = 0; i < 25; i++) {
      dtx.objectStore('diary').put({
        id: `de-${i}`, food_id: 'f-scroll', date: today, time: '12:00', grams: 100, waste_grams: 0,
        meal_label: null, deleted: false, created_at: ts, updated_at: ts,
      });
    }
    await new Promise<void>((res, rej) => { dtx.oncomplete = () => res(); dtx.onerror = () => rej(dtx.error); });
    db.close();
  }, { ts: TS, userId });
}

test('diary scrolls the document and the date row is sticky', async ({ page }) => {
  await page.goto('/');
  await page.evaluate(() => localStorage.clear());
  await page.reload();
  await page.waitForTimeout(3000);
  const { userId } = await registerAccount(page);
  await seedManyEntries(page, userId);
  await page.reload();
  await page.getByTestId('nav-diary').click();
  await page.getByTestId('diary-btn-add').waitFor({ state: 'visible', timeout: 15_000 });

  // The page is taller than the viewport (content overflows).
  const scrollable = await page.evaluate(() => document.documentElement.scrollHeight > window.innerHeight + 50);
  expect(scrollable, 'diary content should overflow the viewport').toBe(true);

  const dateTopBefore = await page.getByTestId('diary-btn-date').evaluate((el) => el.getBoundingClientRect().top);

  // Scroll DEEP into the list — past where a short header wrapper would have
  // released the sticky row (regression guard for "sticky detaches mid-scroll").
  await page.evaluate(() => window.scrollTo(0, Math.floor(document.documentElement.scrollHeight * 0.7)));
  await page.waitForTimeout(300);

  // The document actually scrolled (a fixed shell would have kept scrollY at 0).
  const scrollY = await page.evaluate(() => window.scrollY);
  expect(scrollY, 'window should scroll').toBeGreaterThan(300);

  // The sticky date row is STILL pinned near the top deep into the scroll.
  const dateTopAfter = await page.getByTestId('diary-btn-date').evaluate((el) => el.getBoundingClientRect().top);
  expect(dateTopAfter, `date row should stay pinned deep in the scroll (before ${dateTopBefore}, after ${dateTopAfter})`).toBeLessThan(80);
});
