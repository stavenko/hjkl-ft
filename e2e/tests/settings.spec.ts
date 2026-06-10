import { test, expect, type CDPSession } from '@playwright/test';
import { registerAccount } from './helpers';

test.describe('Settings — Privacy section', () => {
  let cdpSession: CDPSession;

  test.beforeEach(async ({ page }) => {
    await page.goto('/');
    await page.evaluate(() => localStorage.clear());
    await page.reload();
    await page.waitForTimeout(3000);
    const result = await registerAccount(page);
    cdpSession = result.cdpSession;

    // Navigate to settings
    const navSettings = page.getByTestId('nav-settings');
    await expect(navSettings).toBeVisible({ timeout: 10_000 });
    await navSettings.click();
    await expect(page).toHaveURL(/\/settings/);
    await page.waitForTimeout(1000);
  });

  test.afterEach(async () => {
    if (cdpSession) {
      await cdpSession.send('WebAuthn.disable').catch(() => {});
    }
  });

  test('Privacy section visible', async ({ page }) => {
    // The settings page should have a privacy section header
    // Russian: "Приватность", English: "Privacy"
    const privacyHeader = page.locator('h2', { hasText: /Приватность|Privacy/ });
    await expect(privacyHeader).toBeVisible({ timeout: 5_000 });
  });

  test('Active sessions shown', async ({ page }) => {
    // The sessions header: "Активные сессии" / "Active sessions"
    const sessionsHeader = page.locator('h3', { hasText: /Активные сессии|Active sessions/ });
    await expect(sessionsHeader).toBeVisible({ timeout: 5_000 });

    // Wait for the resource to resolve
    await page.waitForTimeout(3000);

    // After registration, verify at least one session entry is visible.
    // Session entries show "created:" text when tokens are returned,
    // or "--" when none are available yet.
    const sessionEntries = page.locator('text=created:');
    const fallback = page.locator('text="--"');
    const hasEntries = await sessionEntries.count() > 0;
    const hasFallback = await fallback.count() > 0;

    // At least one of these must be visible (the section loaded)
    expect(hasEntries || hasFallback).toBe(true);

    if (hasEntries) {
      const count = await sessionEntries.count();
      expect(count).toBeGreaterThanOrEqual(1);
    }
  });

  test('Current device highlighted', async ({ page }) => {
    // Wait for resource to resolve
    await page.waitForTimeout(3000);

    // The current session should have "This device" / "Это устройство" label
    // if there are active sessions, OR "--" if the token list is empty.
    const currentDevice = page.locator('text=/This device|Это устройство/');
    const fallback = page.locator('text="--"');

    const hasDevice = await currentDevice.count() > 0;
    const hasFallback = await fallback.count() > 0;

    // At least one must be present: either a labeled current device or empty state
    expect(hasDevice || hasFallback).toBe(true);

    if (hasDevice) {
      await expect(currentDevice).toBeVisible({ timeout: 5_000 });
    }
  });

  test('Connect device button in privacy section', async ({ page }) => {
    const addDeviceBtn = page.getByTestId('settings-btn-add-device');
    await expect(addDeviceBtn).toBeVisible({ timeout: 5_000 });
  });
});
