import { test, expect, type CDPSession, type Page } from '@playwright/test';
import { patchRegisterFinish, mintTestClaim, paymentBaseUrl } from './helpers';

/**
 * /onboard claim flow — the ONLY place registration happens.
 *
 * These specs only pass against the TEST worker (TEST_ENTITLEMENT=1). In prod the
 * /test/* path 404s, so there is no free-subscription backdoor.
 */

async function addAuthenticator(page: Page): Promise<CDPSession> {
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
  return cdpSession;
}

async function registerAndClaim(page: Page, claimId: string, secret: string, name: string) {
  await page.goto(`/onboard#claim=${claimId}.${secret}`);
  await page.waitForTimeout(2000);
  const createBtn = page.getByTestId('onboard-btn-register');
  await expect(createBtn).toBeVisible({ timeout: 15_000 });
  await page.getByTestId('onboard-input-name').fill(name);
  await expect(createBtn).toBeEnabled({ timeout: 2_000 });
  await createBtn.click();
  for (let i = 0; i < 40; i++) {
    const uid = await page.evaluate(() => localStorage.getItem('user_id'));
    if (uid) break;
    await page.waitForTimeout(500);
  }
}

test.beforeEach(async ({ page }) => {
  await page.goto('/');
  await page.evaluate(() => localStorage.clear());
  await page.evaluate(() => localStorage.setItem('pwa_dismissed', 'true'));
});

test.describe('/onboard claim flow', () => {
  let cdpSession: CDPSession;

  test.afterEach(async () => {
    if (cdpSession) {
      await cdpSession.send('WebAuthn.disable').catch(() => {});
    }
  });

  test('register → claim a paid sub → enters the app', async ({ page }) => {
    cdpSession = await addAuthenticator(page);
    await patchRegisterFinish(page);

    const { claimId, secret } = await mintTestClaim(page);
    await registerAndClaim(page, claimId, secret, 'Claimer One');

    // Success → onboard navigates to "/"; clear push onboarding if present.
    await page.waitForTimeout(2000);
    const skipBtn = page.getByTestId('push-onboarding-btn-skip');
    if (await skipBtn.isVisible({ timeout: 5000 }).catch(() => false)) {
      await skipBtn.click();
    }
    await expect(page.getByTestId('nav-diary')).toBeVisible({ timeout: 15_000 });
  });

  test('a second account cannot claim the same paid sub (MONEY-SAFETY #3: 403)', async ({ page }) => {
    // Account A claims the sub.
    cdpSession = await addAuthenticator(page);
    await patchRegisterFinish(page);
    const { claimId, secret } = await mintTestClaim(page);
    await registerAndClaim(page, claimId, secret, 'Owner');

    const ownerId = await page.evaluate(() => localStorage.getItem('user_id'));
    expect(ownerId).toBeTruthy();

    // Wipe the session so a DIFFERENT account is created, and reset the
    // authenticator so a fresh passkey/user is minted.
    await cdpSession.send('WebAuthn.disable').catch(() => {});
    await page.evaluate(() => localStorage.clear());
    await page.evaluate(() => localStorage.setItem('pwa_dismissed', 'true'));
    cdpSession = await addAuthenticator(page);
    await patchRegisterFinish(page);

    // Account B registers and attempts to claim the ALREADY-CLAIMED sub.
    await registerAndClaim(page, claimId, secret, 'Intruder');

    const intruderId = await page.evaluate(() => localStorage.getItem('user_id'));
    expect(intruderId).toBeTruthy();
    expect(intruderId).not.toBe(ownerId);

    // Hard reject → terminal error screen, never enters the app.
    await expect(page.getByTestId('onboard-error')).toBeVisible({ timeout: 15_000 });
    await expect(page.getByTestId('nav-diary')).not.toBeVisible();
  });

  test('same account re-claiming its own sub is idempotent (success)', async ({ page }) => {
    cdpSession = await addAuthenticator(page);
    await patchRegisterFinish(page);
    const { claimId, secret } = await mintTestClaim(page);
    await registerAndClaim(page, claimId, secret, 'Repeat Claimer');

    const userId = await page.evaluate(() => localStorage.getItem('user_id'));
    expect(userId).toBeTruthy();

    // Re-issue the claim against the SAME (still signed-in) account → idempotent.
    const base = await paymentBaseUrl(page);
    const res = await page.evaluate(async ({ base, claimId, secret }) => {
      const token = localStorage.getItem('auth_token');
      const r = await fetch(`${base}/claim`, {
        method: 'POST',
        headers: { 'Content-Type': 'application/json', Authorization: `Bearer ${token}` },
        body: JSON.stringify({ claimId, secret }),
      });
      return { status: r.status, body: await r.text() };
    }, { base, claimId, secret });

    expect(res.status).toBe(200);
    expect(res.body).toContain('"active":true');
  });

  test('unpaid claim shows the pending/retry state, never the app', async ({ page }) => {
    cdpSession = await addAuthenticator(page);
    await patchRegisterFinish(page);

    // A syntactically-valid-but-unknown claim id/secret never confirms paid →
    // server returns 404/409; the onboard page must NOT drop into the app.
    const fakeClaimId = 'deadbeefdeadbeefdeadbeefdeadbeef';
    const fakeSecret = 'cafebabecafebabecafebabecafebabecafebabecafebabe';
    await registerAndClaim(page, fakeClaimId, fakeSecret, 'No Pay');

    // Either pending (retry) or terminal error — but NEVER the app.
    const pending = page.getByTestId('onboard-claiming');
    const errorScreen = page.getByTestId('onboard-error');
    await expect(pending.or(errorScreen)).toBeVisible({ timeout: 15_000 });
    await expect(page.getByTestId('nav-diary')).not.toBeVisible();
  });
});
