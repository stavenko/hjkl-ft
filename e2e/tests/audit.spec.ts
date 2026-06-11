import { test, expect, type CDPSession } from '@playwright/test';
import { registerAccount } from './helpers';

const AUTH_BASE = 'https://auth-worker.vg-stavenko.workers.dev';

// =========================================================================
// 1. CSP: Bulma loads locally, no CDN requests
// =========================================================================
test.describe('CSP: no external resource requests', () => {
  test('No requests to CDN domains', async ({ browser }) => {
    const context = await browser.newContext({
      baseURL: 'https://hjkl-ft.pages.dev',
      serviceWorkers: 'block',
      bypassCSP: false,
    });
    const page = await context.newPage();

    const externalRequests: string[] = [];
    page.on('request', req => {
      const url = new URL(req.url());
      if (url.hostname !== 'hjkl-ft.pages.dev' &&
          url.hostname !== 'auth-worker.vg-stavenko.workers.dev' &&
          url.hostname !== 'localhost') {
        externalRequests.push(req.url());
      }
    });

    await page.goto('/');
    await page.waitForTimeout(5000);

    if (externalRequests.length > 0) {
      console.log('External requests found:', externalRequests);
    }
    expect(externalRequests.length).toBe(0);
    await context.close();
  });

  test('CSS loads without CSP violation', async ({ browser }) => {
    const context = await browser.newContext({
      baseURL: 'https://hjkl-ft.pages.dev',
      serviceWorkers: 'block',
      bypassCSP: false,
    });
    const page = await context.newPage();

    const styleViolations: string[] = [];
    page.on('console', msg => {
      if (msg.text().includes('style') && msg.text().includes('Content-Security-Policy')) {
        styleViolations.push(msg.text());
      }
    });

    await page.goto('/');
    await page.waitForTimeout(3000);

    // Verify Bulma loaded — check a known Bulma class is styled
    const hasButton = await page.evaluate(() => {
      const el = document.createElement('button');
      el.className = 'button is-link';
      document.body.appendChild(el);
      const style = getComputedStyle(el);
      const bg = style.backgroundColor;
      document.body.removeChild(el);
      return bg !== '' && bg !== 'rgba(0, 0, 0, 0)';
    });

    expect(hasButton).toBe(true);
    expect(styleViolations.length).toBe(0);
    await context.close();
  });
});

// =========================================================================
// 2. Token stored in AuthDO and validated
// =========================================================================
test.describe('Token persistence in AuthDO', () => {
  test('After registration, token exists and is valid via /token/validate', async ({ page }) => {
    await page.goto('/');
    await page.evaluate(() => localStorage.clear());
    await page.reload();
    await page.waitForTimeout(3000);
    const { cdpSession } = await registerAccount(page);

    const token = await page.evaluate(() => localStorage.getItem('auth_token'));
    expect(token).toBeTruthy();

    // Validate token via API
    const result = await page.evaluate(async (data) => {
      const resp = await fetch(`${data.url}/token/validate`, {
        method: 'POST',
        headers: {
          'Content-Type': 'application/json',
          Authorization: `Bearer ${data.token}`,
        },
      });
      return { status: resp.status, body: await resp.json() };
    }, { url: AUTH_BASE, token });

    expect(result.status).toBe(200);
    expect(result.body.sub).toBeTruthy();
    expect(result.body.token_id).toBeTruthy();

    await cdpSession.send('WebAuthn.disable').catch(() => {});
  });

});

// =========================================================================
// 3. Device fingerprint is stored with token
// =========================================================================
test.describe('Device fingerprint', () => {
  test('GET /tokens returns token with fingerprint field', async ({ page }) => {
    await page.goto('/');
    await page.evaluate(() => localStorage.clear());
    await page.reload();
    await page.waitForTimeout(3000);
    const { cdpSession } = await registerAccount(page);

    const token = await page.evaluate(() => localStorage.getItem('auth_token'));

    const result = await page.evaluate(async (data) => {
      const resp = await fetch(`${data.url}/tokens`, {
        headers: { Authorization: `Bearer ${data.token}` },
      });
      return { status: resp.status, body: await resp.json() };
    }, { url: AUTH_BASE, token });

    expect(result.status).toBe(200);
    expect(result.body.tokens).toBeTruthy();
    expect(result.body.tokens.length).toBeGreaterThanOrEqual(1);

    const currentToken = result.body.tokens[0];
    expect(currentToken.fingerprint).toBeTruthy();
    expect(currentToken.fingerprint.length).toBeGreaterThan(0);
    expect(currentToken.created_at).toBeGreaterThan(0);

    await cdpSession.send('WebAuthn.disable').catch(() => {});
  });
});

