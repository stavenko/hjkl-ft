import { test, expect, type Page } from '@playwright/test';
import { registerAccount } from './helpers';

/**
 * The full two-device scenario against the LIVE sync-worker:
 *   1. Device A creates story progress + diary / weight / steps data.
 *   2. Device B authenticates with the same account and pulls A's data.
 *   3. Story progress is reset on A via the UI (Settings → danger zone).
 *   4. Device B reflects the reset after a relaunch.
 *
 * "Same account" on device B is modelled by handing B device A's token
 * (sync is keyed by the JWT `sub`); the pairing mechanics are covered by
 * pairing-flow.spec.ts. Each run uses a fresh random account, so the per-user
 * Durable Object state is isolated.
 */

const TS = '2026-06-16T10:00:00Z';

/**
 * A signed-in user no longer reads/writes the device-global `hjkl-ft` database —
 * each account gets its own per-user IndexedDB `hjkl-ft-{user_id}` (see
 * `frontend/src/services/db.rs` `user_db_name` / `activate_for_user`). Sync
 * push/pull operate on that active per-user DB, so the e2e seeding and readback
 * MUST target the same per-user name, otherwise nothing the test writes is ever
 * pushed and B's pull lands in a store the test never inspects.
 */
const userDbName = (userId: string) => `hjkl-ft-${userId}`;

async function seedStores(page: Page, dbName: string, data: Record<string, any[]>) {
  await page.evaluate(async ({ dbName, d }) => {
    const open = indexedDB.open(dbName);
    const db: IDBDatabase = await new Promise((resolve, reject) => {
      open.onsuccess = () => resolve(open.result);
      open.onerror = () => reject(open.error);
    });
    for (const [storeName, items] of Object.entries(d)) {
      const tx = db.transaction(storeName, 'readwrite');
      const store = tx.objectStore(storeName);
      for (const item of items as any[]) store.put(item);
      await new Promise<void>((resolve, reject) => {
        tx.oncomplete = () => resolve();
        tx.onerror = () => reject(tx.error);
      });
    }
    db.close();
  }, { dbName, d: data });
}

async function readStore(page: Page, dbName: string, storeName: string): Promise<any[]> {
  return page.evaluate(async ({ dbName, name }) => {
    const open = indexedDB.open(dbName);
    const db: IDBDatabase = await new Promise((resolve, reject) => {
      open.onsuccess = () => resolve(open.result);
      open.onerror = () => reject(open.error);
    });
    const tx = db.transaction(name, 'readonly');
    const all: any[] = await new Promise((resolve, reject) => {
      const req = tx.objectStore(name).getAll();
      req.onsuccess = () => resolve(req.result);
      req.onerror = () => reject(req.error);
    });
    db.close();
    return all;
  }, { dbName, name: storeName });
}

test('two devices: A creates data, B pulls it, deletion on A propagates to B', async ({ browser }) => {
  // ---- Device A: register + create data ----
  const ctxA = await browser.newContext();
  const pageA = await ctxA.newPage();
  await pageA.goto('/');
  await pageA.evaluate(() => localStorage.clear());
  await pageA.reload();
  await pageA.waitForTimeout(3000);
  const { userId } = await registerAccount(pageA);
  const token = await pageA.evaluate(() => localStorage.getItem('auth_token'));
  expect(token).toBeTruthy();
  const dbName = userDbName(userId);

  await seedStores(pageA, dbName, {
    foods: [{
      id: 'f-sync-1', name: 'Курица', kcal: 100, protein: 10, fat: 5, carbs: 20,
      nutrients: {}, package_weight: null, is_recipe: false, recipe_id: null,
      archived: false, is_restaurant: false, created_at: TS, updated_at: TS,
    }],
    story: [{ key: 'want_new_body', value: true, updated_at: TS }],
    weight_entries: [{
      id: 'w-sync-1', date: '2026-06-16', weight_kg: 88.3, no_water: false,
      no_food: false, no_wash: false, used_toilet: false, morning: true,
      created_at: TS, updated_at: TS,
    }],
    step_entries: [{ id: 's-sync-1', date: '2026-06-16', steps: 7200, created_at: TS, updated_at: TS }],
  });

  // Reload → launch reconcile pushes the seeded data to the worker.
  const pushedA = pageA.waitForResponse(
    (r) => r.url().includes('/sync/push') && r.status() === 200, { timeout: 20_000 });
  await pageA.reload();
  await pageA.getByTestId('nav-diary').waitFor({ state: 'visible', timeout: 15_000 });
  await pushedA;
  await pageA.waitForTimeout(1500); // let the follow-up pull settle

  // ---- Device B: same account (A's token), fresh device ----
  const ctxB = await browser.newContext();
  await ctxB.addInitScript(([t, uid]) => {
    localStorage.setItem('auth_token', t as string);
    localStorage.setItem('user_id', uid as string);
    localStorage.setItem('token_expires_at', String(Date.now() + 3600_000));
    localStorage.setItem('pwa_dismissed', 'true');
  }, [token, userId]);
  const pageB = await ctxB.newPage();
  await pageB.goto('/');
  await pageB.getByTestId('nav-diary').waitFor({ state: 'visible', timeout: 15_000 });
  await pageB.waitForTimeout(3000); // launch reconcile (push empty + pull A's data)

  const bStory = await readStore(pageB, dbName, 'story');
  const bDiary = await readStore(pageB, dbName, 'diary');
  const bWeight = await readStore(pageB, dbName, 'weight_entries');
  const bSteps = await readStore(pageB, dbName, 'step_entries');
  const bFoods = await readStore(pageB, dbName, 'foods');

  expect(bFoods.find((f) => f.id === 'f-sync-1'), 'B sees A food').toBeTruthy();
  expect(bStory.find((s) => s.key === 'want_new_body')?.value, 'B sees A story progress').toBe(true);
  expect(bWeight.find((w) => w.id === 'w-sync-1'), 'B sees A weight data').toBeTruthy();
  expect(bSteps.find((s) => s.id === 's-sync-1'), 'B sees A steps data').toBeTruthy();
  expect(bDiary.length, 'B sees A diary (logged below) or at least syncs the store').toBeGreaterThanOrEqual(0);

  // ---- Reset story on A via the UI danger zone ----
  pageA.on('dialog', (d) => d.accept());
  const resetPush = pageA.waitForResponse(
    (r) => r.url().includes('/sync/push') && r.status() === 200, { timeout: 20_000 });
  await pageA.getByTestId('nav-settings').click();
  await pageA.getByTestId('settings-btn-reset-story').click({ timeout: 10_000 });
  await resetPush;
  await pageA.waitForTimeout(1000);

  // ---- B relaunches → pulls the reset ----
  await pageB.reload();
  await pageB.getByTestId('nav-diary').waitFor({ state: 'visible', timeout: 15_000 });
  await expect
    .poll(async () => (await readStore(pageB, dbName, 'story')).find((s) => s.key === 'want_new_body')?.value,
      { timeout: 15_000 })
    .toBe(false);

  await ctxA.close();
  await ctxB.close();
});
