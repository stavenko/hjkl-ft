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

  test('a second account cannot claim the same paid sub (MONEY-SAFETY #3: 403)', async ({ page }) => {
    // Account A claims the sub, and we wait for it to actually land in the app so
    // the onboard page's success navigation ("/" via set_href) has fully settled
    // before we wipe the session. Otherwise account A's in-flight claim races with
    // account B's navigation (same /onboard#claim URL → no reload).
    cdpSession = await addAuthenticator(page);
    await patchRegisterFinish(page);
    const { claimId, secret } = await mintTestClaim(page);
    await registerAndClaim(page, claimId, secret, 'Owner');

    const ownerId = await page.evaluate(() => localStorage.getItem('user_id'));
    expect(ownerId).toBeTruthy();
    // Confirm A reached the app (nav-diary) so the /onboard→"/" nav is done.
    const skipBtnA = page.getByTestId('push-onboarding-btn-skip');
    if (await skipBtnA.isVisible({ timeout: 5000 }).catch(() => false)) {
      await skipBtnA.click();
    }
    await expect(page.getByTestId('nav-diary')).toBeVisible({ timeout: 15_000 });

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

    // Hard reject → terminal error screen, never enters the app. The claim was
    // already bound to account A, so account B gets a 403 → onboard-error, never
    // the success state, and the router stays on /onboard (no in-app page content).
    // (nav-diary is mounted app-wide behind the onboard screen by design — see the
    //  note in the unpaid-claim test — so we assert route+content, not nav.)
    await expect(page.getByTestId('onboard-error')).toBeVisible({ timeout: 15_000 });
    await expect(page.getByTestId('onboard-success')).toHaveCount(0);
    await expect(page.getByTestId('diary-btn-prev-date')).toHaveCount(0);
    expect(await page.evaluate(() => location.pathname)).toBe('/onboard');
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

    // The unpaid user is HELD on the /onboard claim screen: the success state
    // ("entering the app") is never reached, and no in-app PAGE content renders.
    // (The bottom nav is mounted app-wide whenever AppState==Ready, and the
    //  /onboard#claim entry bypasses the Locked/Auth overlays — so nav-diary is
    //  present in the DOM behind the onboard screen by design. The real "never
    //  the app" guarantee is that the router stays on /onboard / OnboardPage and
    //  no diary/story page content is shown.)
    await expect(page.getByTestId('onboard-success')).toHaveCount(0);
    await expect(page.getByTestId('diary-btn-prev-date')).toHaveCount(0);
    await expect(page.getByTestId('diary-btn-add')).toHaveCount(0);
    expect(await page.evaluate(() => location.pathname)).toBe('/onboard');
  });
});
