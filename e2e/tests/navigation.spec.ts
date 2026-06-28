import { test, expect, type CDPSession } from '@playwright/test';
import { registerAccount } from './helpers';

// The diary lives at `/diary` now ("/" is the Story home). `nav-diary`
// navigates there; the diary page is recognizable by its date-nav button.
const DIARY_URL = /\/diary$/;

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

  test('starts on Story home, can reach Diary', async ({ page }) => {
    // After onboarding the app lands on the Story home route "/".
    await expect(page).toHaveURL(/\/$/);

    // Diary is its own route now; reach it via the bottom nav.
    await page.getByTestId('nav-diary').click();
    await expect(page).toHaveURL(DIARY_URL);
    await expect(page.getByTestId('diary-btn-date')).toBeVisible({ timeout: 5_000 });
  });

  test('navigate to Recipes and back to Diary', async ({ page }) => {
    await page.getByTestId('nav-recipes').click();
    await expect(page).toHaveURL(/\/recipes/);
    await expect(page.locator('h1', { hasText: 'Рецепты' })).toBeVisible({ timeout: 5_000 });

    await page.getByTestId('nav-diary').click();
    await expect(page).toHaveURL(DIARY_URL);
    await expect(page.getByTestId('diary-btn-date')).toBeVisible({ timeout: 5_000 });
  });

  test('navigate to Settings', async ({ page }) => {
    await page.getByTestId('nav-settings').click();
    await expect(page).toHaveURL(/\/settings/);
    await expect(page.locator('h1', { hasText: 'Настройки' })).toBeVisible({ timeout: 5_000 });
  });

  test('navigate Diary → Recipes → Settings → Diary', async ({ page }) => {
    // Diary
    await page.getByTestId('nav-diary').click();
    await expect(page).toHaveURL(DIARY_URL);

    // Diary → Recipes
    await page.getByTestId('nav-recipes').click();
    await expect(page).toHaveURL(/\/recipes/);

    // Recipes → Settings
    await page.getByTestId('nav-settings').click();
    await expect(page).toHaveURL(/\/settings/);

    // Settings → Diary
    await page.getByTestId('nav-diary').click();
    await expect(page).toHaveURL(DIARY_URL);
  });

  test('Goals page mode toggle is interactive', async ({ page }) => {
    await page.getByTestId('nav-settings').click();
    await expect(page).toHaveURL(/\/settings/);

    // The Goals row is hidden in the current build (SHOW_GOALS=false), but the
    // route still exists — navigate to it directly.
    await page.goto('/settings/goals');
    await expect(page).toHaveURL(/\/settings\/goals/);

    // Calories is a standard nutrient with a Track/Goal segmented toggle.
    const caloriesMode = page.getByTestId('goals-mode-calories');
    await expect(caloriesMode).toBeVisible({ timeout: 5_000 });

    // In Track mode the per-goal direction select is hidden. Switch to Goal mode
    // ("Цель") and assert the direction select ("не менее"/"не более") appears.
    await caloriesMode.getByRole('button', { name: 'Цель' }).click();
    const directionOption = page.locator('option', { hasText: 'не менее' });
    await expect(directionOption.first()).toBeAttached({ timeout: 5_000 });
  });

  test('navigate back and forth multiple times without crash', async ({ page }) => {
    for (let i = 0; i < 5; i++) {
      await page.getByTestId('nav-recipes').click();
      await expect(page).toHaveURL(/\/recipes/);

      await page.getByTestId('nav-settings').click();
      await expect(page).toHaveURL(/\/settings/);

      await page.getByTestId('nav-diary').click();
      await expect(page).toHaveURL(DIARY_URL);
    }

    // App still works -- no panic (the test would have timed out on a click
    // above if the Router had crashed).
    await expect(page.getByTestId('diary-btn-date')).toBeVisible({ timeout: 5_000 });
  });
});
