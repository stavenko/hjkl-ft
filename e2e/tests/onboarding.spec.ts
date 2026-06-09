import { test, expect, type CDPSession } from '@playwright/test';

test.beforeEach(async ({ page }) => {
  await page.goto('/');
  await page.evaluate(() => localStorage.clear());
  await page.reload();
  await page.waitForTimeout(3000);
});

test.describe('PWA install prompt', () => {
  test('shows PWA prompt on first visit in browser', async ({ page }) => {
    const description = page.locator('text=питания');
    await expect(description).toBeVisible({ timeout: 10_000 });

    const dismissBtn = page.getByText('Я хочу использовать в браузере');
    await expect(dismissBtn).toBeVisible();

    await expect(page.getByText('Создать аккаунт')).not.toBeVisible();
  });

  test('dismiss PWA prompt → tries PassKey then shows auth page', async ({ page }) => {
    const dismissBtn = page.getByText('Я хочу использовать в браузере');
    await expect(dismissBtn).toBeVisible({ timeout: 10_000 });

    await dismissBtn.click();

    // After dismiss: TryingPassKey (brief loading) → Auth page (no PassKey found)
    const createBtn = page.getByText('Создать аккаунт');
    await expect(createBtn).toBeVisible({ timeout: 15_000 });

    const dismissed = await page.evaluate(() => localStorage.getItem('pwa_dismissed'));
    expect(dismissed).toBe('true');
  });

  test('PWA prompt stays dismissed after reload', async ({ page }) => {
    const dismissBtn = page.getByText('Я хочу использовать в браузере');
    await expect(dismissBtn).toBeVisible({ timeout: 10_000 });
    await dismissBtn.click();

    await page.reload();
    await page.waitForTimeout(3000);

    // Should skip PWA prompt, try PassKey (fail), then show auth
    const createBtn = page.getByText('Создать аккаунт');
    await expect(createBtn).toBeVisible({ timeout: 15_000 });
  });
});

test.describe('Account creation with PassKey', () => {
  let cdpSession: CDPSession;

  test.beforeEach(async ({ page }) => {
    await page.evaluate(() => localStorage.setItem('pwa_dismissed', 'true'));
    await page.reload();
    await page.waitForTimeout(3000);

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

  test('auth page shows three options: create, pair, recovery', async ({ page }) => {
    // After TryingPassKey fails (no credential), shows auth page
    const createBtn = page.getByText('Создать аккаунт');
    await expect(createBtn).toBeVisible({ timeout: 15_000 });

    const pairBtn = page.getByText('Подключить устройство');
    await expect(pairBtn).toBeVisible({ timeout: 10_000 });

    const recoveryLink = page.getByText('Восстановить доступ по паролю');
    await expect(recoveryLink).toBeVisible({ timeout: 5_000 });
  });

  test('create account triggers PassKey flow and lands on app', async ({ page }) => {
    const requests: string[] = [];
    page.on('request', req => {
      if (req.url().includes('auth-worker')) {
        requests.push(`${req.method()} ${req.url()}`);
      }
    });
    page.on('response', resp => {
      if (resp.url().includes('auth-worker')) {
        requests.push(`RESP ${resp.status()} ${resp.url()}`);
      }
    });
    page.on('console', msg => {
      if (msg.type() === 'error') console.log('BROWSER ERROR:', msg.text());
    });

    const createBtn = page.getByText('Создать аккаунт');
    await expect(createBtn).toBeVisible({ timeout: 15_000 });

    await createBtn.click();

    const result = await Promise.race([
      page.getByText('Дневник').waitFor({ timeout: 20_000 }).then(() => 'app'),
      page.locator('.notification.is-danger').waitFor({ timeout: 20_000 }).then(() => 'error'),
    ]).catch(() => 'timeout');

    console.log('Network log:', requests);

    if (result === 'app') {
      const userId = await page.evaluate(() => localStorage.getItem('user_id'));
      expect(userId).toBeTruthy();
      expect(userId!.length).toBeGreaterThan(0);
    } else {
      const errorText = result === 'error'
        ? await page.locator('.notification.is-danger').textContent()
        : 'timeout';
      console.error(`Auth failed: ${errorText}`);
      expect.soft(result).toBe('app');
    }
  });
});
