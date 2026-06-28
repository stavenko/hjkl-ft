// Real-passkey check for the admin console on its DEPLOYED domain — proves the
// auth-worker's origin-aware RP config works end-to-end: a WebAuthn ceremony
// initiated from https://renorma-admin.pages.dev must pass the server's rp.id +
// origin verification (which is now selected by the request Origin).
//
// We drive a virtual authenticator and assert /register/finish returns 200 (the
// server-side rp_id-hash + origin equality both passed). Authenticate is exercised
// too. The registered expert's sub is random and NOT in EXPERT_IDS, so the queue
// gate (403) is expected after login — that is NOT what this check asserts.
//
// Run: node scripts/admin-passkey-check.mjs

import pw from '../e2e/node_modules/@playwright/test/index.js';
const { chromium } = pw;

const ADMIN_URL = process.env.ADMIN_URL ?? 'https://renorma-admin.pages.dev/';

function assert(cond, msg) {
  if (!cond) throw new Error(`ASSERT FAILED: ${msg}`);
}

async function main() {
  const browser = await chromium.launch();
  const ctx = await browser.newContext();
  const page = await ctx.newPage();

  // Virtual platform authenticator (resident key + UV, auto-present).
  const cdp = await ctx.newCDPSession(page);
  await cdp.send('WebAuthn.enable');
  await cdp.send('WebAuthn.addVirtualAuthenticator', {
    options: {
      protocol: 'ctap2',
      transport: 'internal',
      hasResidentKey: true,
      hasUserVerification: true,
      isUserVerified: true,
      automaticPresenceSimulation: true,
    },
  });

  // Capture the server's verdict on the WebAuthn ceremonies.
  const finishes = {};
  page.on('response', (r) => {
    const u = r.url();
    if (u.includes('/register/finish')) finishes.register = r.status();
    if (u.includes('/authenticate/finish')) finishes.authenticate = r.status();
  });
  const consoleErrors = [];
  page.on('console', (m) => { if (m.type() === 'error') consoleErrors.push(m.text()); });

  await page.goto(ADMIN_URL);
  await page.getByText('Renorma — поддержка').waitFor({ state: 'visible', timeout: 15_000 });

  // ── Register a new expert passkey (origin-aware RP must accept the admin origin) ──
  await page.getByText('Первый вход на этом устройстве').click();
  await page.getByPlaceholder('Имя эксперта').fill('E2E Expert');
  await page.getByRole('button', { name: 'Создать' }).click();

  // The app prints the new sub + "add to EXPERT_IDS" only after /register/finish succeeds.
  await page.getByText(/EXPERT_IDS/).waitFor({ state: 'visible', timeout: 20_000 });
  assert(finishes.register === 200, `register/finish must be 200 (got ${finishes.register}) — origin-aware RP rejected the admin origin`);

  // ── Authenticate with that passkey (same origin-aware config selection) ──
  await page.getByRole('button', { name: 'Войти паскеем' }).click();
  // Wait for the server to answer the assertion (status is set by the response listener).
  await page.waitForFunction(() => true, { timeout: 1 }).catch(() => {});
  await page.waitForTimeout(6000);
  assert(finishes.authenticate === 200, `authenticate/finish must be 200 (got ${finishes.authenticate}) — origin/rp.id verification failed`);

  // No SecurityError (browser-side rp.id rejection) should have surfaced.
  const security = consoleErrors.filter((e) => /SecurityError|relying party|rp id|rpId/i.test(e));
  assert(security.length === 0, `WebAuthn SecurityError(s): ${security.join(' | ')}`);

  await browser.close();
  console.log(`✅ admin passkey check PASSED — register/finish=200, authenticate/finish=200 on ${ADMIN_URL}`);
}

main().catch((e) => {
  console.error('❌ admin passkey check FAILED:', e.message);
  process.exit(1);
});
