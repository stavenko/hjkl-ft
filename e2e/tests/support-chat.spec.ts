import { test, expect, type Page } from '@playwright/test';
import { createHmac, randomUUID } from 'node:crypto';
import { registerAccount } from './helpers';

/**
 * SUPPORT-CHAT PHASE 2 — Live thread e2e against the LIVE support-worker.
 *
 * What this file drives (UI only): toggle to Live mode, send an optimistic
 * message, observe the ack reconcile, observe an expert reply arrive via polling,
 * confirm the mode persists across reload, and confirm the AI thread stays
 * isolated from the Live thread.
 *
 * ──────────────────────────────────────────────────────────────────────────────
 * PARENT RESPONSIBILITIES (NOT done by this file — the agents do not deploy or run
 * wrangler):
 *   1. Deploy the frontend to https://hjkl-ft.pages.dev with `support_base_url`
 *      pointing at the live support-worker (config/frontend.toml already carries
 *      https://support-worker.vg-stavenko.workers.dev).
 *   2. EXPERT-REPLY TRIGGER for step 4: the support-worker must expose a way to
 *      inject an expert message into a user's thread so the poll can pick it up.
 *      Wire `injectExpertReply()` below to that mechanism. Two known options:
 *        (a) the worker exposes an authenticated operator endpoint
 *            POST {support_base_url}/admin/reply  { user_id, text }
 *            guarded by an OPERATOR_TOKEN secret the harness is given via
 *            process.env.SUPPORT_OPERATOR_TOKEN; OR
 *        (b) the parent mints an "expert" JWT (same signing key, role=expert) and
 *            POSTs the worker's expert-side message endpoint.
 *      Until the parent wires one of these, step 4 is SKIPPED (test.skip) rather
 *      than faking a reply — we never inject sample data into the assertions.
 *
 * The user JWT used to call the worker / trigger is read from localStorage
 * (`auth_token`), set by registerAccount().
 * ──────────────────────────────────────────────────────────────────────────────
 */

const SUPPORT_BASE_URL =
  process.env.SUPPORT_BASE_URL ?? 'https://support-worker.vg-stavenko.workers.dev';
// Expert-side injection (option (b) in the header): mint an expert JWT with the
// worker's signing key and POST its expert reply route. Dev defaults match the
// deployed support-worker (JWT_SECRET="dev-secret-change-in-production",
// EXPERT_IDS="expert-1,expert-2"). Override via env for other deployments.
const JWT_SECRET = process.env.SUPPORT_JWT_SECRET ?? 'dev-secret-change-in-production';
const EXPERT_ID = process.env.SUPPORT_EXPERT_ID ?? 'expert-1';

function b64url(buf: Buffer | string): string {
  return Buffer.from(buf).toString('base64url');
}

/** Mint an HS256 JWT for `sub`, signed with the worker's JWT_SECRET. */
function mintJwt(sub: string): string {
  const header = b64url(JSON.stringify({ alg: 'HS256', typ: 'JWT' }));
  const claims = b64url(
    JSON.stringify({ sub, iat: 0, exp: 4_102_444_800, caps: [], token_id: 'e2e' }),
  );
  const signingInput = `${header}.${claims}`;
  const sig = b64url(createHmac('sha256', JWT_SECRET).update(signingInput).digest());
  return `${signingInput}.${sig}`;
}

async function authToken(page: Page): Promise<string> {
  const token = await page.evaluate(() => localStorage.getItem('auth_token'));
  expect(token, 'auth_token must be set after registration').toBeTruthy();
  return token as string;
}

/**
 * Inject an expert reply into THIS user's thread so the next poll surfaces it, by
 * minting an expert JWT and POSTing the worker's expert reply route
 * (`POST /conversations/:uid/reply` { client_id, text }). This is the real
 * expert-side path the future admin PWA will use — no fake/sample data.
 */
async function injectExpertReply(page: Page, userId: string, text: string): Promise<boolean> {
  const res = await page.request.post(
    `${SUPPORT_BASE_URL}/conversations/${encodeURIComponent(userId)}/reply`,
    {
      headers: {
        'Authorization': `Bearer ${mintJwt(EXPERT_ID)}`,
        'Content-Type': 'application/json',
      },
      data: { client_id: randomUUID(), text },
    },
  );
  expect(res.ok(), `expert-reply trigger failed: ${res.status()} ${await res.text()}`).toBeTruthy();
  return true;
}

