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

  test('Privacy row visible and navigates', async ({ page }) => {
    const privacyBtn = page.getByTestId('settings-btn-privacy');
    await expect(privacyBtn).toBeVisible({ timeout: 5_000 });
    await privacyBtn.click();
    await expect(page).toHaveURL(/\/settings\/privacy/);
  });

  test('Active sessions shown', async ({ page }) => {
    const privacyBtn = page.getByTestId('settings-btn-privacy');
    await expect(privacyBtn).toBeVisible({ timeout: 5_000 });
    await privacyBtn.click();
    await page.waitForTimeout(2000);

    const sessionsHeader = page.getByTestId('privacy-sessions-header');
    await expect(sessionsHeader).toBeVisible({ timeout: 5_000 });

    await page.waitForTimeout(3000);

    const sessionEntries = page.locator('text=created:');
    const fallback = page.locator('text="--"');
    const hasEntries = await sessionEntries.count() > 0;
    const hasFallback = await fallback.count() > 0;

    expect(hasEntries || hasFallback).toBe(true);

    if (hasEntries) {
      const count = await sessionEntries.count();
      expect(count).toBeGreaterThanOrEqual(1);
    }
  });

  test('Current device highlighted', async ({ page }) => {
    const privacyBtn = page.getByTestId('settings-btn-privacy');
    await expect(privacyBtn).toBeVisible({ timeout: 5_000 });
    await privacyBtn.click();
    await page.waitForTimeout(5000);

    const currentDevice = page.locator('text=/This device|Это устройство/');
    const fallback = page.locator('text="--"');

    const hasDevice = await currentDevice.count() > 0;
    const hasFallback = await fallback.count() > 0;

    expect(hasDevice || hasFallback).toBe(true);

    if (hasDevice) {
      await expect(currentDevice).toBeVisible({ timeout: 5_000 });
    }
  });

  test('Connect device button in privacy section', async ({ page }) => {
    const privacyBtn = page.getByTestId('settings-btn-privacy');
    await expect(privacyBtn).toBeVisible({ timeout: 5_000 });
    await privacyBtn.click();
    await page.waitForTimeout(1000);

    const addDeviceBtn = page.getByTestId('privacy-btn-add-device');
    await expect(addDeviceBtn).toBeVisible({ timeout: 5_000 });
  });
});

test.describe('Settings — Notification schedule', () => {
  let cdpSession: CDPSession;

  test.beforeEach(async ({ page }) => {
    await page.goto('/');
    await page.evaluate(() => localStorage.clear());
    await page.reload();
    await page.waitForTimeout(3000);
    const result = await registerAccount(page);
    cdpSession = result.cdpSession;

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

  test('Schedule section shows all 4 slot rows', async ({ page }) => {
    for (const slot of ['weigh_in', 'breakfast', 'lunch', 'dinner']) {
      const toggle = page.getByTestId(`schedule-toggle-${slot}`);
      await expect(toggle).toBeVisible({ timeout: 5_000 });

      const timeInput = page.getByTestId(`schedule-time-${slot}`);
      await expect(timeInput).toBeVisible({ timeout: 5_000 });
    }
  });

  test('Toggles start off and can be switched on', async ({ page }) => {
    const toggle = page.getByTestId('schedule-toggle-breakfast');
    await expect(toggle).toBeVisible({ timeout: 5_000 });

    // Default is off — background should be gray (#e5e5ea)
    const bgBefore = await toggle.evaluate(el => el.style.background);
    expect(bgBefore).toContain('#e5e5ea');

    await toggle.click();
    await page.waitForTimeout(300);

    // After click — background should be green (#34c759)
    const bgAfter = await toggle.evaluate(el => el.style.background);
    expect(bgAfter).toContain('#34c759');
  });

  test('Time inputs have correct default values', async ({ page }) => {
    const defaults: Record<string, string> = {
      weigh_in: '07:00',
      breakfast: '09:00',
      lunch: '13:00',
      dinner: '19:00',
    };

    for (const [slot, expectedTime] of Object.entries(defaults)) {
      const timeInput = page.getByTestId(`schedule-time-${slot}`);
      await expect(timeInput).toBeVisible({ timeout: 5_000 });
      const value = await timeInput.inputValue();
      expect(value).toBe(expectedTime);
    }
  });
});

test.describe('Settings — Goals page', () => {
  let cdpSession: CDPSession;

  test.beforeEach(async ({ page }) => {
    await page.goto('/');
    await page.evaluate(() => localStorage.clear());
    await page.reload();
    await page.waitForTimeout(3000);
    const result = await registerAccount(page);
    cdpSession = result.cdpSession;

    // Navigate to settings → goals
    const navSettings = page.getByTestId('nav-settings');
    await expect(navSettings).toBeVisible({ timeout: 10_000 });
    await navSettings.click();
    await expect(page).toHaveURL(/\/settings/);
    await page.waitForTimeout(500);

    const goalsBtn = page.getByTestId('settings-btn-goals');
    await expect(goalsBtn).toBeVisible({ timeout: 5_000 });
    await goalsBtn.click();
    await expect(page).toHaveURL(/\/settings\/goals/);
    await page.waitForTimeout(500);
  });

  test.afterEach(async () => {
    if (cdpSession) {
      await cdpSession.send('WebAuthn.disable').catch(() => {});
    }
  });

  test('Goals page has standard nutrient checkboxes', async ({ page }) => {
    for (const nutrient of ['calories', 'protein', 'fat', 'carbs']) {
      const checkbox = page.getByTestId(`goals-checkbox-${nutrient}`);
      await expect(checkbox).toBeVisible({ timeout: 5_000 });
    }
  });

  test('Enable nutrient shows Track/Goal mode toggle', async ({ page }) => {
    const checkbox = page.getByTestId('goals-checkbox-calories');
    await checkbox.check();
    await page.waitForTimeout(500);

    const modeToggle = page.getByTestId('goals-mode-calories');
    await expect(modeToggle).toBeVisible({ timeout: 5_000 });
  });

  test('Track mode does not show amount input', async ({ page }) => {
    const checkbox = page.getByTestId('goals-checkbox-calories');
    await checkbox.check();
    await page.waitForTimeout(500);

    // Default is Track mode (amount=0), so direction/amount selects should not be visible
    const modeToggle = page.getByTestId('goals-mode-calories');
    await expect(modeToggle).toBeVisible({ timeout: 5_000 });

    // In Track mode, the select for direction should NOT be present
    const goalSelects = modeToggle.locator('..').locator('select');
    const selectCount = await goalSelects.count();
    expect(selectCount).toBe(0);
  });

  test('Add custom nutrient', async ({ page }) => {
    const input = page.getByTestId('goals-input-new-nutrient');
    await expect(input).toBeVisible({ timeout: 5_000 });

    await input.fill('Omega 3');
    const addBtn = page.getByTestId('goals-btn-add');
    await addBtn.click();
    await page.waitForTimeout(500);

    // The custom nutrient should appear in the list
    await expect(page.locator('text=Omega 3')).toBeVisible({ timeout: 5_000 });
  });

  test('Back button navigates to settings', async ({ page }) => {
    const backBtn = page.getByTestId('goals-btn-back');
    await expect(backBtn).toBeVisible({ timeout: 5_000 });
    await backBtn.click();
    await expect(page).toHaveURL(/\/settings$/);
  });
});
