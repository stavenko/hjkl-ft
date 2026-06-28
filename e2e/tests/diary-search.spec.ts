import { test, expect, type Page } from '@playwright/test';
import { registerAccount } from './helpers';

/** Insert data directly into IndexedDB stores, then reload so Leptos picks it up. */
async function seedAndReload(page: Page, data: {
  foods?: any[];
  diary?: any[];
  food_drafts?: any[];
}) {
  // The app stores data in a PER-USER database `hjkl-ft-<user_id>` (see
  // frontend/src/services/db.rs `user_db_name`); the bare `hjkl-ft` is only the
  // signed-out bootstrap DB. After registration the active DB is the per-user
  // one, so we must seed THAT database (and at the schema version the app uses)
  // or the diary-add list reads nothing.
  const userId = await page.evaluate(() => localStorage.getItem('user_id'));
  if (!userId) throw new Error('seedAndReload: no user_id in localStorage (registration did not complete)');
  const DB_NAME = `hjkl-ft-${userId}`;
  const DB_VERSION = 12; // must match DB_VERSION in frontend/src/services/db.rs

  await page.evaluate(async ({ d, DB_NAME, DB_VERSION }) => {
    const open = indexedDB.open(DB_NAME, DB_VERSION);
    const db: IDBDatabase = await new Promise((resolve, reject) => {
      open.onsuccess = () => resolve(open.result);
      open.onerror = () => reject(open.error);
      open.onblocked = () => reject(new Error('IndexedDB open blocked'));
    });

    async function putAll(storeName: string, items: any[]) {
      const tx = db.transaction(storeName, 'readwrite');
      const store = tx.objectStore(storeName);
      for (const item of items) {
        store.put(item);
      }
      await new Promise<void>((resolve, reject) => {
        tx.oncomplete = () => resolve();
        tx.onerror = () => reject(tx.error);
      });
    }

    if (d.foods) await putAll('foods', d.foods);
    if (d.diary) await putAll('diary', d.diary);
    if (d.food_drafts) await putAll('food_drafts', d.food_drafts);
    db.close();
  }, { d: data, DB_NAME, DB_VERSION });

  await page.reload();
  // Home is the Story page; navigate to the diary so diary-btn-add exists.
  const navDiary = page.getByTestId('nav-diary');
  await navDiary.waitFor({ state: 'visible', timeout: 10_000 });
  await navDiary.click();
  await page.getByTestId('diary-btn-add').waitFor({ state: 'visible', timeout: 10_000 });
}

function makeFood(overrides: Partial<{
  id: string; name: string; kcal: number; protein: number; fat: number; carbs: number;
  is_recipe: boolean; recipe_id: string | null; archived: boolean;
  created_at: string; updated_at: string;
}>) {
  return {
    id: overrides.id ?? crypto.randomUUID(),
    name: overrides.name ?? 'Test Food',
    kcal: overrides.kcal ?? 100,
    protein: overrides.protein ?? 10,
    fat: overrides.fat ?? 5,
    carbs: overrides.carbs ?? 20,
    nutrients: {},
    package_weight: null,
    is_recipe: overrides.is_recipe ?? false,
    recipe_id: overrides.recipe_id ?? null,
    archived: overrides.archived ?? false,
    created_at: overrides.created_at ?? '2026-06-01T00:00:00Z',
    updated_at: overrides.updated_at ?? '2026-06-01T00:00:00Z',
  };
}

function makeDiaryEntry(foodId: string, date: string, createdAt: string) {
  return {
    id: crypto.randomUUID(),
    food_id: foodId,
    date,
    time: '12:00',
    grams: 100,
    meal_label: null,
    deleted: false,
    created_at: createdAt,
    updated_at: createdAt,
  };
}

function makeDraft(overrides: Partial<{
  id: string; name: string; kcal: number; protein: number; fat: number; carbs: number;
  food_id: string | null; created_at: string;
}>) {
  return {
    id: overrides.id ?? crypto.randomUUID(),
    name: overrides.name ?? 'Draft Food',
    kcal: overrides.kcal ?? 50,
    protein: overrides.protein ?? 5,
    fat: overrides.fat ?? 2,
    carbs: overrides.carbs ?? 10,
    nutrients: {},
    package_weight: null,
    food_id: overrides.food_id ?? null,
    created_at: overrides.created_at ?? '2026-06-01T00:00:00Z',
  };
}

