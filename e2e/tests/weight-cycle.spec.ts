import { test, expect, type Page } from '@playwright/test';
import { registerAccount } from './helpers';

/**
 * Female-only menstrual-cycle readout in the weight chart modal: cycle detection
 * (period · swing) plus the de-cycled current weight. Pure client-side over
 * IndexedDB `weight_entries`; gated on profile sex == female.
 */

// 90 daily entries (ending today) = base + slope·day + amplitude·sin(2π·day/period).
async function seedCyclicWeights(page: Page, sex: 'female' | 'male', amplitude: number) {
  await page.evaluate(async ([sexVal, amp]) => {
    if (sexVal) localStorage.setItem('profile_sex', sexVal as string);

    const open = indexedDB.open('hjkl-ft');
    const db: IDBDatabase = await new Promise((resolve, reject) => {
      open.onsuccess = () => resolve(open.result);
      open.onerror = () => reject(open.error);
    });
    const iso = (d: Date) => d.toISOString().slice(0, 10);
    const today = new Date();
    const N = 90;
    const period = 28;

    const wtx = db.transaction('weight_entries', 'readwrite');
    for (let i = N - 1; i >= 0; i--) {
      const d = new Date(today);
      d.setDate(today.getDate() - i);
      const day = N - 1 - i;
      const cyc = (amp as number) * Math.sin((2 * Math.PI * day) / period);
      wtx.objectStore('weight_entries').put({
        id: `wc-${i}`,
        date: iso(d),
        weight_kg: 75.0 - 0.03 * day + cyc,
        no_water: false, no_food: false, no_wash: false, used_toilet: false, morning: true,
        created_at: d.toISOString(), updated_at: d.toISOString(),
      });
    }
    await new Promise<void>((res, rej) => { wtx.oncomplete = () => res(); wtx.onerror = () => rej(wtx.error); });

    const stx = db.transaction('story', 'readwrite');
    const ts = today.toISOString();
    for (const key of ['language_configured', 'notification_received']) {
      stx.objectStore('story').put({ key, value: true, updated_at: ts });
    }
    await new Promise<void>((res, rej) => { stx.oncomplete = () => res(); stx.onerror = () => rej(stx.error); });
    db.close();
  }, [sex, amplitude] as const);
}

async function openWeightModal(page: Page, sex: 'female' | 'male', amplitude: number) {
  await page.goto('/');
  await page.evaluate(() => localStorage.clear());
  await page.reload();
  await page.waitForTimeout(3000);
  await registerAccount(page);
  await seedCyclicWeights(page, sex, amplitude);
  await page.reload();
  await page.getByTestId('nav-diary').click();
  await page.getByTestId('diary-weight-widget').waitFor({ state: 'visible', timeout: 15_000 });
  await page.getByTestId('diary-weight-widget').click();
  await page.locator('.modal.is-active').waitFor({ state: 'visible', timeout: 10_000 });
}

test('female with a clear cycle: modal shows detection + de-cycled weight', async ({ page }) => {
  await openWeightModal(page, 'female', 1.0);
  const modal = page.locator('.modal.is-active');
  await expect(modal).toContainText('Месячные:');
  await expect(modal).toContainText('Вес без месячных:');
  await expect(modal).toContainText('дн'); // "~28 дн · ±1.0 кг"
});

test('female cycle: de-cycled weight equals the trend baseline, not the raw value', async ({ page }) => {
  // Seed: weight = 75 − 0.03·day + 1.0·sin(2π·day/28), 90 days. Latest day = 89,
  // so the trend baseline today is 75 − 0.03·89 = 72.33; the raw weight carries
  // the cyclic offset on top. The "Вес без месячных" line must show ~72.3.
  await openWeightModal(page, 'female', 1.0);
  const modal = page.locator('.modal.is-active');
  const text = (await modal.textContent()) ?? '';

  const m = text.match(/Вес без месячных:\s*([\d.]+)/);
  expect(m, `de-cycled value not found in modal: ${text}`).not.toBeNull();
  const decycled = parseFloat(m![1]);
  expect(decycled, `de-cycled ${decycled} should be ~72.3 (trend baseline)`).toBeGreaterThan(71.8);
  expect(decycled).toBeLessThan(72.9);

  // And it must differ from the raw current weight (which includes the cycle).
  const rawText = (await page.getByTestId('weight-widget-value').textContent()) ?? '';
  const raw = parseFloat(rawText.match(/([\d.]+)/)![1]);
  expect(Math.abs(raw - decycled), `correction should be non-trivial (raw ${raw}, decycled ${decycled})`)
    .toBeGreaterThan(0.4);
});

test('male profile: no menstrual block shown', async ({ page }) => {
  await openWeightModal(page, 'male', 1.0);
  await expect(page.locator('.modal.is-active')).not.toContainText('Месячные');
});
