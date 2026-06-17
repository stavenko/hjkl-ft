import { test, expect, type Page } from '@playwright/test';
import { registerAccount } from './helpers';

/**
 * Deletion sync via an explicit tombstone log: deleting an entity creates a
 * DeletionRecord that is pushed, stored on the server (which never hard-deletes
 * the entity), and APPLIED on every device on every pull. A deletion must
 * propagate to other devices and must NOT resurrect on subsequent syncs even
 * though the server keeps re-serving the (un-deleted) entity.
 */

const TS = '2026-06-17T10:00:00Z';

async function seed(page: Page, data: Record<string, any[]>) {
  await page.evaluate(async (d) => {
    const open = indexedDB.open('hjkl-ft');
    const db: IDBDatabase = await new Promise((res, rej) => { open.onsuccess = () => res(open.result); open.onerror = () => rej(open.error); });
    for (const [store, items] of Object.entries(d)) {
      const tx = db.transaction(store, 'readwrite');
      for (const it of items as any[]) tx.objectStore(store).put(it);
      await new Promise<void>((res, rej) => { tx.oncomplete = () => res(); tx.onerror = () => rej(tx.error); });
    }
    db.close();
  }, data);
}

async function diaryIds(page: Page): Promise<string[]> {
  return page.evaluate(async () => {
    const open = indexedDB.open('hjkl-ft');
    const db: IDBDatabase = await new Promise((res, rej) => { open.onsuccess = () => res(open.result); open.onerror = () => rej(open.error); });
    const tx = db.transaction('diary', 'readonly');
    const all: any[] = await new Promise((res, rej) => { const r = tx.objectStore('diary').getAll(); r.onsuccess = () => res(r.result); r.onerror = () => rej(r.error); });
    db.close();
    return all.map((e) => e.id);
  });
}

const today = () => new Date().toISOString().slice(0, 10);
const pushed = (p: Page) => p.waitForResponse((r) => r.url().includes('/sync/push') && r.status() === 200, { timeout: 20_000 });

test('a deletion propagates to another device and never resurrects', async ({ browser }) => {
  const FOOD = 'f-del-1';
  const ENTRY = 'd-del-1';

  // ---- Device A: register, seed a food + today's diary entry, push ----
  const ctxA = await browser.newContext();
  const pageA = await ctxA.newPage();
  await pageA.goto('/');
  await pageA.evaluate(() => localStorage.clear());
  await pageA.reload();
  await pageA.waitForTimeout(3000);
  const { userId } = await registerAccount(pageA);
  const token = await pageA.evaluate(() => localStorage.getItem('auth_token'));

  await seed(pageA, {
    foods: [{ id: FOOD, name: 'Курица', kcal: 100, protein: 10, fat: 5, carbs: 20, nutrients: {}, package_weight: null, is_recipe: false, recipe_id: null, archived: false, is_restaurant: false, created_at: TS, updated_at: TS }],
    diary: [{ id: ENTRY, food_id: FOOD, date: today(), time: null, grams: 100, waste_grams: 0, meal_label: null, deleted: false, created_at: TS, updated_at: TS }],
  });
  let p = pushed(pageA);
  await pageA.reload();
  await pageA.getByTestId('nav-diary').waitFor({ state: 'visible', timeout: 15_000 });
  await p;
  await pageA.waitForTimeout(1500);

  // ---- Device B: same account, pulls the entry ----
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
  await pageB.waitForTimeout(3000);
  expect(await diaryIds(pageB), 'B should have the entry before deletion').toContain(ENTRY);

  // ---- Device A: delete the entry (tombstone) + push ----
  await seed(pageA, { deletions: [{ id: 'del-1', kind: 'diary', target_id: ENTRY, created_at: '2026-06-17T11:00:00Z' }] });
  p = pushed(pageA);
  await pageA.reload(); // launch sync pushes the deletion, pull applies it locally
  await pageA.getByTestId('nav-diary').waitFor({ state: 'visible', timeout: 15_000 });
  await p;
  await expect.poll(() => diaryIds(pageA), { timeout: 10_000 }).not.toContain(ENTRY);

  // ---- Device B: relaunch → deletion applied ----
  await pageB.reload();
  await pageB.getByTestId('nav-diary').waitFor({ state: 'visible', timeout: 15_000 });
  await expect.poll(() => diaryIds(pageB), { timeout: 15_000 }).not.toContain(ENTRY);

  // ---- Device B: sync again → still gone (server still holds the entity) ----
  await pageB.reload();
  await pageB.getByTestId('nav-diary').waitFor({ state: 'visible', timeout: 15_000 });
  await pageB.waitForTimeout(3000);
  expect(await diaryIds(pageB), 'deleted entry must not resurrect').not.toContain(ENTRY);

  await ctxA.close();
  await ctxB.close();
});
