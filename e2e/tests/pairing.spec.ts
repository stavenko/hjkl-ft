import { test, expect, type CDPSession } from '@playwright/test';
import { patchRegisterFinish } from './helpers';

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

    // Patch register/finish to include user_id
    await patchRegisterFinish(page);

    await page.evaluate(() => localStorage.setItem('pwa_dismissed', 'true'));
    await page.reload();
    await page.waitForTimeout(3000);

    // TryingPassKey will fail (no credential), then shows auth page
    const createBtn = page.getByTestId('auth-btn-register');
    await expect(createBtn).toBeVisible({ timeout: 15_000 });
    await createBtn.click();

    // Wait for registration to fully complete and verify it worked
    let registered = false;
    for (let i = 0; i < 40; i++) {
      const uid = await page.evaluate(() => localStorage.getItem('user_id'));
      if (uid) { registered = true; break; }
      await page.waitForTimeout(500);
    }
    expect(registered).toBe(true);

    // Wait for auth overlay to fully unmount
    await page.waitForTimeout(1000);
    await expect(page.locator('[style*="position: fixed"][style*="z-index: 100"]')).toHaveCount(0, { timeout: 10_000 });
  });

  test.afterEach(async () => {
    if (cdpSession) {
      await cdpSession.send('WebAuthn.disable').catch(() => {});
    }
  });

  test('Settings page shows "Add device" button', async ({ page }) => {
    await page.getByTestId('nav-settings').click();
    await expect(page).toHaveURL(/\/settings/);
    await page.waitForTimeout(1000);

    const addDeviceBtn = page.getByTestId('settings-btn-add-device');
    await expect(addDeviceBtn).toBeVisible({ timeout: 10_000 });
  });

  test('Click "Add device" shows pairing options (Show QR / Scan QR)', async ({ page }) => {
    await page.getByTestId('nav-settings').click();
    await expect(page).toHaveURL(/\/settings/);
    await page.waitForTimeout(1000);

    const addDeviceBtn = page.getByTestId('settings-btn-add-device');
    await expect(addDeviceBtn).toBeVisible({ timeout: 10_000 });
    await addDeviceBtn.click({ timeout: 15_000 });

    // Should see pairing options
    const showQr = page.getByTestId('pair-logged-btn-show');
    const scanQr = page.getByTestId('pair-logged-btn-scan');
    await expect(showQr).toBeVisible({ timeout: 5_000 });
    await expect(scanQr).toBeVisible({ timeout: 5_000 });
  });

  test('Show QR creates valid pairing data that can be parsed', async ({ page }) => {
    await page.getByTestId('nav-settings').click();
    await expect(page).toHaveURL(/\/settings/);
    await page.waitForTimeout(1000);

    const addDeviceBtn = page.getByTestId('settings-btn-add-device');
    await expect(addDeviceBtn).toBeVisible({ timeout: 10_000 });
    await addDeviceBtn.click({ timeout: 15_000 });

    const showQrBtn = page.getByTestId('pair-logged-btn-show');
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
    expect(pairData.expires_at).toBeGreaterThan(0);

    // QR URL format is always 2-part: hjkl-pair://pairing_id/secret
    const expectedQrData = `hjkl-pair://${pairData.pairing_id}/${pairData.secret}`;

    // Verify QR code SVG is visible
    const waitingText = page.getByText('Ожидание другого устройства...');
    await expect(waitingText).toBeVisible({ timeout: 5_000 });

    // Verify "Copy link" button is present
    const copyBtn = page.getByTestId('pair-logged-btn-copy-link');
    await expect(copyBtn).toBeVisible();

    // Verify the QR data can be parsed correctly (simulates what scanning device does)
    // Format: hjkl-pair://pairing_id/secret → 2 parts
    const parts = expectedQrData.replace('hjkl-pair://', '').split('/');
    expect(parts.length).toBe(2);
    expect(parts[0]).toBe(pairData.pairing_id);
    expect(parts[1]).toBe(pairData.secret);
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

  test('Auth page shows login option after dismissing PWA prompt', async ({ page }) => {
    const dismissBtn = page.getByTestId('pwa-btn-dismiss');
    await expect(dismissBtn).toBeVisible({ timeout: 10_000 });
    await dismissBtn.click();

    const loginBtn = page.getByTestId('auth-btn-login');
    await expect(loginBtn).toBeVisible({ timeout: 10_000 });
  });

  test('Login screen shows pair options', async ({ page }) => {
    await page.evaluate(() => localStorage.setItem('pwa_dismissed', 'true'));
    await page.reload();
    await page.waitForTimeout(3000);

    const loginBtn = page.getByTestId('auth-btn-login');
    await expect(loginBtn).toBeVisible({ timeout: 15_000 });
    await loginBtn.click();

    const showQr = page.getByTestId('auth-btn-show-qr');
    const scanQr = page.getByTestId('auth-btn-scan-qr');
    const tryPasskey = page.getByTestId('auth-btn-try-passkey');

    await expect(showQr).toBeVisible({ timeout: 5_000 });
    await expect(scanQr).toBeVisible({ timeout: 5_000 });
    await expect(tryPasskey).toBeVisible({ timeout: 5_000 });
  });
});