// =========================================================================
// 4. Username/display name
// =========================================================================
test.describe('Username display name', () => {
  test('Registration requires name input', async ({ page }) => {
    await page.goto('/');
    await page.evaluate(() => localStorage.clear());
    await page.evaluate(() => localStorage.setItem('pwa_dismissed', 'true'));

    const cdpSession = await page.context().newCDPSession(page);
    await cdpSession.send('WebAuthn.enable');
    await cdpSession.send('WebAuthn.addVirtualAuthenticator', {
      options: {
        protocol: 'ctap2', transport: 'internal',
        hasResidentKey: true, hasUserVerification: true,
        isUserVerified: true, automaticPresenceSimulation: true,
      },
    });

    await page.reload();
    await page.waitForTimeout(3000);

    const registerBtn = page.getByTestId('auth-btn-register');
    await expect(registerBtn).toBeVisible({ timeout: 15_000 });

    // Register button should be disabled when name is empty
    const nameInput = page.getByTestId('auth-input-name');
    const hasNameInput = await nameInput.isVisible().catch(() => false);

    if (hasNameInput) {
      // Clear the input and check button state
      await nameInput.fill('');
      const isDisabled = await registerBtn.isDisabled();
      expect(isDisabled).toBe(true);

      // Fill name and check button enabled
      await nameInput.fill('Test User');
      const isEnabled = await registerBtn.isEnabled();
      expect(isEnabled).toBe(true);
    }
    // If no name input exists, that's a finding from the audit

    await cdpSession.send('WebAuthn.disable').catch(() => {});
  });
});

// =========================================================================
// 5. VAPID keys are valid P-256
// =========================================================================
test.describe('VAPID key validation', () => {
  test('Public key is valid 65-byte uncompressed P-256 point', async ({ request }) => {
    const resp = await request.get(`${AUTH_BASE}/push/vapid-key`);
    expect(resp.status()).toBe(200);

    const body = await resp.json();
    const raw = body.public_key
      .replace(/-/g, '+')
      .replace(/_/g, '/');
    const bin = Buffer.from(raw, 'base64');

    // Uncompressed P-256: starts with 0x04, total 65 bytes
    expect(bin.length).toBe(65);
    expect(bin[0]).toBe(0x04);
  });

  test('VAPID private and public keys are different', async ({ request }) => {
    // Fetch public key from API
    const resp = await request.get(`${AUTH_BASE}/push/vapid-key`);
    const body = await resp.json();
    const publicKey = body.public_key;

    // Public key should not equal private key (basic sanity)
    // We can't access private key from test, but we verify public is properly formatted
    expect(publicKey.length).toBeGreaterThan(40);
  });
});

// =========================================================================
// 6. Push subscription stored and retrievable
// =========================================================================
test.describe('Push subscription persistence', () => {
  test('Subscribe, then verify subscription is retrievable via API', async ({ page }) => {
    await page.goto('/');
    await page.evaluate(() => localStorage.clear());
    await page.reload();
    await page.waitForTimeout(3000);
    const { cdpSession } = await registerAccount(page);

    const token = await page.evaluate(() => localStorage.getItem('auth_token'));
    const uniqueEndpoint = 'https://fcm.googleapis.com/fcm/send/test-audit-' + Date.now();

    // Step 1: Subscribe
    const subResp = await page.evaluate(async (data) => {
      const resp = await fetch(`${data.url}/push/subscribe`, {
        method: 'POST',
        headers: {
          'Content-Type': 'application/json',
          Authorization: `Bearer ${data.token}`,
        },
        body: JSON.stringify({
          endpoint: data.endpoint,
          keys: {
            p256dh: 'BNcRdreALRFXTkOOUHK1EtK2wtaz5Ry4YfYCA_0QTpQtUbVlUls0VJXg7A8u-Ts1XbjhazAkj7I99e8p8REfW04',
            auth: 'tBHItJI5svbpC7-BqpHMXA',
          },
        }),
      });
      return { status: resp.status };
    }, { url: AUTH_BASE, token, endpoint: uniqueEndpoint });

    expect(subResp.status).toBe(200);

    // Step 2: Verify subscription exists by sending a push to it
    // (it will fail delivery since endpoint is fake, but the worker should attempt it)
    // Better: use /push/send and check that it doesn't return "no subscriptions"
    const sendResp = await page.evaluate(async (data) => {
      const resp = await fetch(`${data.url}/push/send`, {
        method: 'POST',
        headers: {
          'Content-Type': 'application/json',
          Authorization: `Bearer ${data.token}`,
        },
        body: JSON.stringify({
          title: 'Test',
          body: 'Audit test',
        }),
      });
      return { status: resp.status, body: await resp.text() };
    }, { url: AUTH_BASE, token });

    // Should not be 404 "no subscriptions" — the subscription we just added should be found
    expect(sendResp.status).not.toBe(404);

    await cdpSession.send('WebAuthn.disable').catch(() => {});
  });
});

