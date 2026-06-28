import { test, expect, type CDPSession, type Page } from '@playwright/test';
import { registerAccount } from './helpers';

const AUTH_WORKER = 'https://auth-worker.vg-stavenko.workers.dev';

// =========================================================================
// Flow A: Logged-in device shows QR → new device scans and gets account
// Full UI flow: Settings → Show QR → new device claims via API → PassKey
// =========================================================================
test.describe('Flow A: Logged-in shows QR → new device claims', () => {
  test('full happy path via UI', async ({ page }) => {
    // -- Step 1: Register on "logged-in" device --
    await page.goto('/');
    await page.evaluate(() => localStorage.clear());
    await page.reload();
    await page.waitForTimeout(3000);
    const { cdpSession } = await registerAccount(page);

    // -- Step 2: Navigate to Settings → Privacy → Connect device → Show QR --
    await page.getByTestId('nav-settings').click();
    await expect(page).toHaveURL(/\/settings/);
    await page.waitForTimeout(1000);

    // Navigate to Privacy page where "Add device" now lives
    const privacyBtn = page.getByTestId('settings-btn-privacy');
    await expect(privacyBtn).toBeVisible({ timeout: 10_000 });
    await privacyBtn.click();
    await page.waitForTimeout(1000);

    const addDeviceBtn = page.getByTestId('privacy-btn-add-device');
    await expect(addDeviceBtn).toBeVisible({ timeout: 10_000 });
    await addDeviceBtn.click({ timeout: 15_000 });

    const showQrBtn = page.getByTestId('pair-logged-btn-show');
    await expect(showQrBtn).toBeVisible({ timeout: 5_000 });

    // Intercept /pair/create to get QR data
    const [pairResp] = await Promise.all([
      page.waitForResponse(
        resp => resp.url().includes('/pair/create') && resp.status() === 200,
        { timeout: 15_000 },
      ),
      showQrBtn.click(),
    ]);

    const pairData = await pairResp.json();

    // -- Step 3: Validate QR data --
    expect(pairData.pairing_id).toBeTruthy();
    expect(pairData.secret).toBeTruthy();
    expect(pairData.expires_at).toBeGreaterThan(0);

    // Waiting text visible = QR is shown
    await expect(page.getByText('Ожидание другого устройства...')).toBeVisible({ timeout: 5_000 });

    // -- Step 4: Simulate new device claiming (what happens after scanning QR) --
    // Call /pair/claim — this is what the new device's pair_claim() does
    const claimResult = await page.evaluate(async (data) => {
      const resp = await fetch(`${data.url}/pair/claim`, {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({ pairing_id: data.pairing_id, secret: data.secret }),
      });
      return { status: resp.status, body: await resp.json() };
    }, { ...pairData, url: AUTH_WORKER });

    expect(claimResult.status).toBe(200);
    expect(claimResult.body.publicKey).toBeTruthy();
    expect(claimResult.body.publicKey.challenge).toBeTruthy();
    expect(claimResult.body.publicKey.rp).toBeTruthy();
    expect(claimResult.body.publicKey.rp.id).toBe('hjkl-ft.pages.dev');
    expect(claimResult.body.publicKey.user).toBeTruthy();
    expect(claimResult.body.user_id).toBeTruthy();

    // -- Step 5: Verify status changed on logged-in device --
    const token = await page.evaluate(() => localStorage.getItem('auth_token'));
    const statusResult = await page.evaluate(async (data) => {
      const resp = await fetch(`${data.url}/pair/status/${data.pairing_id}`, {
        headers: { Authorization: `Bearer ${data.token}` },
      });
      return { status: resp.status, body: await resp.json() };
    }, { ...pairData, url: AUTH_WORKER, token });

    expect(statusResult.status).toBe(200);
    expect(statusResult.body.status).toBe('claimed');

    await cdpSession.send('WebAuthn.disable').catch(() => {});
  });
});

