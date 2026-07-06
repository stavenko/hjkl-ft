// Admin console (Phase 3) smoke — drives the built admin PWA against the LIVE
// support-worker: seed a pending conversation as a user, then as an expert see it
// in the queue, open the thread, reply, and confirm the reply reaches the user side.
//
// Auth is bypassed for the test (we inject an expert JWT into localStorage instead
// of doing passkey) — the queue/thread/reply server path is what's under test.
//
// Run: node scripts/admin-smoke.mjs   (from repo root; admin/dist must be built)

import { createHmac, randomUUID } from 'node:crypto';
import { createServer } from 'node:http';
import { readFile } from 'node:fs/promises';
import { extname, join, normalize } from 'node:path';
import pw from '../e2e/node_modules/@playwright/test/index.js';
const { chromium } = pw;

const SUPPORT = process.env.SUPPORT_BASE_URL ?? 'https://support-worker.vg-stavenko.workers.dev';
const SECRET = process.env.SUPPORT_JWT_SECRET ?? 'dev-secret-change-in-production';
const EXPERT_ID = process.env.SUPPORT_EXPERT_ID ?? 'expert-1';
const DIST = new URL('../admin/dist/', import.meta.url).pathname;
const PORT = 3701;

const b64 = (b) => Buffer.from(b).toString('base64url');
function mint(sub) {
  const h = b64(JSON.stringify({ alg: 'HS256', typ: 'JWT' }));
  const c = b64(JSON.stringify({ sub, iat: 0, exp: 4_102_444_800, caps: [], token_id: 'smoke' }));
  const si = `${h}.${c}`;
  return `${si}.${b64(createHmac('sha256', SECRET).update(si).digest())}`;
}

const MIME = {
  '.html': 'text/html', '.js': 'text/javascript', '.wasm': 'application/wasm',
  '.toml': 'text/plain', '.css': 'text/css', '.json': 'application/json',
};

function staticServer() {
  return createServer(async (req, res) => {
    try {
      let p = decodeURIComponent(req.url.split('?')[0]);
      if (p === '/') p = '/index.html';
      const file = normalize(join(DIST, p));
      if (!file.startsWith(DIST)) { res.writeHead(403).end(); return; }
      const body = await readFile(file);
      res.writeHead(200, { 'Content-Type': MIME[extname(file)] ?? 'application/octet-stream' });
      res.end(body);
    } catch {
      res.writeHead(404).end('not found');
    }
  });
}

function assert(cond, msg) {
  if (!cond) throw new Error(`ASSERT FAILED: ${msg}`);
}

async function main() {
  // Against a deployed URL (ADMIN_URL) we skip the local static server.
  const adminUrl = process.env.ADMIN_URL;
  let server = null;
  if (!adminUrl) {
    server = staticServer();
    await new Promise((r) => server.listen(PORT, r));
  }
  const baseUrl = adminUrl ?? `http://localhost:${PORT}/`;

  const userSub = `smoke-user-${Date.now()}`;
  const userJwt = mint(userSub);
  const expertJwt = mint(EXPERT_ID);
  const userMsg = `smoke user question ${Date.now()}`;
  const replyMsg = `smoke expert answer ${Date.now()}`;

  // 1. Seed a pending conversation by sending a user message to the live worker.
  const seed = await fetch(`${SUPPORT}/message`, {
    method: 'POST',
    headers: { Authorization: `Bearer ${userJwt}`, 'Content-Type': 'application/json' },
    body: JSON.stringify({ client_id: randomUUID(), text: userMsg }),
  });
  assert(seed.ok, `seed /message -> ${seed.status} ${await seed.text()}`);

  const browser = await chromium.launch();
  const ctx = await browser.newContext();
  const page = await ctx.newPage();
  // Inject the expert token BEFORE the app loads (skips passkey; the queue path is
  // what we're testing).
  await page.addInitScript((tok) => localStorage.setItem('auth_token', tok), expertJwt);

  const errors = [];
  page.on('console', (m) => { if (m.type() === 'error') errors.push(m.text()); });

  await page.goto(baseUrl);

  // 2. Queue shows the seeded conversation (find the row by its preview text).
  const row = page.getByTestId('conv').filter({ hasText: userMsg });
  await row.waitFor({ state: 'visible', timeout: 15_000 });

  // 3. Open the thread → the user's message is there.
  await row.click();
  await page.getByTestId('msg').filter({ hasText: userMsg }).waitFor({ state: 'visible', timeout: 10_000 });

  // 4. Reply as the expert.
  await page.getByTestId('reply-input').fill(replyMsg);
  await page.getByTestId('reply-send').click();
  const replyBubble = page.getByTestId('msg').filter({ hasText: replyMsg });
  await replyBubble.waitFor({ state: 'visible', timeout: 10_000 });
  assert((await replyBubble.getAttribute('data-sender')) === 'expert', 'reply bubble must be sender=expert');

  // 5. The reply reaches the USER side via the worker (cursor GET).
  const poll = await fetch(`${SUPPORT}/messages?after_seq=0&limit=100`, {
    headers: { Authorization: `Bearer ${userJwt}` },
  });
  assert(poll.ok, `user /messages -> ${poll.status}`);
  const { messages } = await poll.json();
  assert(
    messages.some((m) => m.sender === 'expert' && m.text === replyMsg),
    'expert reply must be visible on the user side',
  );

  assert(errors.length === 0, `console errors: ${errors.join(' | ')}`);

  await browser.close();
  if (server) server.close();
  console.log(`✅ admin smoke PASSED (${baseUrl})`);
}

main().catch((e) => {
  console.error('❌ admin smoke FAILED:', e.message);
  process.exit(1);
});
