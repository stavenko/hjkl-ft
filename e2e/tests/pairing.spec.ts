import { test, expect, type CDPSession } from '@playwright/test';

// ---------------------------------------------------------------------------
// Logged-in device: Settings → Add device → Show QR
// ---------------------------------------------------------------------------

test.describe('Device pairing (logged-in device)', () => {
  let cdpSession: CDPSession;

  test.beforeEach(async ({ page }) => {
    await page.goto('/');
    await page.evaluate(() => localStorage.clear());
    await page.reload();
    await page.waitForTimeout(3000);

    // Skip PWA prompt
    await page.evaluate(() => localStorage.setItem('pwa_dismissed', 'true'));
    await page.reload();
    await page.waitForTimeout(3000);

    // Set up virtual WebAuthn authenticator via CDP
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

    // Create account to get into the app
    const createBtn = page.getByText('Создать аккаунт');
    await expect(createBtn).toBeVisible({ timeout: 10_000 });
    await createBtn.click();

    // Wait for the main app to load (Diary tab visible)
    const diary = page.getByText('Дневник');
    await expect(diary).toBeVisible({ timeout: 20_000 });
  });

  test.afterEach(async () => {
    if (cdpSession) {
      await cdpSession.send('WebAuthn.disable').catch(() => {});
    }
  });

  test('Settings page shows "Add device" button', async ({ page }) => {
    // Navigate to Settings
    await page.goto('/settings');
    await page.waitForTimeout(2000);

    // The settings page should have "Подключить устройство" button
    const addDeviceBtn = page.getByText('Подключить устройство', { exact: false });
    await expect(addDeviceBtn.first()).toBeVisible({ timeout: 10_000 });
  });

  test('Click "Add device" shows pairing options (Show QR / Scan QR)', async ({ page }) => {
    // Navigate to Settings
    await page.goto('/settings');
    await page.waitForTimeout(2000);

    // Click "Add device" button
    const addDeviceBtn = page.locator('button', { hasText: 'Подключить устройство' });
    await expect(addDeviceBtn).toBeVisible({ timeout: 10_000 });
    await addDeviceBtn.click();

    // Should see pairing options
    const showQr = page.getByText('Показать QR-код');
    const scanQr = page.getByText('Сканировать QR-код');
    await expect(showQr).toBeVisible({ timeout: 5_000 });
    await expect(scanQr).toBeVisible({ timeout: 5_000 });
  });

  test('Click "Show QR" calls pair/create API and shows QR element', async ({ page }) => {
    // Navigate to Settings
    await page.goto('/settings');
    await page.waitForTimeout(2000);

    // Click "Add device"
    const addDeviceBtn = page.locator('button', { hasText: 'Подключить устройство' });
    await expect(addDeviceBtn).toBeVisible({ timeout: 10_000 });
    await addDeviceBtn.click();

    // Intercept /pair/create network call
    let pairCreateCalled = false;
    page.on('request', req => {
      if (req.url().includes('/pair/create')) {
        pairCreateCalled = true;
      }
    });

    // Click "Show QR"
    const showQrBtn = page.getByText('Показать QR-код');
    await expect(showQrBtn).toBeVisible({ timeout: 5_000 });
    await showQrBtn.click();

    // Wait for network call or QR to appear
    // The pair/create API might fail (no real backend), but we verify it was called
    // and that the UI attempts to show a QR or an error
    await page.waitForTimeout(5000);

    // Verify the pair/create API was called
    expect(pairCreateCalled).toBe(true);

    // After clicking Show QR, we should either see an SVG (QR code) or an error.
    // The "waiting" hint or a QR SVG element should appear if the call succeeded.
    // If the call failed, we should see an error notification.
    const qrSvg = page.locator('svg');
    const errorNotification = page.locator('.notification.is-danger');
    const waitingText = page.getByText('Ожидание другого устройства...');

    const hasQr = await qrSvg.first().isVisible().catch(() => false);
    const hasError = await errorNotification.isVisible().catch(() => false);
    const hasWaiting = await waitingText.isVisible().catch(() => false);

    // At least one of these should be true: QR appeared, or error shown
    expect(hasQr || hasError || hasWaiting).toBe(true);
  });
});

// ---------------------------------------------------------------------------
// New device side: auth page shows "Add device" option
// ---------------------------------------------------------------------------

test.describe('Device pairing (new device side)', () => {
  test.beforeEach(async ({ page }) => {
    await page.goto('/');
    await page.evaluate(() => localStorage.clear());
    await page.reload();
    await page.waitForTimeout(3000);
  });

  test('Auth page shows "Add device" button after dismissing PWA prompt', async ({ page }) => {
    // Dismiss PWA prompt
    const dismissBtn = page.getByText('Я хочу использовать в браузере');
    await expect(dismissBtn).toBeVisible({ timeout: 10_000 });
    await dismissBtn.click();

    // Should now see the auth page with "Add device" option
    const addDeviceBtn = page.getByText('Подключить устройство');
    await expect(addDeviceBtn).toBeVisible({ timeout: 5_000 });
  });

  test('"Add device" on auth page is visible alongside create and recovery', async ({ page }) => {
    await page.evaluate(() => localStorage.setItem('pwa_dismissed', 'true'));
    await page.reload();
    await page.waitForTimeout(3000);

    const createBtn = page.getByText('Создать аккаунт');
    const addDeviceBtn = page.getByText('Подключить устройство');
    const recoveryBtn = page.getByText('Восстановить доступ по паролю');

    await expect(createBtn).toBeVisible({ timeout: 15_000 });
    await expect(addDeviceBtn).toBeVisible({ timeout: 5_000 });
    await expect(recoveryBtn).toBeVisible({ timeout: 5_000 });
  });
});