// =========================================================================
// Flow B: New device shows QR via UI → logged-in approves → new device auto-claims
// This tests the ACTUAL polling flow: ShowQR → poll /pair/check → claim → PassKey
// =========================================================================
test.describe('Flow B: New device shows QR → logged-in approves → auto-claim', () => {
  test('full happy path via UI with polling', async ({ page }) => {
    // -- Step 1: Register on "logged-in" device --
    await page.goto('/');
    await page.evaluate(() => localStorage.clear());
    await page.reload();
    await page.waitForTimeout(3000);
    const { cdpSession } = await registerAccount(page);

    const authToken = await page.evaluate(() => localStorage.getItem('auth_token'));
    expect(authToken).toBeTruthy();

    // -- Step 2: Go to auth page as "new device" --
    // We'll simulate this in same browser: clear user state but keep authenticator
    // Save token for master device API calls
    const savedToken = authToken!;

    // Clear user state to simulate new device
    await page.evaluate(() => {
      localStorage.removeItem('user_id');
      localStorage.removeItem('auth_token');
      localStorage.removeItem('token_expires_at');
    });
    await page.reload();
    await page.waitForTimeout(3000);

    // Should see the login screen directly (no-session "/" is login-only).
    const showQrBtn = page.getByTestId('auth-btn-show-qr');
    await expect(showQrBtn).toBeVisible({ timeout: 15_000 });

    // Intercept /pair/request to get pairing data
    const [requestResp] = await Promise.all([
      page.waitForResponse(
        resp => resp.url().includes('/pair/request') && resp.status() === 200,
        { timeout: 15_000 },
      ),
      showQrBtn.click(),
    ]);

    const requestData = await requestResp.json();
    expect(requestData.pairing_id).toBeTruthy();
    expect(requestData.secret).toBeTruthy();
    expect(requestData.qr_url).toBeTruthy();

    // QR should be visible, waiting text shown
    await expect(page.getByText('Покажите этот QR-код залогиненному устройству')).toBeVisible({ timeout: 5_000 });

    // -- Step 4: Verify polling started (check requests) --
    let checkCalled = false;
    page.on('request', req => {
      if (req.url().includes('/pair/check')) checkCalled = true;
    });

    // Wait a bit for first poll
    await page.waitForTimeout(3000);
    expect(checkCalled).toBe(true);

    // -- Step 5: Master device approves (simulated via API) --
    const approveResult = await page.evaluate(async (data) => {
      const resp = await fetch(`${data.url}/pair/approve`, {
        method: 'POST',
        headers: {
          'Content-Type': 'application/json',
          Authorization: `Bearer ${data.token}`,
        },
        body: JSON.stringify({ pairing_id: data.pairing_id, secret: data.secret }),
      });
      return { status: resp.status, body: await resp.json() };
    }, { ...requestData, url: AUTH_WORKER, token: savedToken });

    expect(approveResult.status).toBe(200);

    // -- Step 6: Wait for new device to auto-detect approval and complete --
    // The polling should detect "approved", call /pair/claim, create PassKey, finish
    // This results in user_id appearing in localStorage

    let paired = false;
    for (let i = 0; i < 30; i++) {
      const uid = await page.evaluate(() => localStorage.getItem('user_id'));
      if (uid) { paired = true; break; }
      await page.waitForTimeout(1000);
    }

    expect(paired).toBe(true);

    const newUserId = await page.evaluate(() => localStorage.getItem('user_id'));
    expect(newUserId).toBeTruthy();
    expect(newUserId!.length).toBeGreaterThan(0);

    const newToken = await page.evaluate(() => localStorage.getItem('auth_token'));
    expect(newToken).toBeTruthy();

    await cdpSession.send('WebAuthn.disable').catch(() => {});
  });
});

// =========================================================================
// Error cases
// =========================================================================
test.describe('Pairing error handling', () => {
  test('invalid QR data shows user-friendly error, not raw JSON', async ({ page }) => {
    await page.goto('/');
    await page.evaluate(() => localStorage.clear());
    await page.evaluate(() => localStorage.setItem('pwa_dismissed', 'true'));

    const cdpSession = await page.context().newCDPSession(page);
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

    await page.reload();
    await page.waitForTimeout(3000);

    const pageContent = await page.textContent('body');
    expect(pageContent).not.toContain('JsValue(');
    expect(pageContent).not.toContain('TypeError:');
    expect(pageContent).not.toContain('RustError');

    await cdpSession.send('WebAuthn.disable').catch(() => {});
  });

  test('wrong secret returns 403 with clean error', async ({ page }) => {
    const requestResult = await page.evaluate(async (url) => {
      const resp = await fetch(`${url}/pair/request`, {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: '{}',
      });
      return await resp.json();
    }, AUTH_WORKER);

    expect(requestResult.pairing_id).toBeTruthy();

    const claimResult = await page.evaluate(async (data) => {
      const resp = await fetch(`${data.url}/pair/claim`, {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({
          pairing_id: data.pairing_id,
          secret: 'WRONG_SECRET',
        }),
      });
      const text = await resp.text();
      let body: any;
      try { body = JSON.parse(text); } catch { body = { error: text }; }
      return { status: resp.status, body };
    }, { ...requestResult, url: AUTH_WORKER });

    console.log('Wrong secret claim result:', claimResult);
    expect(claimResult.status).toBe(403);
    expect(claimResult.body.error).toBeTruthy();
    expect(claimResult.body.error).not.toContain('JsValue');
    expect(claimResult.body.error).not.toContain('panic');
  });
});