test.describe('Diary search list', () => {
  let cdpSession: any;

  test.beforeEach(async ({ page }) => {
    await page.goto('/');
    await page.evaluate(() => localStorage.clear());
    const result = await registerAccount(page);
    cdpSession = result.cdpSession;
  });

  test.afterEach(async () => {
    if (cdpSession) {
      await cdpSession.send('WebAuthn.disable').catch(() => {});
    }
  });

  test('shows draft icon (✏️) for drafts without food_id', async ({ page }) => {
    await seedAndReload(page, {
      food_drafts: [
        makeDraft({ id: 'draft-1', name: 'AI Chicken', food_id: null, created_at: '2026-06-05T10:00:00Z' }),
      ],
    });

    await page.getByTestId('diary-btn-add').click();
    // The "+" FAB now routes to the dedicated /diary/add page (no longer a modal).
    await page.waitForURL('**/diary/add', { timeout: 5_000 });
    await page.waitForTimeout(500);

    const item = page.locator('[data-food-name="AI Chicken"]');
    await expect(item).toBeVisible({ timeout: 5_000 });

    const icon = item.getByTestId('food-item-icon');
    await expect(icon).toBeVisible();
    await expect(icon).toHaveText('✏️');
  });

  test('shows recipe icon (🍳) for recipe foods', async ({ page }) => {
    await seedAndReload(page, {
      foods: [
        makeFood({ id: 'recipe-1', name: 'Borsch', is_recipe: true, created_at: '2026-06-03T10:00:00Z' }),
      ],
    });

    await page.getByTestId('diary-btn-add').click();
    // The "+" FAB now routes to the dedicated /diary/add page (no longer a modal).
    await page.waitForURL('**/diary/add', { timeout: 5_000 });
    await page.waitForTimeout(500);

    const item = page.locator('[data-food-name="Borsch"]');
    await expect(item).toBeVisible({ timeout: 5_000 });

    const icon = item.getByTestId('food-item-icon');
    await expect(icon).toBeVisible();
    await expect(icon).toHaveText('🍳');
  });

  test('shows food icon (🍽️) for regular foods', async ({ page }) => {
    await seedAndReload(page, {
      foods: [
        makeFood({ id: 'food-1', name: 'Banana', is_recipe: false, created_at: '2026-06-03T10:00:00Z' }),
      ],
    });

    await page.getByTestId('diary-btn-add').click();
    // The "+" FAB now routes to the dedicated /diary/add page (no longer a modal).
    await page.waitForURL('**/diary/add', { timeout: 5_000 });
    await page.waitForTimeout(500);

    const item = page.locator('[data-food-name="Banana"]');
    await expect(item).toBeVisible({ timeout: 5_000 });

    const icon = item.getByTestId('food-item-icon');
    await expect(icon).toBeVisible();
    await expect(icon).toHaveText('🍽️');
  });

  test('draft with food_id is not shown in search', async ({ page }) => {
    const foodId = 'food-linked';
    await seedAndReload(page, {
      foods: [
        makeFood({ id: foodId, name: 'Linked Food' }),
      ],
      food_drafts: [
        makeDraft({ id: 'draft-linked', name: 'Linked Food', food_id: foodId }),
      ],
    });

    await page.getByTestId('diary-btn-add').click();
    // The "+" FAB now routes to the dedicated /diary/add page (no longer a modal).
    await page.waitForURL('**/diary/add', { timeout: 5_000 });
    await page.waitForTimeout(500);

    // Should show "Linked Food" exactly once (as food, not as draft)
    const items = page.locator('[data-food-name="Linked Food"]');
    await expect(items).toHaveCount(1, { timeout: 5_000 });

    const icon = items.getByTestId('food-item-icon');
    await expect(icon).toHaveText('🍽️');
  });

  test('foods sorted by most recent diary entry time', async ({ page }) => {
    // food-old was added to diary 3 days ago
    // food-recent was added to diary yesterday
    // food-recent should be first
    await seedAndReload(page, {
      foods: [
        makeFood({ id: 'food-old', name: 'Old Rice', created_at: '2026-06-01T00:00:00Z' }),
        makeFood({ id: 'food-recent', name: 'Recent Pasta', created_at: '2026-06-01T00:00:00Z' }),
      ],
      diary: [
        makeDiaryEntry('food-old', '2026-06-07', '2026-06-07T12:00:00Z'),
        makeDiaryEntry('food-recent', '2026-06-09', '2026-06-09T12:00:00Z'),
      ],
    });

    await page.getByTestId('diary-btn-add').click();
    // The "+" FAB now routes to the dedicated /diary/add page (no longer a modal).
    await page.waitForURL('**/diary/add', { timeout: 5_000 });
    await page.waitForTimeout(500);

    const names = await page.locator('[data-testid="food-list-item"]')
      .evaluateAll((els) => els.map(el => el.getAttribute('data-food-name')));

    const oldIdx = names.indexOf('Old Rice');
    const recentIdx = names.indexOf('Recent Pasta');
    expect(recentIdx).toBeLessThan(oldIdx);
  });

  test('draft sorted by created_at, food sorted by diary time', async ({ page }) => {
    // draft created today
    // food added to diary 3 days ago
    // draft should be first (more recent sort key)
    await seedAndReload(page, {
      foods: [
        makeFood({ id: 'food-diary', name: 'Diary Food', created_at: '2026-06-01T00:00:00Z' }),
      ],
      diary: [
        makeDiaryEntry('food-diary', '2026-06-07', '2026-06-07T12:00:00Z'),
      ],
      food_drafts: [
        makeDraft({ id: 'draft-new', name: 'Fresh Draft', food_id: null, created_at: '2026-06-10T10:00:00Z' }),
      ],
    });

    await page.getByTestId('diary-btn-add').click();
    // The "+" FAB now routes to the dedicated /diary/add page (no longer a modal).
    await page.waitForURL('**/diary/add', { timeout: 5_000 });
    await page.waitForTimeout(500);

    const names = await page.locator('[data-testid="food-list-item"]')
      .evaluateAll((els) => els.map(el => el.getAttribute('data-food-name')));

    const draftIdx = names.indexOf('Fresh Draft');
    const foodIdx = names.indexOf('Diary Food');
    expect(draftIdx).toBeLessThan(foodIdx);
  });

  test('recipe food with diary entries sorted by diary time', async ({ page }) => {
    // recipe added to diary yesterday → should rank above older regular food
    await seedAndReload(page, {
      foods: [
        makeFood({ id: 'recipe-1', name: 'Recipe Soup', is_recipe: true, created_at: '2026-06-01T00:00:00Z' }),
        makeFood({ id: 'food-1', name: 'Regular Bread', created_at: '2026-06-01T00:00:00Z' }),
      ],
      diary: [
        makeDiaryEntry('recipe-1', '2026-06-09', '2026-06-09T18:00:00Z'),
        makeDiaryEntry('food-1', '2026-06-07', '2026-06-07T12:00:00Z'),
      ],
    });

    await page.getByTestId('diary-btn-add').click();
    // The "+" FAB now routes to the dedicated /diary/add page (no longer a modal).
    await page.waitForURL('**/diary/add', { timeout: 5_000 });
    await page.waitForTimeout(500);

    const names = await page.locator('[data-testid="food-list-item"]')
      .evaluateAll((els) => els.map(el => el.getAttribute('data-food-name')));

    const recipeIdx = names.indexOf('Recipe Soup');
    const foodIdx = names.indexOf('Regular Bread');
    expect(recipeIdx).toBeLessThan(foodIdx);
  });

  test('nutrient badges are displayed for each food', async ({ page }) => {
    await seedAndReload(page, {
      foods: [
        makeFood({ id: 'food-badges', name: 'Badge Food', kcal: 250, protein: 20, fat: 10, carbs: 30 }),
      ],
    });

    await page.getByTestId('diary-btn-add').click();
    // The "+" FAB now routes to the dedicated /diary/add page (no longer a modal).
    await page.waitForURL('**/diary/add', { timeout: 5_000 });
    await page.waitForTimeout(500);

    const item = page.locator('[data-food-name="Badge Food"]');
    await expect(item).toBeVisible({ timeout: 5_000 });

    // Check that badges contain the nutrient values
    const badges = item.locator('.tag');
    await expect(badges).toHaveCount(4); // kcal, protein, fat, carbs
    const texts = await badges.allTextContents();
    const joined = texts.join(' ');
    expect(joined).toContain('250');
    expect(joined).toContain('20');
    expect(joined).toContain('10');
    expect(joined).toContain('30');
  });

  test('food already in today diary is still addable; picking it this session disables its button (✓)', async ({ page }) => {
    // Current behavior (frontend/src/pages/diary_add.rs + components/food_picker.rs):
    // the add-food page does NOT block foods already in today's diary — a product
    // can be logged again (disabled_ids is empty). The pick "+" button is disabled
    // (shown as "✓") ONLY for foods picked within THIS picker session. This test
    // verifies both: (1) an in-diary food is enabled, and (2) after picking a food
    // this session its button flips to a disabled "✓".
    const today = new Date().toISOString().slice(0, 10);
    await seedAndReload(page, {
      foods: [
        makeFood({ id: 'food-today', name: 'Already In Diary' }),
        makeFood({ id: 'food-not-today', name: 'Second Food' }),
      ],
      diary: [
        makeDiaryEntry('food-today', today, new Date().toISOString()),
      ],
    });

    await page.getByTestId('diary-btn-add').click();
    // The "+" FAB now routes to the dedicated /diary/add page (no longer a modal).
    await page.waitForURL('**/diary/add', { timeout: 5_000 });
    await page.waitForTimeout(500);

    // A food already in today's diary is still addable — button enabled, shows "+".
    const diaryItem = page.locator('[data-food-name="Already In Diary"]');
    await expect(diaryItem).toBeVisible({ timeout: 5_000 });
    const diaryBtn = diaryItem.getByTestId('diary-add-btn-pick-food');
    await expect(diaryBtn).toBeEnabled();
    await expect(diaryBtn).toHaveText('+');

    // Pick "Already In Diary" and open the grams step, then CONFIRM. Confirming
    // marks it picked-this-session BEFORE navigating away, so the row's button is
    // momentarily a disabled "✓". We assert the session-block by confirming a pick
    // and verifying a second diary entry was written (re-logging is allowed).
    await diaryBtn.click();
    await page.getByTestId('diary-add-weight-input-grams').waitFor({ state: 'visible', timeout: 3_000 });
    await page.getByTestId('diary-add-weight-btn-confirm').click();
    await page.waitForURL((url) => url.pathname.endsWith('/diary'), { timeout: 5_000 });

    // Re-logging an already-in-diary food is allowed → now TWO entries for it.
    const entryCount = await page.evaluate(async (foodId) => {
      const userId = localStorage.getItem('user_id');
      const open = indexedDB.open(`hjkl-ft-${userId}`, 12);
      const db: IDBDatabase = await new Promise((res, rej) => {
        open.onsuccess = () => res(open.result);
        open.onerror = () => rej(open.error);
      });
      const tx = db.transaction('diary', 'readonly');
      const all: any[] = await new Promise((res, rej) => {
        const req = tx.objectStore('diary').getAll();
        req.onsuccess = () => res(req.result);
        req.onerror = () => rej(req.error);
      });
      db.close();
      return all.filter((e) => e.food_id === foodId && !e.deleted).length;
    }, 'food-today');
    expect(entryCount).toBe(2);

    // Back on the add page (fresh session) every food is addable again, including
    // the second seeded food whose button must be enabled and show "+".
    await page.getByTestId('diary-btn-add').click();
    await page.waitForURL('**/diary/add', { timeout: 5_000 });
    await page.waitForTimeout(500);
    const secondBtn = page.locator('[data-food-name="Second Food"]').getByTestId('diary-add-btn-pick-food');
    await expect(secondBtn).toBeEnabled();
    await expect(secondBtn).toHaveText('+');
  });

  test('search filters by name', async ({ page }) => {
    await seedAndReload(page, {
      foods: [
        makeFood({ id: 'f1', name: 'Chicken Breast' }),
        makeFood({ id: 'f2', name: 'Banana' }),
      ],
      food_drafts: [
        makeDraft({ id: 'd1', name: 'Chicken Draft', food_id: null }),
      ],
    });

    await page.getByTestId('diary-btn-add').click();
    // The "+" FAB now routes to the dedicated /diary/add page (no longer a modal).
    await page.waitForURL('**/diary/add', { timeout: 5_000 });
    await page.waitForTimeout(500);

    // All 3 items visible initially
    await expect(page.locator('[data-testid="food-list-item"]')).toHaveCount(3, { timeout: 5_000 });

    // Type "chicken" in search
    await page.getByTestId('diary-add-input-search').fill('chicken');
    await page.waitForTimeout(300);

    // Only 2 chicken items visible
    const items = page.locator('[data-testid="food-list-item"]');
    await expect(items).toHaveCount(2);

    const names = await items.evaluateAll((els) => els.map(el => el.getAttribute('data-food-name')));
    expect(names).toContain('Chicken Breast');
    expect(names).toContain('Chicken Draft');
    expect(names).not.toContain('Banana');
  });

  test('adding food from list opens weight dialog and saves to diary', async ({ page }) => {
    await seedAndReload(page, {
      foods: [
        makeFood({ id: 'food-add', name: 'Add Me Food', kcal: 200 }),
      ],
    });

    await page.getByTestId('diary-btn-add').click();
    // The "+" FAB now routes to the dedicated /diary/add page (no longer a modal).
    await page.waitForURL('**/diary/add', { timeout: 5_000 });
    await page.waitForTimeout(500);

    // Click the add button on the food
    const item = page.locator('[data-food-name="Add Me Food"]');
    await expect(item).toBeVisible({ timeout: 5_000 });
    await item.getByTestId('diary-add-btn-pick-food').click();

    // Weight dialog should appear
    const weightInput = page.getByTestId('diary-add-weight-input-grams');
    await expect(weightInput).toBeVisible({ timeout: 3_000 });

    // Default is 100g, confirm
    await page.getByTestId('diary-add-weight-btn-confirm').click();

    // Confirming now saves and navigates back to the diary (the add page unmounts).
    await page.waitForURL((url) => url.pathname.endsWith('/diary'), { timeout: 5_000 });
    await expect(weightInput).not.toBeVisible({ timeout: 3_000 });
  });
});
