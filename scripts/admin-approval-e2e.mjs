// Full UI approval flow for the admin console, end-to-end against the deployed
// admin + live support-worker. Exercises the REAL path (no JWT injection):
//   real passkey register + sign-in  →  RequestAccess screen  →  request a code
//   →  operator approves the code with X-Admin-Secret  →  Проверить доступ
//   →  Queue shows a seeded pending conversation  →  open thread  →  reply.
//
// Run: node scripts/admin-approval-e2e.mjs
//   ADMIN_URL (default https://renorma-admin.pages.dev/)
//   SUPPORT_BASE_URL, SUPPORT_JWT_SECRET, ADMIN_APPROVE_SECRET (dev defaults)

import pw from '../e2e/node_modules/@playwright/test/index.js';
import { createHmac, randomUUID } from 'node:crypto';
const { chromium } = pw;

const ADMIN_URL = process.env.ADMIN_URL ?? 'https://renorma-admin.pages.dev/';
const SUPPORT = process.env.SUPPORT_BASE_URL ?? 'https://support-worker.vg-stavenko.workers.dev';
const JWT_SECRET = process.env.SUPPORT_JWT_SECRET ?? 'dev-secret-change-in-production';
const APPROVE_SECRET = process.env.ADMIN_APPROVE_SECRET ?? 'dev-admin-approve-secret';

const b64 = (b) => Buffer.from(b).toString('base64url');
function mintUser(sub) {
  const h = b64(JSON.stringify({ alg: 'HS256', typ: 'JWT' }));
  const c = b64(JSON.stringify({ sub, iat: 0, exp: 4_102_444_800, caps: [], token_id: 'e2e' }));
  const si = `${h}.${c}`;
  return `${si}.${b64(createHmac('sha256', JWT_SECRET).update(si).digest())}`;
}
function assert(cond, msg) { if (!cond) throw new Error(`ASSERT FAILED: ${msg}`); }

async function main() {
  const browser = await chromium.launch();
  const ctx = await browser.newContext();
  const page = await ctx.newPage();

  const cdp = await ctx.newCDPSession(page);
  await cdp.send('WebAuthn.enable');
  await cdp.send('WebAuthn.addVirtualAuthenticator', {
    options: { protocol: 'ctap2', transport: 'internal', hasResidentKey: true, hasUserVerification: true, isUserVerified: true, automaticPresenceSimulation: true },
  });

  await page.goto(ADMIN_URL);
  await page.getByText('Renorma — поддержка').waitFor({ state: 'visible', timeout: 15_000 });

  // ── 1. Register a passkey (first-time), then sign in ──
  await page.getByText('Первый вход на этом устройстве').click();
  await page.getByPlaceholder('Имя эксперта').fill('E2E Expert');
  await page.getByRole('button', { name: 'Создать' }).click();
  await page.getByText(/Войдите паскеем/).waitFor({ state: 'visible', timeout: 20_000 });

  await page.getByRole('button', { name: 'Войти паскеем' }).click();

  // ── 2. RequestAccess: candidate is not approved → request a code ──
  const requestBtn = page.getByRole('button', { name: 'Запросить доступ' });
  await requestBtn.waitFor({ state: 'visible', timeout: 20_000 });
  await requestBtn.click();

  const codeEl = page.locator('code');
  await codeEl.waitFor({ state: 'visible', timeout: 15_000 });
  const code = (await codeEl.innerText()).trim();
  assert(/^[A-Z0-9]{6,}$/.test(code), `code looks wrong: "${code}"`);

  // ── 3. Operator approves the code with the secret (the real /admin/approve path) ──
  const approve = await page.request.post(`${SUPPORT}/admin/approve`, {
    headers: { 'X-Admin-Secret': APPROVE_SECRET, 'Content-Type': 'application/json' },
    data: { code },
  });
  assert(approve.ok(), `approve failed: ${approve.status()} ${await approve.text()}`);

  // ── 4. Seed a pending conversation as a user (so the Queue has something) ──
  const userSub = `e2e-appr-user-${Date.now()}`;
  const userMsg = `approval e2e question ${Date.now()}`;
  const seed = await page.request.post(`${SUPPORT}/message`, {
    headers: { Authorization: `Bearer ${mintUser(userSub)}`, 'Content-Type': 'application/json' },
    data: { client_id: randomUUID(), text: userMsg },
  });
  assert(seed.ok(), `seed /message failed: ${seed.status()}`);

  // ── 5. Проверить доступ → now approved → Queue ──
  await page.getByRole('button', { name: 'Проверить доступ' }).click();
  const row = page.getByTestId('conv').filter({ hasText: userMsg });
  await row.waitFor({ state: 'visible', timeout: 20_000 });

  // ── 6. Open the thread, reply, confirm the expert bubble ──
  await row.click();
  await page.getByTestId('msg').filter({ hasText: userMsg }).waitFor({ state: 'visible', timeout: 10_000 });
  const replyMsg = `approval e2e answer ${Date.now()}`;
  await page.getByTestId('reply-input').fill(replyMsg);
  await page.getByTestId('reply-send').click();
  const replyBubble = page.getByTestId('msg').filter({ hasText: replyMsg });
  await replyBubble.waitFor({ state: 'visible', timeout: 10_000 });
  assert((await replyBubble.getAttribute('data-sender')) === 'expert', 'reply must be sender=expert');

  // ── 7. The reply reached the user side ──
  const poll = await page.request.get(`${SUPPORT}/messages?after_seq=0&limit=100`, {
    headers: { Authorization: `Bearer ${mintUser(userSub)}` },
  });
  const { messages } = await poll.json();
  assert(messages.some((m) => m.sender === 'expert' && m.text === replyMsg), 'expert reply must be on the user side');

  await browser.close();
  console.log(`✅ admin approval e2e PASSED — passkey → request(${code}) → approve → queue → reply (${ADMIN_URL})`);
}

main().catch((e) => { console.error('❌ admin approval e2e FAILED:', e.message); process.exit(1); });
