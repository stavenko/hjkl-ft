import { test, expect, type CDPSession } from '@playwright/test';

// Clear localStorage before each test so onboarding starts fresh
test.beforeEach(async ({ page }) => {
  await page.goto('/');
  await page.evaluate(() => localStorage.clear());
  await page.reload();
  // Wait for WASM to load
  await page.waitForTimeout(3000);
});

test.describe('PWA install prompt', () => {
  test('shows PWA prompt on first visit in browser', async ({ page }) => {
    // Should see the PWA install prompt (not the auth page, not the app)
    const description = page.locator('text=питания');
    await expect(description).toBeVisible({ timeout: 10_000 });

    // Should see the dismiss button
    const dismissBtn = page.getByText('Я хочу использовать в браузере');
    await expect(dismissBtn).toBeVisible();

    // Should NOT see the auth page yet
    await expect(page.getByText('Создать аккаунт')).not.toBeVisible();
  });

  test('dismiss PWA prompt → shows auth page', async ({ page }) => {
    // Wait for PWA prompt
    const dismissBtn = page.getByText('Я хочу использовать в браузере');
    await expect(dismissBtn).toBeVisible({ timeout: 10_000 });

    // Click dismiss
    await dismissBtn.click();

    // Should now see the auth page
    const createBtn = page.getByText('Создать аккаунт');
    await expect(createBtn).toBeVisible({ timeout: 5_000 });

    // localStorage should have pwa_dismissed
    const dismissed = await page.evaluate(() => localStorage.getItem('pwa_dismissed'));
    expect(dismissed).toBe('true');
  });

  test('PWA prompt stays dismissed after reload', async ({ page }) => {
    // Dismiss PWA prompt
    const dismissBtn = page.getByText('Я хочу использовать в браузере');
    await expect(dismissBtn).toBeVisible({ timeout: 10_000 });
    await dismissBtn.click();

    // Reload
    await page.reload();
    await page.waitForTimeout(3000);

    // Should go straight to auth page, not PWA prompt
    const createBtn = page.getByText('Создать аккаунт');
    await expect(createBtn).toBeVisible({ timeout: 10_000 });
  });
});

test.describe('Account creation with PassKey', () => {
  let cdpSession: CDPSession;

  test.beforeEach(async ({ page }) => {
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
  });

  test.afterEach(async () => {
    if (cdpSession) {
      await cdpSession.send('WebAuthn.disable').catch(() => {});
    }
  });

  test('shows auth page after dismissing PWA prompt', async ({ page }) => {
    const createBtn = page.getByText('Создать аккаунт');
    await expect(createBtn).toBeVisible({ timeout: 10_000 });

    const loginBtn = page.getByText('У меня уже есть аккаунт');
    await expect(loginBtn).toBeVisible();
  });

  test('create account triggers PassKey flow and lands on app', async ({ page }) => {
    // Intercept network to debug
    const requests: string[] = [];
    page.on('request', req => {
      if (req.url().includes('auth-worker') || req.url().includes('/register')) {
        requests.push(`${req.method()} ${req.url()}`);
      }
    });
    page.on('requestfailed', req => {
      requests.push(`FAILED ${req.method()} ${req.url()} ${req.failure()?.errorText}`);
    });
    page.on('response', resp => {
      if (resp.url().includes('auth-worker') || resp.url().includes('/register')) {
        requests.push(`RESP ${resp.status()} ${resp.url()}`);
      }
    });

    const createBtn = page.getByText('Создать аккаунт');
    await expect(createBtn).toBeVisible({ timeout: 10_000 });

    // Capture console errors for debugging
    page.on('console', msg => {
      if (msg.type() === 'error') console.log('BROWSER ERROR:', msg.text());
    });
    page.on('pageerror', err => console.log('PAGE ERROR:', err.message));

    // Click create account
    await createBtn.click();

    // Wait for either the main app or an error
    const result = await Promise.race([
      page.getByText('Дневник').waitFor({ timeout: 20_000 }).then(() => 'app'),
      page.locator('.notification.is-danger').waitFor({ timeout: 20_000 }).then(() => 'error'),
    ]).catch(() => 'timeout');

    console.log('Network log:', requests);

    if (result === 'error') {
      const errorText = await page.locator('.notification.is-danger').textContent();
      console.log('Error shown to user:', errorText);
      // Take screenshot for debugging
      await page.screenshot({ path: 'test-results/auth-error.png' });
    }

    if (result === 'app') {
      const userId = await page.evaluate(() => localStorage.getItem('user_id'));
      expect(userId).toBeTruthy();
      expect(userId!.length).toBeGreaterThan(0);
    } else {
      // Fail with details
      const errorText = result === 'error'
        ? await page.locator('.notification.is-danger').textContent()
        : 'timeout — no response';
      expect.soft(result).toBe('app');
      console.error(`Auth failed: ${errorText}\nRequests: ${JSON.stringify(requests, null, 2)}`);
    }
  });
});
