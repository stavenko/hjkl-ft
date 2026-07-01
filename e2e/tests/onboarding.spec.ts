import { test, expect, type CDPSession } from '@playwright/test';
import { patchRegisterFinish, mintTestClaim } from './helpers';

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

    // After dismiss: TryingPassKey (brief loading) → Auth page. Account CREATION lives
    // in /onboard, so the auth screen shows the passkey login. A "Регистрация" link may
    // be present (it only routes to the paid landing) — but the inline account-creation
    // form (name input) must NOT be here.
    const loginBtn = page.getByTestId('auth-btn-try-passkey');
    await expect(loginBtn).toBeVisible({ timeout: 15_000 });
    await expect(page.getByTestId('auth-input-name')).not.toBeVisible();

    const dismissed = await page.evaluate(() => localStorage.getItem('pwa_dismissed'));
    expect(dismissed).toBe('true');
  });

  test('PWA prompt stays dismissed after reload', async ({ page }) => {
    const dismissBtn = page.getByTestId('pwa-btn-dismiss');
    await expect(dismissBtn).toBeVisible({ timeout: 10_000 });
    await dismissBtn.click();

    await page.reload();
    await page.waitForTimeout(3000);

    // Should skip PWA prompt, try PassKey (fail), then show the login-only auth.
    const loginBtn = page.getByTestId('auth-btn-try-passkey');
    await expect(loginBtn).toBeVisible({ timeout: 15_000 });
  });
});

test.describe('No-session "/" entry is login-only', () => {
  // The root with no session must show LOGIN only; creating an account happens
  // exclusively in the paid /onboard claim flow (registration is never at "/").
  test('auth page shows login options and NO register', async ({ page }) => {
    await page.evaluate(() => localStorage.setItem('pwa_dismissed', 'true'));
    await page.reload();
    await page.waitForTimeout(3000);

    const loginBtn = page.getByTestId('auth-btn-try-passkey');
    await expect(loginBtn).toBeVisible({ timeout: 15_000 });
    // Device-pairing is offered via the single "Добавить устройство" entry (the QR /
    // scan sub-screen opens from there).
    await expect(page.getByTestId('auth-btn-add-device')).toBeVisible({ timeout: 5_000 });

    // INLINE account creation must NOT appear at "/". (A "Регистрация" link is allowed —
    // it only routes to the paid landing — but there is no name-input form here.)
    await expect(page.getByTestId('auth-input-name')).not.toBeVisible();
  });
});

