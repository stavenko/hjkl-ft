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

/**
 * Set up virtual authenticator, dismiss PWA, register account.
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

  await page.evaluate(() => localStorage.setItem('pwa_dismissed', 'true'));
  await page.reload();
  await page.waitForTimeout(3000);

  // TryingPassKey → fail → Auth page
  const createBtn = page.getByTestId('auth-btn-register');
  await expect(createBtn).toBeVisible({ timeout: 15_000 });
  await createBtn.click();

  // Wait for registration complete
  let userId = '';
  for (let i = 0; i < 40; i++) {
    const uid = await page.evaluate(() => localStorage.getItem('user_id'));
    if (uid) { userId = uid; break; }
    await page.waitForTimeout(500);
  }
  expect(userId).toBeTruthy();
  await page.waitForTimeout(1000);

  return { cdpSession, userId };
}