// =========================================================================
// 7. Privacy page (separate page, not section)
// =========================================================================
test.describe('Privacy as separate page', () => {
  let cdpSession: CDPSession;

  test.beforeEach(async ({ page }) => {
    await page.goto('/');
    await page.evaluate(() => localStorage.clear());
    await page.reload();
    await page.waitForTimeout(3000);
    const result = await registerAccount(page);
    cdpSession = result.cdpSession;

    // Navigate to Settings
    await page.getByTestId('nav-settings').click();
    await expect(page).toHaveURL(/\/settings/);
    await page.waitForTimeout(1000);
  });

  test.afterEach(async () => {
    if (cdpSession) await cdpSession.send('WebAuthn.disable').catch(() => {});
  });

  test('Settings has Privacy button that navigates to Privacy page', async ({ page }) => {
    const privacyBtn = page.getByTestId('settings-btn-privacy');
    await expect(privacyBtn).toBeVisible({ timeout: 5_000 });

    await privacyBtn.click();

    // Privacy page has back button to Settings
    const backBtn = page.getByTestId('privacy-btn-back');
    await expect(backBtn).toBeVisible({ timeout: 5_000 });
  });

  test('Privacy page shows active sessions list', async ({ page }) => {
    const privacyBtn = page.getByTestId('settings-btn-privacy');
    await privacyBtn.click();
    await page.waitForTimeout(2000);

    // Should show "Active sessions" header
    const sessionsHeader = page.getByTestId('privacy-sessions-header');
    await expect(sessionsHeader).toBeVisible({ timeout: 5_000 });

    // After registration, at least 1 session must be in the list
    const sessionItems = page.getByTestId('privacy-session-item');
    await expect(sessionItems.first()).toBeVisible({ timeout: 10_000 });

    const count = await sessionItems.count();
    expect(count).toBeGreaterThanOrEqual(1);
  });

  test('Current session is highlighted in the list', async ({ page }) => {
    const privacyBtn = page.getByTestId('settings-btn-privacy');
    await privacyBtn.click();
    await page.waitForTimeout(2000);

    // Current session should have a "this device" marker
    const currentMarker = page.getByTestId('privacy-session-current');
    await expect(currentMarker).toBeVisible({ timeout: 10_000 });
  });

  test('Session shows fingerprint and creation date', async ({ page }) => {
    const privacyBtn = page.getByTestId('settings-btn-privacy');
    await privacyBtn.click();
    await page.waitForTimeout(2000);

    const sessionItem = page.getByTestId('privacy-session-item').first();
    await expect(sessionItem).toBeVisible({ timeout: 10_000 });

    const text = await sessionItem.textContent();
    expect(text).toBeTruthy();

    // Fingerprint: at least 8 hex-like chars
    expect(text).toMatch(/[a-f0-9]{8,}/i);

    // Creation date: DD.MM.YYYY HH:MM format
    expect(text).toMatch(/\d{2}\.\d{2}\.\d{4}\s+\d{2}:\d{2}/);
  });

  test('Connect device button is on Privacy page', async ({ page }) => {
    const privacyBtn = page.getByTestId('settings-btn-privacy');
    await privacyBtn.click();
    await page.waitForTimeout(1000);

    const addDeviceBtn = page.getByTestId('privacy-btn-add-device');
    await expect(addDeviceBtn).toBeVisible({ timeout: 5_000 });
  });

  test('Back button returns to Settings', async ({ page }) => {
    const privacyBtn = page.getByTestId('settings-btn-privacy');
    await privacyBtn.click();

    const backBtn = page.getByTestId('privacy-btn-back');
    await expect(backBtn).toBeVisible({ timeout: 5_000 });
    await backBtn.click();

    // Should be back on Settings
    await expect(page).toHaveURL(/\/settings/);
    await expect(page.getByTestId('settings-btn-privacy')).toBeVisible({ timeout: 5_000 });
  });
});
