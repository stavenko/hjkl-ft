import { test, expect, type Page } from '@playwright/test';
import { registerAccount } from './helpers';

/**
 * Cross-device sync — PRODUCTION reality checks.
 *
 * These run against the deployed Pages site (playwright.config baseURL).
 * The wire format is postcard (binary), so we don't mock the server here —
 * we only inspect what the client SENDS and whether the deployment answers.
 * Each assertion encodes desired behaviour, so a failure pins a real gap.
 *
 * The genuine two-device data-propagation scenario needs a real sync server
 * speaking postcard; that lives in `sync-local.spec.ts` against the dev stack.
 */

const TS = '2026-06-16T10:00:00Z';
async function seedFood(page: Page, name = 'Курица') {
  await page.evaluate(async ([n, ts]) => {
    const open = indexedDB.open('hjkl-ft');
    const db: IDBDatabase = await new Promise((resolve, reject) => {
      open.onsuccess = () => resolve(open.result);
      open.onerror = () => reject(open.error);
    });
    const tx = db.transaction('foods', 'readwrite');
    tx.objectStore('foods').put({
      id: crypto.randomUUID(), name: n, kcal: 100, protein: 10, fat: 5, carbs: 20,
      nutrients: {}, package_weight: null, is_recipe: false, recipe_id: null,
      archived: false, is_restaurant: false, created_at: ts, updated_at: ts,
    });
    await new Promise<void>((resolve, reject) => {
      tx.oncomplete = () => resolve();
      tx.onerror = () => reject(tx.error);
    });
    db.close();
  }, [name, TS] as const);
}

/** Log the first food in the diary via the real UI, which fires sync::push_background(). */
async function logFoodInDiary(page: Page) {
  await page.getByTestId('nav-diary').click();
  await page.getByTestId('diary-btn-add').click({ timeout: 10_000 });
  await page.getByTestId('diary-add-btn-pick-food').first().click({ timeout: 10_000 });
  await page.getByTestId('diary-add-weight-btn-confirm').click({ timeout: 10_000 });
}

// ---------------------------------------------------------------------------
test('production has a deployed sync server (sync-worker)', async ({ page }) => {
  await page.goto('/');
  // The app reads sync_base_url from /config/frontend.toml and POSTs there.
  const base = await page.evaluate(async () => {
    const txt = await (await fetch('/config/frontend.toml')).text();
    return txt.match(/sync_base_url\s*=\s*"([^"]*)"/)?.[1] ?? '';
  });
  expect(base, 'sync_base_url must be configured').toBeTruthy();

  const status = await page.evaluate(async (b) => {
    const r = await fetch(`${b}/sync/dump`, {
      method: 'POST',
      headers: { 'content-type': 'application/json' },
      body: '{}',
    });
    return r.status;
  }, base);
  // A real server answers: 401 unauthenticated (or 200). 404/405 would mean no
  // worker is wired. (Was 405 against the SPA origin before sync-worker existed.)
  expect([200, 401], `sync server returned ${status}`).toContain(status);
});

// ---------------------------------------------------------------------------
test('sync push is authenticated so the server can scope data to a user', async ({ page }) => {
  await page.goto('/');
  await page.evaluate(() => localStorage.clear());
  await page.reload();
  await page.waitForTimeout(3000);
  await registerAccount(page);

  await seedFood(page);
  await page.reload();
  await page.getByTestId('nav-diary').waitFor({ state: 'visible', timeout: 15_000 });

  const pushReq = page.waitForRequest('**/sync/push', { timeout: 15_000 });
  await logFoodInDiary(page);
  const req = await pushReq;

  // api::post sends no Authorization header → the server cannot tell which user
  // (or which device's account) the data belongs to. "Same credentials" can't
  // route to the same dataset.
  expect(
    req.headers()['authorization'],
    'sync push carries no Authorization header → sync is not user-scoped',
  ).toBeTruthy();
});

// ---------------------------------------------------------------------------
test('a launched device re-pulls remote changes even with local data present', async ({ page, context }) => {
  await page.goto('/');
  await page.evaluate(() => localStorage.clear());
  await page.reload();
  await page.waitForTimeout(3000);
  await registerAccount(page);
  await seedFood(page); // local DB is now non-empty

  let dumps = 0;
  await context.route('**/sync/dump', async (route) => {
    dumps += 1;
    await route.fulfill({ status: 200, body: '' });
  });

  await page.reload();
  await page.getByTestId('nav-diary').waitFor({ state: 'visible', timeout: 15_000 });
  await page.waitForTimeout(2500);

  // lib.rs pulls only when `is_empty()` (foods+goals empty). Once a device has
  // synced once, it never pulls again → deletions/updates made on another
  // device (scenario step 4) never reach it. The launch should re-pull.
  expect(
    dumps,
    'no /sync/dump on launch with local data → device never receives later remote changes',
  ).toBeGreaterThan(0);
});
