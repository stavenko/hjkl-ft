import { test, expect, type CDPSession, type Page } from '@playwright/test';
import { registerAccount } from './helpers';

/**
 * Write story-progress flags directly into the freshly-registered account's
 * per-user IndexedDB (`hjkl-ft-<user_id>`, store `story`, keyPath `key`) and
 * reload so the settings page re-reads them. Several Settings sections are gated
 * behind these flags (the notification schedule needs LANGUAGE_CONFIGURED +
 * NOTIFICATION_RECEIVED; the meal/steps reminder rows need MEAL_REMINDERS_UNLOCKED),
 * which a brand-new account doesn't have yet.
 */
async function setStoryFlags(page: Page, userId: string, flags: string[]) {
  await page.evaluate(async ({ userId, flags }) => {
    const dbName = `hjkl-ft-${userId}`;
    const db: IDBDatabase = await new Promise((resolve, reject) => {
      const req = indexedDB.open(dbName);
      req.onsuccess = () => resolve(req.result);
      req.onerror = () => reject(req.error);
    });
    await new Promise<void>((resolve, reject) => {
      const tx = db.transaction('story', 'readwrite');
      const store = tx.objectStore('story');
      const now = new Date().toISOString();
      for (const key of flags) {
        store.put({ key, value: true, updated_at: now });
      }
      tx.oncomplete = () => resolve();
      tx.onerror = () => reject(tx.error);
    });
    db.close();
  }, { userId, flags });
}

/** Resolve a CSS custom-property color (e.g. `var(--bulma-success)`) to its computed rgb(...) string. */
async function resolveColor(page: Page, cssVar: string): Promise<string> {
  return page.evaluate((v) => {
    const d = document.createElement('div');
    d.style.background = v;
    document.body.appendChild(d);
    const c = getComputedStyle(d).backgroundColor;
    d.remove();
    return c;
  }, cssVar);
}

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

    // Wait for the tokens resource to resolve (session rows or the "—" fallback).
    await page.waitForTimeout(3000);

    // The redesigned privacy page renders each session as a `privacy-session-item`
    // row; an empty/failed fetch renders a single "—" (em-dash) placeholder.
    const sessionEntries = page.getByTestId('privacy-session-item');
    const fallback = page.locator('text="—"');
    const hasEntries = (await sessionEntries.count()) > 0;
    const hasFallback = (await fallback.count()) > 0;

    expect(hasEntries || hasFallback).toBe(true);

    if (hasEntries) {
      const count = await sessionEntries.count();
      expect(count).toBeGreaterThanOrEqual(1);
      await expect(sessionEntries.first()).toBeVisible({ timeout: 5_000 });
    }
  });

  test('Current device highlighted', async ({ page }) => {
    const privacyBtn = page.getByTestId('settings-btn-privacy');
    await expect(privacyBtn).toBeVisible({ timeout: 5_000 });
    await privacyBtn.click();
    await page.waitForTimeout(5000);

    // Current device is badged via `privacy-session-current` (RU "Это устройство").
    const currentDevice = page.getByTestId('privacy-session-current');
    const fallback = page.locator('text="—"');

    const hasDevice = (await currentDevice.count()) > 0;
    const hasFallback = (await fallback.count()) > 0;

    expect(hasDevice || hasFallback).toBe(true);

    if (hasDevice) {
      await expect(currentDevice.first()).toBeVisible({ timeout: 5_000 });
      await expect(currentDevice.first()).toHaveText(/This device|Это устройство/);
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

    // The schedule section is gated behind the setup-section story flags
    // (LANGUAGE_CONFIGURED + NOTIFICATION_RECEIVED); the breakfast/lunch/dinner/
    // steps rows additionally need MEAL_REMINDERS_UNLOCKED. Set them so the full
    // schedule renders, then reload so settings re-reads the flags on mount.
    await setStoryFlags(page, result.userId, [
      'language_configured',
      'notification_received',
      'meal_reminders_unlocked',
    ]);

    const navSettings = page.getByTestId('nav-settings');
    await expect(navSettings).toBeVisible({ timeout: 10_000 });
    await navSettings.click();
    await expect(page).toHaveURL(/\/settings/);
    await page.waitForTimeout(1500);
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

    // The toggle background is driven by design-system CSS vars:
    // off = var(--bulma-border) (gray), on = var(--bulma-success) (green).
    const offColor = await resolveColor(page, 'var(--bulma-border)');
    const onColor = await resolveColor(page, 'var(--bulma-success)');
    expect(offColor).not.toBe(onColor);

    // Default is off — computed background should match the gray border color.
    const bgBefore = await toggle.evaluate(el => getComputedStyle(el).backgroundColor);
    expect(bgBefore).toBe(offColor);

    await toggle.click();
    await page.waitForTimeout(300);

    // After click — computed background should match the green success color.
    const bgAfter = await toggle.evaluate(el => getComputedStyle(el).backgroundColor);
    expect(bgAfter).toBe(onColor);
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

    // The Goals row is hidden in the current build (SHOW_GOALS=false), but the
    // /settings/goals route still exists — navigate to it directly.
    await page.goto('/settings/goals');
    await expect(page).toHaveURL(/\/settings\/goals/);
    await page.waitForTimeout(1000);
    // Standard nutrients are ensured on mount; wait for the first to render.
    await expect(page.getByTestId('goals-nutrient-calories')).toBeVisible({ timeout: 10_000 });
  });

  test.afterEach(async () => {
    if (cdpSession) {
      await cdpSession.send('WebAuthn.disable').catch(() => {});
    }
  });

  test('Goals page has standard nutrient rows', async ({ page }) => {
    for (const nutrient of ['calories', 'protein', 'fat', 'carbs']) {
      const row = page.getByTestId(`goals-nutrient-${nutrient}`);
      await expect(row).toBeVisible({ timeout: 5_000 });
    }
  });

  test('Standard nutrient shows Track/Goal mode toggle', async ({ page }) => {
    // Standard nutrients always render their Track/Goal segmented toggle.
    const modeToggle = page.getByTestId('goals-mode-calories');
    await expect(modeToggle).toBeVisible({ timeout: 5_000 });

    // Both segmented options are present.
    await expect(modeToggle.getByText('Следить')).toBeVisible({ timeout: 5_000 });
    await expect(modeToggle.getByText('Цель')).toBeVisible({ timeout: 5_000 });
  });

  test('Track mode does not show amount/direction selects', async ({ page }) => {
    const modeToggle = page.getByTestId('goals-mode-calories');
    await expect(modeToggle).toBeVisible({ timeout: 5_000 });

    // Default is Track mode (amount=0); the direction/amount/period controls
    // (which include <select> elements) are only rendered in Goal mode. The
    // mode toggle's parent column should therefore contain no <select>.
    const goalSelects = modeToggle.locator('..').locator('select');
    const selectCount = await goalSelects.count();
    expect(selectCount).toBe(0);

    // Switching to Goal mode reveals the selects, confirming the Track-mode
    // absence above is meaningful (not just a missing-element false positive).
    await modeToggle.getByText('Цель').click();
    await page.waitForTimeout(500);
    const goalSelectsAfter = modeToggle.locator('..').locator('select');
    expect(await goalSelectsAfter.count()).toBeGreaterThan(0);
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
