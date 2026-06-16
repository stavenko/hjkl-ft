import { test, expect, type Page } from '@playwright/test';
import { registerAccount } from './helpers';

/**
 * Weight-trend UI: the widget colours the current weight by energy balance
 * (green = deficit/losing, pink = surplus/gaining, default = maintenance), and
 * the chart modal shows a 14-day trend summary. Pure client-side computation
 * over IndexedDB `weight_entries` — no server involved.
 */

// Seed `weight_entries` (one per day, last `n` days ending today) plus the story
// flags that reveal the dashboard widgets, then reload.
async function seedWeights(page: Page, startKg: number, slopePerDay: number, n: number) {
  await page.evaluate(async ([start, slope, count]) => {
    const open = indexedDB.open('hjkl-ft');
    const db: IDBDatabase = await new Promise((resolve, reject) => {
      open.onsuccess = () => resolve(open.result);
      open.onerror = () => reject(open.error);
    });
    const iso = (d: Date) => d.toISOString().slice(0, 10);
    const today = new Date();

    const wtx = db.transaction('weight_entries', 'readwrite');
    for (let i = (count as number) - 1; i >= 0; i--) {
      const d = new Date(today);
      d.setDate(today.getDate() - i);
      const day = (count as number) - 1 - i;
      // Deterministic clean line: a confident trend reads ~100%, a flat line 50%
      // (unclear). Statistical behaviour under noise is covered by unit tests.
      wtx.objectStore('weight_entries').put({
        id: `wt-${i}`,
        date: iso(d),
        weight_kg: (start as number) + (slope as number) * day,
        no_water: false, no_food: false, no_wash: false, used_toilet: false, morning: true,
        created_at: d.toISOString(), updated_at: d.toISOString(),
      });
    }
    await new Promise<void>((res, rej) => { wtx.oncomplete = () => res(); wtx.onerror = () => rej(wtx.error); });

    // Reveal dashboard widgets (setup_done flags).
    const stx = db.transaction('story', 'readwrite');
    const ts = today.toISOString();
    for (const key of ['language_configured', 'notification_received']) {
      stx.objectStore('story').put({ key, value: true, updated_at: ts });
    }
    await new Promise<void>((res, rej) => { stx.oncomplete = () => res(); stx.onerror = () => rej(stx.error); });
    db.close();
  }, [startKg, slopePerDay, n] as const);
}

async function openDiaryWithWeights(page: Page, startKg: number, slope: number, n: number) {
  await page.goto('/');
  await page.evaluate(() => localStorage.clear());
  await page.reload();
  await page.waitForTimeout(3000);
  await registerAccount(page);
  await seedWeights(page, startKg, slope, n);
  await page.reload();
  await page.getByTestId('nav-diary').click();
  await page.getByTestId('diary-weight-widget').waitFor({ state: 'visible', timeout: 15_000 });
}

function colorOf(page: Page, testId: string) {
  return page.getByTestId(testId).evaluate((el) => getComputedStyle(el).color);
}
function rgb(s: string): [number, number, number] {
  const m = s.match(/\d+/g)!.map(Number);
  return [m[0], m[1], m[2]];
}

test('losing weight: widget is green and modal says "Снижается"', async ({ page }) => {
  await openDiaryWithWeights(page, 90.0, -0.1, 14); // ~0.7 kg/week down

  const [r, g, b] = rgb(await colorOf(page, 'weight-widget-value'));
  expect(g, `green channel should dominate (got ${r},${g},${b})`).toBeGreaterThan(r);
  expect(g).toBeGreaterThan(b);

  await page.getByTestId('diary-weight-widget').click();
  const modal = page.locator('.modal.is-active');
  await expect(modal).toContainText('Снижается');
  await expect(modal).toContainText('достоверность');
  await expect(modal).toContainText('%');
});

test('gaining weight: widget is pink and modal says "Растёт"', async ({ page }) => {
  await openDiaryWithWeights(page, 70.0, 0.12, 14); // gaining

  const [r, g, b] = rgb(await colorOf(page, 'weight-widget-value'));
  // pink #e0699b ~ rgb(224,105,155): red highest, blue > green.
  expect(r, `red should dominate (got ${r},${g},${b})`).toBeGreaterThan(g);
  expect(b).toBeGreaterThan(g);

  await page.getByTestId('diary-weight-widget').click();
  await expect(page.locator('.modal.is-active')).toContainText('Растёт');
});

test('flat weight: modal reports unclear / maintenance', async ({ page }) => {
  await openDiaryWithWeights(page, 80.0, 0.0, 14); // no real trend

  await page.getByTestId('diary-weight-widget').click();
  await expect(page.locator('.modal.is-active')).toContainText('Тренд пока не ясен');
});
