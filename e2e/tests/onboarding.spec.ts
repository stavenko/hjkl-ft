import { test, expect, type CDPSession } from '@playwright/test';
import { patchRegisterFinish } from './helpers';

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

    const dismissBtn = page.getByTestId('pwa-btn-dismiss');
    await expect(dismissBtn).toBeVisible();

    await expect(page.getByTestId('auth-btn-register')).not.toBeVisible();
  });

  test('dismiss PWA prompt → tries PassKey then shows auth page', async ({ page }) => {
    const dismissBtn = page.getByTestId('pwa-btn-dismiss');
    await expect(dismissBtn).toBeVisible({ timeout: 10_000 });

    await dismissBtn.click();

    // After dismiss: TryingPassKey (brief loading) → Auth page (no PassKey found)
    const createBtn = page.getByTestId('auth-btn-register');
    await expect(createBtn).toBeVisible({ timeout: 15_000 });

    const dismissed = await page.evaluate(() => localStorage.getItem('pwa_dismissed'));
    expect(dismissed).toBe('true');
  });

  test('PWA prompt stays dismissed after reload', async ({ page }) => {
    const dismissBtn = page.getByTestId('pwa-btn-dismiss');
    await expect(dismissBtn).toBeVisible({ timeout: 10_000 });
    await dismissBtn.click();

    await page.reload();
    await page.waitForTimeout(3000);

    // Should skip PWA prompt, try PassKey (fail), then show auth
    const createBtn = page.getByTestId('auth-btn-register');
    await expect(createBtn).toBeVisible({ timeout: 15_000 });
  });
});

test.describe('Account creation with PassKey', () => {
  let cdpSession: CDPSession;

  test.beforeEach(async ({ page }) => {
    // Set up virtual authenticator BEFORE reload so TryingPassKey sees it
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
  });

  test.afterEach(async () => {
    if (cdpSession) {
      await cdpSession.send('WebAuthn.disable').catch(() => {});
    }
  });

  test('auth page shows register and login options', async ({ page }) => {
    const createBtn = page.getByTestId('auth-btn-register');
    await expect(createBtn).toBeVisible({ timeout: 15_000 });

    const alreadyUsed = page.getByText('Я уже пользовался этим приложением');
    await expect(alreadyUsed).toBeVisible({ timeout: 5_000 });

    const loginBtn = page.getByTestId('auth-btn-login');
    await expect(loginBtn).toBeVisible({ timeout: 5_000 });
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

    const createBtn = page.getByTestId('auth-btn-register');
    await expect(createBtn).toBeVisible({ timeout: 15_000 });

    // Fill in display name (required for registration)
    const nameInput = page.getByTestId('auth-input-name');
    await nameInput.fill('Test User');
    await expect(createBtn).toBeEnabled({ timeout: 2_000 });

    await createBtn.click();

    // Wait for registration to complete: either user_id in localStorage or error
    const result = await Promise.race([
      (async () => {
        for (let i = 0; i < 40; i++) {
          const uid = await page.evaluate(() => localStorage.getItem('user_id'));
          if (uid) return 'app';
          await page.waitForTimeout(500);
        }
        return 'timeout';
      })(),
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

  test('registration requires display name', async ({ page }) => {
    const createBtn = page.getByTestId('auth-btn-register');
    await expect(createBtn).toBeVisible({ timeout: 15_000 });

    // Register button should be disabled when name is empty
    await expect(createBtn).toBeDisabled();

    // Fill in display name
    const nameInput = page.getByTestId('auth-input-name');
    await nameInput.fill('Test User');

    // Register button should be enabled when name is filled
    await expect(createBtn).toBeEnabled({ timeout: 2_000 });

    // Clear name — button should be disabled again
    await nameInput.fill('');
    await expect(createBtn).toBeDisabled();

    // Whitespace-only name should also keep button disabled
    await nameInput.fill('   ');
    await expect(createBtn).toBeDisabled();
  });

  test('app loads with expired token without crashing', async ({ page }) => {
    // -- Step 1: Register account --
    const createBtn = page.getByTestId('auth-btn-register');
    await expect(createBtn).toBeVisible({ timeout: 15_000 });

    // Fill in display name (required for registration)
    const nameInput = page.getByTestId('auth-input-name');
    await nameInput.fill('Test User');
    await expect(createBtn).toBeEnabled({ timeout: 2_000 });

    await createBtn.click();

    // Wait for registration complete
    for (let i = 0; i < 40; i++) {
      const uid = await page.evaluate(() => localStorage.getItem('user_id'));
      if (uid) break;
      await page.waitForTimeout(500);
    }
    const userId = await page.evaluate(() => localStorage.getItem('user_id'));
    expect(userId).toBeTruthy();
    await page.waitForTimeout(1000);

    // Verify we have a valid token
    const tokenBefore = await page.evaluate(() => localStorage.getItem('auth_token'));
    expect(tokenBefore).toBeTruthy();

    // -- Step 2: Manually expire token and reload --
    await page.evaluate(() => {
      const pastTimestamp = Math.floor(Date.now() / 1000) - 1;
      localStorage.setItem('token_expires_at', pastTimestamp.toString());
    });

    await patchRegisterFinish(page);
    await page.reload();
    await page.waitForTimeout(2000);

    // -- Step 3: App should load to Ready state (user_id is present) --
    // The app skips auth overlay when user_id exists, even with expired token.
    // Navigation should be accessible.
    const navRecipes = page.getByTestId('nav-recipes');
    await expect(navRecipes).toBeVisible({ timeout: 10_000 });

    // user_id should still be in localStorage
    const userIdAfter = await page.evaluate(() => localStorage.getItem('user_id'));
    expect(userIdAfter).toBe(userId);

    // -- Step 4: Verify no JsValue errors shown to user --
    const pageContent = await page.textContent('body');
    expect(pageContent).not.toContain('JsValue(');
    expect(pageContent).not.toContain('TypeError:');
    expect(pageContent).not.toContain('RustError');
  });
});
