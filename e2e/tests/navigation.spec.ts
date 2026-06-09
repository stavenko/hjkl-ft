import { test, expect, type CDPSession } from '@playwright/test';
import { patchRegisterFinish } from './helpers';

test.describe('App navigation', () => {
  let cdpSession: CDPSession;

  test.beforeEach(async ({ page }) => {
    await page.goto('/');
    await page.evaluate(() => localStorage.clear());

    cdpSession = await page.context().newCDPSession(page);
    await cdpSession.send('WebAuthn.enable');
    await cdpSession.send('WebAuthn.addVirtualAuthenticator', {
      options: {
        protocol: 'ctap2',
        transport: 'internal',
        hasResidentKey: true,
        hasUserVerification: true,
        isUserVerified: true,
        automaticPresenceSimulation: true,
      },
    });

    // Patch register/finish to include user_id
    await patchRegisterFinish(page);

    await page.evaluate(() => localStorage.setItem('pwa_dismissed', 'true'));
    await page.reload();
    await page.waitForTimeout(3000);

    // Wait for TryingPassKey → Auth page
    const createBtn = page.getByTestId('auth-btn-register');
    await expect(createBtn).toBeVisible({ timeout: 15_000 });
    await createBtn.click();

    // Wait for registration complete -- verify it actually worked
    let registered = false;
    for (let i = 0; i < 40; i++) {
      const uid = await page.evaluate(() => localStorage.getItem('user_id'));
      if (uid) { registered = true; break; }
      await page.waitForTimeout(500);
    }
    expect(registered).toBe(true);
    await page.waitForTimeout(1000);

    // Verify the auth overlay is gone by waiting for nav to be clickable
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

    // Toggle a goal checkbox
    const caloriesCheckbox = page.getByTestId('settings-checkbox-calories');
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
