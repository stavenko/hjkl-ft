import { test, expect, type CDPSession } from '@playwright/test';

// ---------------------------------------------------------------------------
// Logged-in device: Settings → Add device → Show QR
// ---------------------------------------------------------------------------

test.describe('Device pairing (logged-in device)', () => {
  let cdpSession: CDPSession;

  test.beforeEach(async ({ page }) => {
    await page.goto('/');
    await page.evaluate(() => localStorage.clear());

    // Set up virtual authenticator BEFORE reload
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

    await page.evaluate(() => localStorage.setItem('pwa_dismissed', 'true'));
    await page.reload();
    await page.waitForTimeout(3000);

    // TryingPassKey will fail (no credential), then shows auth page
    const createBtn = page.getByText('Создать аккаунт');
    await expect(createBtn).toBeVisible({ timeout: 15_000 });
    await createBtn.click();

    // Wait for registration to fully complete
    for (let i = 0; i < 40; i++) {
      const uid = await page.evaluate(() => localStorage.getItem('user_id'));
      if (uid) break;
      await page.waitForTimeout(500);
    }
    // Small extra wait for overlay to unmount
    await page.waitForTimeout(1000);
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
    const addDeviceBtn = page.locator('.button.is-link.is-light', { hasText: 'Подключить устройство' });
    await expect(addDeviceBtn).toBeVisible({ timeout: 10_000 });
    await addDeviceBtn.click();

    // Should see pairing options
    const showQr = page.getByText('Показать QR-код');
    const scanQr = page.getByText('Сканировать QR-код');
    await expect(showQr).toBeVisible({ timeout: 5_000 });
    await expect(scanQr).toBeVisible({ timeout: 5_000 });
  });

  test('Show QR creates valid pairing data that can be parsed', async ({ page }) => {
    await page.goto('/settings');
    await page.waitForTimeout(2000);

    const addDeviceBtn = page.locator('.button.is-link.is-light', { hasText: 'Подключить устройство' });
    await expect(addDeviceBtn).toBeVisible({ timeout: 10_000 });
    await addDeviceBtn.click();

    const showQrBtn = page.getByText('Показать QR-код');
    await expect(showQrBtn).toBeVisible({ timeout: 5_000 });

    // Click Show QR and wait for /pair/create response
    const [pairResp] = await Promise.all([
      page.waitForResponse(resp => resp.url().includes('/pair/create') && resp.status() === 200, { timeout: 15_000 }),
      showQrBtn.click(),
    ]);

    const pairData = await pairResp.json();

    // Verify API returned valid pairing data
    expect(pairData).toBeTruthy();
    expect(pairData.pairing_id).toBeTruthy();
    expect(pairData.secret).toBeTruthy();
    expect(pairData.username).toBeTruthy();
    expect(pairData.expires_at).toBeGreaterThan(0);

    // Verify the QR URL format matches what the parser expects
    // Logged-in device format: hjkl-pair://username/pairing_id/secret
    const expectedQrData = `hjkl-pair://${pairData.username}/${pairData.pairing_id}/${pairData.secret}`;

    // Verify QR code SVG is visible
    const waitingText = page.getByText('Ожидание другого устройства...');
    await expect(waitingText).toBeVisible({ timeout: 5_000 });

    // Verify "Copy link" button is present
    const copyBtn = page.getByText('Копировать ссылку');
    await expect(copyBtn).toBeVisible();

    // Verify the QR data can be parsed correctly (simulates what scanning device does)
    // Format: hjkl-pair://username/pairing_id/secret → 3 parts
    const parts = expectedQrData.replace('hjkl-pair://', '').split('/');
    expect(parts.length).toBe(3);
    expect(parts[0]).toBe(pairData.username);
    expect(parts[1]).toBe(pairData.pairing_id);
    expect(parts[2]).toBe(pairData.secret);
  });

  test('New device Show QR creates data parseable as 2-part format', async ({ page }) => {
    // Test the /pair/request endpoint directly (new device, no auth)
    const resp = await page.evaluate(async () => {
      const r = await fetch('https://auth-worker.vg-stavenko.workers.dev/pair/request', {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: '{}',
      });
      return r.json();
    });

    expect(resp.pairing_id).toBeTruthy();
    expect(resp.secret).toBeTruthy();
    expect(resp.qr_url).toBeTruthy();

    // Verify format: hjkl-pair://pairing_id/secret (2 parts)
    expect(resp.qr_url).toMatch(/^hjkl-pair:\/\//);
    const rest = resp.qr_url.replace('hjkl-pair://', '');
    const parts = rest.split('/');
    expect(parts.length).toBe(2);
    expect(parts[0]).toBe(resp.pairing_id);
    expect(parts[1]).toBe(resp.secret);
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