test('live support thread: toggle, optimistic send, expert reply, persistence, isolation', async ({ browser }) => {
  const ctx = await browser.newContext();
  const page = await ctx.newPage();
  await page.goto('/');
  const { userId } = await registerAccount(page);

  // registerAccount lands directly in the app with a claimed (active) sub —
  // there is no paywall overlay to suppress anymore.
  await expect(page.getByTestId('nav-support')).toBeVisible({ timeout: 15_000 });

  // ── Navigate to /chat ──
  await page.getByTestId('nav-support').click();
  await expect(page.getByTestId('chat-mode-toggle')).toBeVisible({ timeout: 10_000 });

  // ── 1. Default is AI; AI-only UI (Context) belongs to AI mode ──
  // (Context only shows once there are tool calls; the toggle itself is enough to
  // assert AI is the default-active button.)
  await expect(page.getByTestId('chat-mode-ai')).toHaveClass(/is-link/);

  // ── 2. Switch to Live; AI Context / streaming are gone, Live empty-state shows ──
  await page.getByTestId('chat-mode-live').click();
  await expect(page.getByTestId('chat-mode-live')).toHaveClass(/is-link/);
  await expect(page.getByTestId('chat-context')).toHaveCount(0);

  // ── 3. Send a message → optimistic "sending" bubble → reconciles after the ack ──
  const sent = `e2e live ping ${Date.now()}`;
  await page.getByTestId('chat-input').fill(sent);
  await page.getByTestId('chat-send').click();

  // Optimistic outbox bubble appears immediately.
  const outbox = page.getByTestId('live-outbox');
  await expect(outbox).toContainText(sent, { timeout: 5_000 });

  // After the /message ack it reconciles into a real user message (left/right
  // bubble keyed by seq) and the outbox row goes away.
  await expect(
    page.getByTestId('live-message').filter({ hasText: sent }),
  ).toBeVisible({ timeout: 15_000 });
  await expect(page.getByTestId('live-outbox')).toHaveCount(0, { timeout: 15_000 });

  // ── 4. Expert reply arrives via polling (expert JWT → expert reply route) ──
  const replyText = `e2e expert reply ${Date.now()}`;
  await injectExpertReply(page, userId, replyText);
  // Poll interval is ~4s; allow generous slack.
  const expertBubble = page.getByTestId('live-message').filter({ hasText: replyText });
  await expect(expertBubble).toBeVisible({ timeout: 30_000 });
  await expect(expertBubble).toHaveAttribute('data-role', 'expert');

  // ── 5. Mode persists across reload; Live cache restored from IndexedDB ──
  await page.reload();
  await expect(page.getByTestId('chat-mode-toggle')).toBeVisible({ timeout: 10_000 });
  await expect(page.getByTestId('chat-mode-live')).toHaveClass(/is-link/);
  await expect(
    page.getByTestId('live-message').filter({ hasText: sent }),
  ).toBeVisible({ timeout: 15_000 });

  // ── 6. Isolation: AI thread is unaffected by the Live thread ──
  await page.getByTestId('chat-mode-ai').click();
  await expect(page.getByTestId('chat-mode-ai')).toHaveClass(/is-link/);
  // Live messages are not shown in AI mode.
  await expect(page.getByTestId('live-message')).toHaveCount(0);
  // The AI message the Live `sent` text would have produced never existed.
  await expect(
    page.getByTestId('chat-message').filter({ hasText: sent }),
  ).toHaveCount(0);

  // Switch back to Live → Live messages reappear, AI bubbles absent.
  await page.getByTestId('chat-mode-live').click();
  await expect(
    page.getByTestId('live-message').filter({ hasText: sent }),
  ).toBeVisible({ timeout: 10_000 });

  await ctx.close();
});

test('push deep-link /chat?notif=1 opens the Live thread (not AI)', async ({ browser }) => {
  const ctx = await browser.newContext();
  const page = await ctx.newPage();
  await page.goto('/');
  await registerAccount(page);

  // Deep-link exactly as the support-worker push nudge does.
  await page.goto('/chat?notif=1');
  await expect(page.getByTestId('chat-mode-toggle')).toBeVisible({ timeout: 15_000 });

  // Live is the active segment, AI is not.
  await expect(page.getByTestId('chat-mode-live')).toHaveClass(/is-link/);
  await expect(page.getByTestId('chat-mode-ai')).not.toHaveClass(/is-link/);

  // The choice persists: a plain reload (no query) stays on Live.
  await page.goto('/chat');
  await expect(page.getByTestId('chat-mode-live')).toHaveClass(/is-link/, { timeout: 15_000 });

  await ctx.close();
});
