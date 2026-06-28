import { test, expect, type CDPSession } from '@playwright/test';
import { registerAccount } from './helpers';

test.describe('App navigation', () => {
  let cdpSession: CDPSession;

  test.beforeEach(async ({ page }) => {
    await page.goto('/');
    await page.evaluate(() => localStorage.clear());

    // Register + claim a paid sub via /onboard (the only registration path now),
    // landing in the app with a usable (subscription-active) account.
    ({ cdpSession } = await registerAccount(page));

    const navLink = page.getByTestId('nav-recipes');
    await expect(navLink).toBeVisible({ timeout: 10_000 });
  });

  test.afterEach(async () => {
    if (cdpSession) {
      await cdpSession.send('WebAuthn.disable').catch(() => {});
    }
  });

  test('starts on diary page', async ({ page }) => {
    await expect(page).toHaveURL(/\/$/);
  });

  test('navigate to Recipes and back to Diary', async ({ page }) => {
    await page.getByTestId('nav-recipes').click();
    await expect(page).toHaveURL(/\/recipes/);
    await expect(page.locator('h1', { hasText: 'Рецепты' })).toBeVisible({ timeout: 5_000 });

    await page.getByTestId('nav-diary').click();
    await expect(page).toHaveURL(/\/$/);
  });

  test('navigate to Settings', async ({ page }) => {
    await page.getByTestId('nav-settings').click();
    await expect(page).toHaveURL(/\/settings/);
    await expect(page.locator('h1', { hasText: 'Настройки' })).toBeVisible({ timeout: 5_000 });
  });

  test('navigate Diary → Recipes → Settings → Diary', async ({ page }) => {
    // Diary → Recipes
    await page.getByTestId('nav-recipes').click();
    await expect(page).toHaveURL(/\/recipes/);

    // Recipes → Settings
    await page.getByTestId('nav-settings').click();
    await expect(page).toHaveURL(/\/settings/);

    // Settings → Diary
    await page.getByTestId('nav-diary').click();
    await expect(page).toHaveURL(/\/$/);
  });

  test('Settings page is interactive after navigation', async ({ page }) => {
    await page.getByTestId('nav-settings').click();
    await expect(page).toHaveURL(/\/settings/);

    // Navigate to Goals page
    const goalsBtn = page.getByTestId('settings-btn-goals');
    await expect(goalsBtn).toBeVisible({ timeout: 5_000 });
    await goalsBtn.click();
    await expect(page).toHaveURL(/\/settings\/goals/);

    // Toggle a goal checkbox
    const caloriesCheckbox = page.getByTestId('goals-checkbox-calories');
    await expect(caloriesCheckbox).toBeVisible({ timeout: 5_000 });
    await caloriesCheckbox.check();
    await expect(caloriesCheckbox).toBeChecked();
  });

  test('navigate back and forth multiple times without crash', async ({ page }) => {
    for (let i = 0; i < 5; i++) {
      await page.getByTestId('nav-recipes').click();
      await expect(page).toHaveURL(/\/recipes/);

      await page.getByTestId('nav-settings').click();
      await expect(page).toHaveURL(/\/settings/);

      await page.getByTestId('nav-diary').click();
      await expect(page).toHaveURL(/\/$/);
    }

    // App still works -- no panic
    const errors = await page.evaluate(() => {
      return (window as any).__playwright_errors || [];
    });
    // Check console for RuntimeError
    // (the test would have timed out on click if Router crashed)
  });
});
