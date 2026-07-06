// Payments section UI smoke for the admin console, against the deployed admin +
// live Rust payment-worker. Verifies the operator can see the unbound-payments
// worklist and void a row (exercises CORS from the admin origin + require_admin).
//
// Auth is bypassed by injecting an expert-1 JWT (in EXPERT_IDS) — the queue/payments
// path is what's under test, not passkey.
//
// Run: node scripts/payments-ui-smoke.mjs

import pw from '../e2e/node_modules/@playwright/test/index.js';
import { createHmac } from 'node:crypto';
const { chromium } = pw;

const ADMIN_URL = process.env.ADMIN_URL ?? 'https://renorma-admin.pages.dev/';
const PAY = process.env.PAYMENT_BASE_URL ?? 'https://payment-worker.vg-stavenko.workers.dev';
const SECRET = process.env.SUPPORT_JWT_SECRET ?? 'dev-secret-change-in-production';

const b64 = (b) => Buffer.from(b).toString('base64url');
function mint(sub) {
  const h = b64(JSON.stringify({ alg: 'HS256', typ: 'JWT' }));
  const c = b64(JSON.stringify({ sub, iat: 0, exp: 4_102_444_800, caps: [], token_id: 'smoke' }));
  const si = `${h}.${c}`;
  return `${si}.${b64(createHmac('sha256', SECRET).update(si).digest())}`;
}
const expertJwt = mint('expert-1');
const userJwt = mint(`pay-ui-user-${Date.now()}`);
const AH = { Authorization: `Bearer ${expertJwt}`, 'Content-Type': 'application/json' };
function assert(c, m) { if (!c) throw new Error(`ASSERT FAILED: ${m}`); }

async function main() {
  // 0. Clean slate: void any existing unbound payments so the list is deterministic.
  let r = await fetch(`${PAY}/admin/unbound-payments`, { headers: AH });
  let { unbound } = await r.json();
  for (const p of unbound) {
    await fetch(`${PAY}/admin/void-payment`, { method: 'POST', headers: AH, body: JSON.stringify({ claimId: p.claim_id }) });
  }
  // 1. Seed exactly ONE unbound (paid, unclaimed) payment via the test path.
  r = await fetch(`${PAY}/test/guest-checkout`, {
    method: 'POST',
    headers: { Authorization: `Bearer ${userJwt}`, 'Content-Type': 'application/json' },
    body: JSON.stringify({ planId: 'monthly' }),
  });
  assert(r.ok, `seed failed: ${r.status}`);

  const browser = await chromium.launch();
  const ctx = await browser.newContext();
  const page = await ctx.newPage();
  await page.addInitScript((tok) => localStorage.setItem('auth_token', tok), expertJwt);
  const consoleErrors = [];
  page.on('console', (m) => { if (m.type() === 'error') consoleErrors.push(m.text()); });

  await page.goto(ADMIN_URL);
  // expert-1 is in EXPERT_IDS → /admin/me approved → Queue.
  await page.getByTestId('nav-payments').waitFor({ state: 'visible', timeout: 20_000 });

  // 2. Open the payments worklist → exactly one row (CORS from the admin origin + require_admin).
  await page.getByTestId('nav-payments').click();
  await page.getByTestId('payment-row').waitFor({ state: 'visible', timeout: 15_000 });
  assert((await page.getByTestId('payment-row').count()) === 1, 'expected exactly one unbound payment row');

  // 3. Void it → row gone, empty state shown.
  await page.getByTestId('payment-void').first().click();
  await page.getByText('Нет непривязанных платежей').waitFor({ state: 'visible', timeout: 15_000 });
  assert((await page.getByTestId('payment-row').count()) === 0, 'row should be gone after void');

  // 4. Confirm server-side it's voided.
  r = await fetch(`${PAY}/admin/unbound-payments`, { headers: AH });
  ({ unbound } = await r.json());
  assert(unbound.length === 0, 'server unbound list must be empty after void');

  // No CORS / fetch errors should have surfaced.
  const cors = consoleErrors.filter((e) => /CORS|Access-Control|Failed to fetch|blocked/i.test(e));
  assert(cors.length === 0, `CORS/fetch errors: ${cors.join(' | ')}`);

  await browser.close();
  console.log(`✅ payments UI smoke PASSED (${ADMIN_URL})`);
}

main().catch((e) => { console.error('❌ payments UI smoke FAILED:', e.message); process.exit(1); });
