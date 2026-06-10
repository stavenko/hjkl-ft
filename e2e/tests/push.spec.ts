import { test, expect, type CDPSession } from '@playwright/test';
import { registerAccount } from './helpers';

const AUTH_BASE = 'https://auth-worker.vg-stavenko.workers.dev';

test.describe('Push notification flow', () => {
  test('VAPID public key endpoint works', async ({ request }) => {
    const resp = await request.get(`${AUTH_BASE}/push/vapid-key`);
    expect(resp.status()).toBe(200);

    const body = await resp.json();
    expect(body).toHaveProperty('public_key');
    expect(typeof body.public_key).toBe('string');
    expect(body.public_key.length).toBeGreaterThan(0);

    // Decode base64url to bytes — uncompressed P-256 point is 65 bytes
    const raw = body.public_key
      .replace(/-/g, '+')
      .replace(/_/g, '/');
    const bin = Buffer.from(raw, 'base64');
    expect(bin.length).toBe(65);
  });

  test('Push subscribe endpoint requires auth', async ({ request }) => {
    const resp = await request.post(`${AUTH_BASE}/push/subscribe`, {
      data: {
        endpoint: 'https://fake-push.example.com/sub/123',
        keys: {
          p256dh: 'AAAA',
          auth: 'BBBB',
        },
      },
      headers: { 'Content-Type': 'application/json' },
    });
    expect(resp.status()).toBe(401);
  });

  test('Push subscribe with valid token', async ({ page }) => {
    await page.goto('/');
    await page.evaluate(() => localStorage.clear());
    await page.reload();
    await page.waitForTimeout(3000);

    const { cdpSession, userId } = await registerAccount(page);

    // Grab the auth token from localStorage
    const token = await page.evaluate(() => localStorage.getItem('auth_token'));
    expect(token).toBeTruthy();

    // POST /push/subscribe with Bearer token and mock subscription data
    const resp = await page.request.post(`${AUTH_BASE}/push/subscribe`, {
      data: {
        endpoint: 'https://fcm.googleapis.com/fcm/send/test-e2e-fake',
        keys: {
          p256dh: 'BNcRdreALRFXTkOOUHK1EtK2wtaz5Ry4YfYCA_0QTpQtUbVlUls0VJXg7A8u-Ts1XbjhazAkj7I99e8p8REfW04',
          auth: 'tBHItJI5svbpC7-BqpHMXA',
        },
      },
      headers: {
        'Content-Type': 'application/json',
        Authorization: `Bearer ${token}`,
      },
    });
    expect(resp.status()).toBe(200);

    await cdpSession.send('WebAuthn.disable').catch(() => {});
  });

  test('Settings shows notification toggle', async ({ page }) => {
    await page.goto('/');
    await page.evaluate(() => localStorage.clear());
    await page.reload();
    await page.waitForTimeout(3000);

    const { cdpSession } = await registerAccount(page);

    // Navigate to settings
    const navSettings = page.getByTestId('nav-settings');
    await expect(navSettings).toBeVisible({ timeout: 10_000 });
    await navSettings.click();
    await expect(page).toHaveURL(/\/settings/);
    await page.waitForTimeout(1000);

    // Verify "Уведомления" / "Notifications" section heading exists
    const notifHeader = page.locator('h2', { hasText: /Уведомления|Notifications/ });
    await expect(notifHeader).toBeVisible({ timeout: 5_000 });

    // Verify the enable/disable button is present
    // The button text depends on push support; in headless Chromium without
    // a service worker the page shows "not supported" text instead of a button.
    // We check for either the button OR the "not supported" message.
    const enableBtn = page.locator('button', { hasText: /Включить|Enable|Отключить|Disable/ });
    const notSupported = page.locator('text=/не поддерживаются|not supported/');
    const hasButton = await enableBtn.isVisible().catch(() => false);
    const hasNotSupported = await notSupported.isVisible().catch(() => false);
    expect(hasButton || hasNotSupported).toBe(true);

    await cdpSession.send('WebAuthn.disable').catch(() => {});
  });
});
