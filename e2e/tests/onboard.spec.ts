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
  // Force a fresh document load of /onboard. Without this, if the page is already
  // sitting on the same /onboard#claim=… URL (e.g. a prior account's flow), a goto
  // to the identical URL is treated as a same-document hash nav and does NOT reboot
  // the WASM app — so OnboardPage never re-mounts and the register form never shows.
  await page.goto('about:blank');
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

  test('a second account cannot claim an already-claimed sub (MONEY-SAFETY #3: 403)', async ({ page }) => {
    // Account A claims its sub and lands in the app.
    cdpSession = await addAuthenticator(page);
    await patchRegisterFinish(page);
    const a = await mintTestClaim(page);
    await registerAndClaim(page, a.claimId, a.secret, 'Owner');

    const ownerId = await page.evaluate(() => localStorage.getItem('user_id'));
    expect(ownerId).toBeTruthy();
    const skipBtnA = page.getByTestId('push-onboarding-btn-skip');
    if (await skipBtnA.isVisible({ timeout: 5000 }).catch(() => false)) {
      await skipBtnA.click();
    }
    await expect(page.getByTestId('nav-diary')).toBeVisible({ timeout: 15_000 });

    // A DIFFERENT account B, with its OWN paid claim and a fresh authenticator. (Since
    // F-1, B can't even register against A's claim — the pre-check blocks it — so B is a
    // real, separately-onboarded account, which is exactly the attack we must reject.)
    await cdpSession.send('WebAuthn.disable').catch(() => {});
    await page.evaluate(() => localStorage.clear());
    await page.evaluate(() => localStorage.setItem('pwa_dismissed', 'true'));
    cdpSession = await addAuthenticator(page);
    await patchRegisterFinish(page);
    const b = await mintTestClaim(page);
    await registerAndClaim(page, b.claimId, b.secret, 'Other');

    const otherId = await page.evaluate(() => localStorage.getItem('user_id'));
    expect(otherId).toBeTruthy();
    expect(otherId).not.toBe(ownerId);

    // B (authenticated) attempts to claim A's already-bound sub directly against the API.
    // The atomic CAS in ClaimDO must hard-reject it: one sub = one account (403
    // claimed_by_other). This is the money-safety invariant, tested at its true seam.
    const base = await paymentBaseUrl(page);
    const res = await page.evaluate(async ({ base, claimId, secret }) => {
      const token = localStorage.getItem('auth_token');
      const r = await fetch(`${base}/claim`, {
        method: 'POST',
        headers: { 'Content-Type': 'application/json', Authorization: `Bearer ${token}` },
        body: JSON.stringify({ claimId, secret }),
      });
      return { status: r.status, body: await r.text() };
    }, { base, claimId: a.claimId, secret: a.secret });

    expect(res.status).toBe(403);
    expect(res.body).toContain('claimed_by_other');
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

  test('an unbindable claim is blocked before registration — no account, never the app', async ({ page }) => {
    cdpSession = await addAuthenticator(page);
    await patchRegisterFinish(page);

    // A syntactically-valid-but-unknown claim id/secret resolves to status "none". The
    // onboard PRE-CHECK (F-1) must refuse to register: registration runs before the
    // claim binds, so an unbindable claim would otherwise leave an orphan account. The
    // guarantee: no account is created, and the app is never entered.
    const fakeClaimId = 'deadbeefdeadbeefdeadbeefdeadbeef';
    const fakeSecret = 'cafebabecafebabecafebabecafebabecafebabecafebabe';
    await registerAndClaim(page, fakeClaimId, fakeSecret, 'No Pay');

    // Held on the onboard register screen with an inline error; NO session established
    // (the whole point of F-1 — no orphan account).
    await expect(page.locator('.notification.is-danger')).toBeVisible({ timeout: 15_000 });
    await expect(page.getByTestId('onboard-btn-register')).toBeVisible();
    expect(await page.evaluate(() => localStorage.getItem('user_id'))).toBeNull();

    // Never the app: no success state, no in-app PAGE content, still on /onboard.
    await expect(page.getByTestId('onboard-success')).toHaveCount(0);
    await expect(page.getByTestId('diary-btn-prev-date')).toHaveCount(0);
    await expect(page.getByTestId('diary-btn-add')).toHaveCount(0);
    expect(await page.evaluate(() => location.pathname)).toBe('/onboard');
  });
});
