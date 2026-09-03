import { type Page, type CDPSession, expect } from '@playwright/test';

/**
 * The deployed frontend doesn't send user_id in /register/finish body,
 * but the refactored auth-worker requires it. This route intercept
 * patches the request to include user_id extracted from the prior
 * /register/begin response.
 *
 * Also patches /authenticate/begin to ensure empty body is fine.
 */
let lastUserId = '';

export async function patchRegisterFinish(page: Page) {
  // Intercept /register/begin to capture user_id
  await page.route('**/register/begin', async (route) => {
    const response = await route.fetch();
    const body = await response.json();
    if (body.user_id) {
      lastUserId = body.user_id;
    }
    await route.fulfill({ response });
  });

  // Intercept /register/finish to inject user_id
  await page.route('**/register/finish', async (route) => {
    const request = route.request();
    const postData = request.postDataJSON();
    if (!postData.user_id && lastUserId) {
      postData.user_id = lastUserId;
    }
    await route.continue({
      postData: JSON.stringify(postData),
    });
  });
}

/** Read the test env's `payment_base_url` from the served frontend config. */
export async function paymentBaseUrl(page: Page): Promise<string> {
  const toml = await page.evaluate(async () => {
    const r = await fetch('/config/frontend.toml');
    return r.text();
  });
  const m = toml.match(/^\s*payment_base_url\s*=\s*"([^"]+)"/m);
  if (!m) throw new Error('payment_base_url not found in /config/frontend.toml');
  return m[1];
}

/**
 * Mint a deterministically-paid GUEST claim via the payment-worker's
 * PRODUCTION-IMPOSSIBLE test-entitlement path (`POST /test/guest-checkout`,
 * gated by TEST_ENTITLEMENT — absent in prod, where it 404s). No real money.
 * Returns the `{claimId, secret}` that go in the `#claim=claimId.secret` fragment.
 */
export async function mintTestClaim(page: Page, planId = 'monthly'): Promise<{ claimId: string; secret: string }> {
  const base = await paymentBaseUrl(page);
  const res = await page.evaluate(async ({ base, planId }) => {
    const r = await fetch(`${base}/test/guest-checkout`, {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({ planId }),
    });
    return { status: r.status, body: await r.text() };
  }, { base, planId });
  if (res.status !== 200) {
    throw new Error(`test/guest-checkout failed: HTTP ${res.status}: ${res.body}`);
  }
  const json = JSON.parse(res.body);
  return { claimId: json.claimId, secret: json.secret };
}

/**
 * Set up virtual authenticator, dismiss PWA, then run the FULL paid-onboarding:
 * mint a test claim → /onboard#claim=… → register (name+passkey) → auto-claim →
 * land in the app. Since the trial is gone, this is how e2e gets a *usable*
 * (subscription-active) account.
 * Returns user_id and cdpSession.
 */
export async function registerAccount(page: Page): Promise<{ cdpSession: CDPSession; userId: string }> {
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

  // Patch the register/finish request
  await patchRegisterFinish(page);

  // Mint the paid guest claim first (needs the page origin for the config fetch).
  await page.evaluate(() => localStorage.setItem('pwa_dismissed', 'true'));
  const { claimId, secret } = await mintTestClaim(page);

  // Drive the onboarding entry with the claim in the URL fragment.
  await page.goto(`/onboard#claim=${claimId}.${secret}`);
  await page.waitForTimeout(2000);

  const createBtn = page.getByTestId('onboard-btn-register');
  await expect(createBtn).toBeVisible({ timeout: 15_000 });

  const nameInput = page.getByTestId('onboard-input-name');
  await nameInput.fill('Test User');
  await expect(createBtn).toBeEnabled({ timeout: 2_000 });
  await createBtn.click();

  // Wait for registration complete
  let userId = '';
  for (let i = 0; i < 40; i++) {
    const uid = await page.evaluate(() => localStorage.getItem('user_id'));
    if (uid) { userId = uid; break; }
    await page.waitForTimeout(500);
  }
  expect(userId).toBeTruthy();

  // Auto-claim (test row is already paid) → success → app. The onboard page
  // navigates to "/" on success; push onboarding may appear there.
  await page.waitForTimeout(2000);
  const skipBtn = page.getByTestId('push-onboarding-btn-skip');
  const navDiary = page.getByTestId('nav-diary');
  const visible = await skipBtn.isVisible({ timeout: 5000 }).catch(() => false);
  if (visible) {
    await skipBtn.click();
  }
  await expect(navDiary).toBeVisible({ timeout: 15_000 });

  return { cdpSession, userId };
}

/**
 * Push rows into the app's database and reload so the app picks them up.
 *
 * Do NOT open IndexedDB from the test: migrations run at startup, walk every
 * store and rewrite rows to the current schema, and whatever the test wrote is
 * gone before the first render. That is what `diary-search.spec.ts` still tries
 * to do — it even hardcodes `DB_VERSION = 12`, which the app left behind long
 * ago (it is 24 now), and opening an existing database with a LOWER version
 * fails outright.
 *
 * Instead the rows go into localStorage and the APP lays them out itself, right
 * after migrations (`frontend/src/services/test_seed.rs`). Two consequences
 * worth knowing:
 *
 *   - the test no longer needs to know the schema version at all;
 *   - the seed is CONSUMED. Deleting a row and reloading leaves it deleted,
 *     which is what a delete test needs. Call this again to seed again.
 *
 * Works only on the test host the app hardcodes; on production the key is
 * ignored.
 *
 * @param page   Playwright page, already registered (the per-user database must exist).
 * @param data   Store name → rows, e.g. `{ foods: [...], diary: [...] }`.
 */
export async function seedDatabase(
  page: Page,
  data: Record<string, unknown[]>,
): Promise<void> {
  await page.evaluate(
    ({ key, payload }) => localStorage.setItem(key, JSON.stringify(payload)),
    { key: 'ft_test_seed', payload: data },
  );
  await page.reload();
}

/** Turn a curator-gated feature on for this account by seeding its app flag. */
export async function seedFeature(page: Page, feature: string): Promise<void> {
  await seedDatabase(page, {
    app_flags: [{ key: `feature.${feature}`, value: 'true', updated_at: new Date().toISOString() }],
  });
}
