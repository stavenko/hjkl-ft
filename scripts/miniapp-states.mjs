// Capture the Telegram Mini App (payment bot) in three subscription states, on the
// DEV/test stack, driving the REAL backend flow — checkout → lava-mock pay → webhook,
// plus optional "enter the app" (chapter) and cancel.
//
// The Mini App is normally reachable only inside Telegram (it validates signed
// initData). On DEV the bot token is the PUBLIC placeholder from wrangler.toml
// (`dev-telegram-bot-token`), so we self-sign valid initData here — no real bot
// secret, no real Telegram. `telegram-web-app.js` is neutralised (page.route) so it
// can't overwrite our injected `window.Telegram` stub.
//
// Produces:
//   scripts/miniapp-A-paid-not-linked.png  — paid, app NOT linked  → «Получить доступ к re:Norma»
//   scripts/miniapp-B-linked.png           — paid + entered        → «Открыть приложение»
//   scripts/miniapp-C-cancelled.png        — entered + cancelled    → «Подписка отменена · N дней»
//
// Run: node scripts/miniapp-states.mjs
import pw from '../e2e/node_modules/@playwright/test/index.js';
import crypto from 'node:crypto';
const { chromium } = pw;

const TG   = process.env.TG_URL   ?? 'https://telegram-worker-dev.vg-stavenko.workers.dev';
const PAY  = process.env.PAY_URL  ?? 'https://payment-worker-dev.vg-stavenko.workers.dev';
const AUTH = process.env.AUTH_URL ?? 'https://auth-worker-dev.vg-stavenko.workers.dev';
const MOCK = process.env.MOCK_URL ?? 'https://lava-mock-dev.vg-stavenko.workers.dev';
const IKEY = process.env.INTERNAL_KEY ?? 'dev-internal-push-key';
const BOT  = process.env.BOT_TOKEN    ?? 'dev-telegram-bot-token';

// ── self-signed Telegram WebApp initData (dev bot token = public placeholder) ──
function initData(id, username) {
  const user = JSON.stringify({ id, username, first_name: 'E2E' });
  const fields = { auth_date: String(Math.floor(Date.now() / 1000)), user };
  const dcs = Object.keys(fields).sort().map((k) => `${k}=${fields[k]}`).join('\n');
  const secret = crypto.createHmac('sha256', 'WebAppData').update(BOT).digest();
  const hash = crypto.createHmac('sha256', secret).update(dcs).digest('hex');
  return new URLSearchParams({ ...fields, hash }).toString();
}

async function post(url, headers, body) {
  const r = await fetch(url, {
    method: 'POST',
    headers: { 'Content-Type': 'application/json', ...headers },
    body: JSON.stringify(body ?? {}),
  });
  try { return await r.json(); } catch { return {}; }
}

const resolveUser = (uid, un) =>
  post(`${AUTH}/internal/account-resolve`, { 'X-Internal-Key': IKEY },
    { provider: 'telegram', providerUid: String(uid), username: un }).then((r) => r.userId);

async function checkoutAndPay(tg) {
  const co = await post(`${PAY}/internal/checkout`, { 'X-Internal-Key': IKEY },
    { tgUserId: tg, currency: 'RUB', paymentMethod: 'CARD' });
  const oid = (co.payUrl || '').split('oid=')[1]?.split('&')[0] ?? '';
  await post(`${MOCK}/pay/confirm`, {}, { contractId: oid });  // «Оплатить (тест)» → webhook
  return { claimId: co.claimId, secret: co.secret, oid };
}

async function appToken(uid) {
  const { code } = await post(`${AUTH}/internal/code/mint`, { 'X-Internal-Key': IKEY }, { userId: uid });
  const { token } = await post(`${AUTH}/code/verify`, {}, { userId: uid, code });
  return token;
}

// "Enter the app": the first story chapter becoming available is the «app linked» signal.
const enterApp = (token) =>
  post(`${AUTH}/chapters/available`, { Authorization: `Bearer ${token}` }, { chapter: 'ch1' });

async function shoot(browser, { id, username, file }) {
  const ctx = await browser.newContext({ viewport: { width: 390, height: 844 } });
  const page = await ctx.newPage();
  // Neutralise the real telegram-web-app.js so it can't clobber our injected stub.
  await page.route('**/telegram-web-app.js', (route) =>
    route.fulfill({ contentType: 'application/javascript', body: '' }));
  await page.addInitScript((d) => {
    window.Telegram = { WebApp: {
      initData: d.initData,
      initDataUnsafe: { user: d.user },
      ready() {}, expand() {}, openLink(u) { window.__opened = u; },
    } };
  }, { initData: initData(id, username), user: { id, username, first_name: 'E2E' } });

  await page.goto(TG, { waitUntil: 'networkidle' });
  await page.waitForSelector('#access:not(.hidden)', { timeout: 15000 });
  await page.waitForTimeout(500);
  await page.screenshot({ path: file });
  const btn = (await page.textContent('#createBtn').catch(() => '')).trim();
  const status = (await page.textContent('#accessStatus').catch(() => '')).trim();
  console.log(`✔ ${file}\n    button:  ${btn}\n    status:  ${status}`);
  await ctx.close();
}

async function main() {
  const browser = await chromium.launch();
  try {
    const rnd = Math.floor(Date.now() / 1000) % 100000;

    // ── A: paid, app NOT linked → «Получить доступ к re:Norma» ──
    const a = { id: 910000 + rnd, username: 'tester_a' };
    await resolveUser(a.id, a.username);
    await checkoutAndPay(a.id);
    await shoot(browser, { ...a, file: 'scripts/miniapp-A-paid-not-linked.png' });

    // ── B: paid + entered → «Открыть приложение» ──
    const b = { id: 920000 + rnd, username: 'tester_b' };
    const uidB = await resolveUser(b.id, b.username);
    await checkoutAndPay(b.id);
    await enterApp(await appToken(uidB));
    await shoot(browser, { ...b, file: 'scripts/miniapp-B-linked.png' });

    // ── C: paid + entered + bound + cancelled → «Подписка отменена · N дней» ──
    const c = { id: 930000 + rnd, username: 'tester_c' };
    const uidC = await resolveUser(c.id, c.username);
    const { claimId, secret } = await checkoutAndPay(c.id);
    const tokenC = await appToken(uidC);
    await enterApp(tokenC);
    await post(`${PAY}/claim`, { Authorization: `Bearer ${tokenC}` }, { claimId, secret }); // bind (claimed_by)
    await post(`${PAY}/cancel`, { Authorization: `Bearer ${tokenC}` }, {});                 // cancel → no-renew
    await shoot(browser, { ...c, file: 'scripts/miniapp-C-cancelled.png' });
  } finally {
    await browser.close();
  }
}

main().catch((e) => { console.error(e); process.exit(1); });