test.describe('Account creation via /onboard (paid claim)', () => {
  let cdpSession: CDPSession;

  test.beforeEach(async ({ page }) => {
    // Set up virtual authenticator BEFORE driving the onboard flow.
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

    await patchRegisterFinish(page);
    await page.evaluate(() => localStorage.setItem('pwa_dismissed', 'true'));
  });

  test.afterEach(async () => {
    if (cdpSession) {
      await cdpSession.send('WebAuthn.disable').catch(() => {});
    }
  });

  // Drive to /onboard with a freshly-minted (deterministically-paid) test claim.
  async function gotoOnboard(page: import('@playwright/test').Page) {
    const { claimId, secret } = await mintTestClaim(page);
    await page.goto(`/onboard#claim=${claimId}.${secret}`);
    await page.waitForTimeout(2000);
  }

  test('onboard shows the register form (name + create)', async ({ page }) => {
    await gotoOnboard(page);
    await expect(page.getByTestId('onboard-input-name')).toBeVisible({ timeout: 15_000 });
    await expect(page.getByTestId('onboard-btn-register')).toBeVisible({ timeout: 5_000 });
  });

  test('register + claim lands on the app', async ({ page }) => {
    page.on('console', msg => {
      if (msg.type() === 'error') console.log('BROWSER ERROR:', msg.text());
    });

    await gotoOnboard(page);

    const createBtn = page.getByTestId('onboard-btn-register');
    await expect(createBtn).toBeVisible({ timeout: 15_000 });

    const nameInput = page.getByTestId('onboard-input-name');
    await nameInput.fill('Test User');
    await expect(createBtn).toBeEnabled({ timeout: 2_000 });
    await createBtn.click();

    // Registration completes, then the page auto-claims (test row already paid).
    let userId = '';
    for (let i = 0; i < 40; i++) {
      const uid = await page.evaluate(() => localStorage.getItem('user_id'));
      if (uid) { userId = uid; break; }
      await page.waitForTimeout(500);
    }
    expect(userId).toBeTruthy();

    // After claim success, onboard navigates to "/"; push onboarding may appear.
    await page.waitForTimeout(2000);
    const skipBtn = page.getByTestId('push-onboarding-btn-skip');
    if (await skipBtn.isVisible({ timeout: 5000 }).catch(() => false)) {
      await skipBtn.click();
    }
    await expect(page.getByTestId('nav-diary')).toBeVisible({ timeout: 15_000 });
  });

  test('registration requires display name', async ({ page }) => {
    await gotoOnboard(page);

    const createBtn = page.getByTestId('onboard-btn-register');
    await expect(createBtn).toBeVisible({ timeout: 15_000 });

    // Disabled when name is empty.
    await expect(createBtn).toBeDisabled();

    const nameInput = page.getByTestId('onboard-input-name');
    await nameInput.fill('Test User');
    await expect(createBtn).toBeEnabled({ timeout: 2_000 });

    // Clear name → disabled again.
    await nameInput.fill('');
    await expect(createBtn).toBeDisabled();

    // Whitespace-only also keeps it disabled.
    await nameInput.fill('   ');
    await expect(createBtn).toBeDisabled();
  });

  test('after the claimed registration the app loads (no push-onboarding screen)', async ({ page }) => {
    await gotoOnboard(page);

    const createBtn = page.getByTestId('onboard-btn-register');
    await expect(createBtn).toBeVisible({ timeout: 15_000 });
    await page.getByTestId('onboard-input-name').fill('Test User');
    await createBtn.click();

    for (let i = 0; i < 40; i++) {
      const uid = await page.evaluate(() => localStorage.getItem('user_id'));
      if (uid) break;
      await page.waitForTimeout(500);
    }

    // Push onboarding is no longer a screen — register→claim goes straight to the
    // app. Enabling push now lives in the Story (chapter 1 setup section).
    await expect(page.getByTestId('nav-diary')).toBeVisible({ timeout: 15_000 });
    await expect(page.getByTestId('push-onboarding')).toHaveCount(0);

    // Survives a reload (session + claimed sub → Ready).
    await patchRegisterFinish(page);
    await page.goto('/');
    await expect(page.getByTestId('nav-diary')).toBeVisible({ timeout: 15_000 });
    await expect(page.getByTestId('push-onboarding')).toHaveCount(0);
  });

  test('app loads with expired token without crashing', async ({ page }) => {
    // -- Step 1: Register + claim an account via /onboard --
    await gotoOnboard(page);

    const createBtn = page.getByTestId('onboard-btn-register');
    await expect(createBtn).toBeVisible({ timeout: 15_000 });
    await page.getByTestId('onboard-input-name').fill('Test User');
    await expect(createBtn).toBeEnabled({ timeout: 2_000 });
    await createBtn.click();

    for (let i = 0; i < 40; i++) {
      const uid = await page.evaluate(() => localStorage.getItem('user_id'));
      if (uid) break;
      await page.waitForTimeout(500);
    }
    const userId = await page.evaluate(() => localStorage.getItem('user_id'));
    expect(userId).toBeTruthy();
    await page.waitForTimeout(1000);

    const tokenBefore = await page.evaluate(() => localStorage.getItem('auth_token'));
    expect(tokenBefore).toBeTruthy();

    // -- Step 2: Manually expire token and reload "/" --
    await page.evaluate(() => {
      const pastTimestamp = Math.floor(Date.now() / 1000) - 1;
      localStorage.setItem('token_expires_at', pastTimestamp.toString());
    });

    await patchRegisterFinish(page);
    await page.goto('/');
    await page.waitForTimeout(2000);

    // -- Step 3: App should load to Ready (user_id present) --
    const navRecipes = page.getByTestId('nav-recipes');
    await expect(navRecipes).toBeVisible({ timeout: 10_000 });

    const userIdAfter = await page.evaluate(() => localStorage.getItem('user_id'));
    expect(userIdAfter).toBe(userId);

    // -- Step 4: Verify no JsValue errors shown to user --
    const pageContent = await page.textContent('body');
    expect(pageContent).not.toContain('JsValue(');
    expect(pageContent).not.toContain('TypeError:');
    expect(pageContent).not.toContain('RustError');
  });
});
