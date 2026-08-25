// Завести на DEV худеющего с оплаченной подпиской и выдать ссылку онбординга.
//
// Нужно, чтобы руками пройти путь, который иначе начинается с настоящей оплаты:
// принять приглашение куратора, отправить отчёт, увидеть применённую планку.
//
// Никаких обходных путей здесь нет — это тот же порядок, каким пользуется бот, и
// та же цепочка, что в scripts/check-deleted-key-login.mjs:
//   1. auth-worker  /internal/account-resolve  — завести аккаунт, получить userId
//   2. payment-worker /internal/checkout       — выставить счёт
//   3. lava-mock    /pay/confirm               — подтвердить оплату (мок, не деньги)
//   4. payment-worker /subscription            — убедиться, что подписка активна
//   5. auth-worker  /internal/code/mint        — одноразовый код входа
//
// Всё это ТОЛЬКО на dev: lava-mock в прод не катится, а `dev-internal-push-key`
// там не действует.
//
// Запуск:
//   node scripts/dev-onboard.mjs              — новый пользователь + ссылка
//   node scripts/dev-onboard.mjs <userId>     — свежий код тому же (код живёт 10 минут)

import { createHmac } from 'node:crypto';

const AUTH = process.env.AUTH_BASE ?? 'https://auth-worker-dev.vg-stavenko.workers.dev';
const PAY  = process.env.PAY_BASE  ?? 'https://payment-worker-dev.vg-stavenko.workers.dev';
const MOCK = process.env.LAVA_MOCK ?? 'https://lava-mock-dev.vg-stavenko.workers.dev';
const FE   = process.env.FE_BASE   ?? 'https://renorma-fit-dev.pages.dev';
const KEY  = process.env.INTERNAL_KEY ?? 'dev-internal-push-key';
const JWT_SECRET = process.env.JWT_SECRET ?? 'dev-secret-change-in-production';

const K = { 'X-Internal-Key': KEY };
const post = (u, body, h = {}) =>
  fetch(u, { method: 'POST', headers: { 'Content-Type': 'application/json', ...h }, body: JSON.stringify(body) });
const j = async (r) => {
  const t = await r.text();
  try { return JSON.parse(t); } catch { throw new Error(`${r.status}: ${t.slice(0, 200)}`); }
};
const die = (m) => { console.error(`ПРОВАЛ: ${m}`); process.exit(1); };

/// Токен пользователя тем же секретом, что у dev-воркера, — только чтобы
/// ПРОЧИТАТЬ статус подписки его же глазами. Ничего не меняет.
function mint(sub) {
  const b64 = (x) => Buffer.from(x).toString('base64url');
  const si = `${b64(JSON.stringify({ alg: 'HS256', typ: 'JWT' }))}.` +
             `${b64(JSON.stringify({ sub, iat: 0, exp: 4102444800, caps: [], token_id: 'dev-onboard' }))}`;
  return `${si}.${b64(createHmac('sha256', JWT_SECRET).update(si).digest())}`;
}

async function subscription(userId) {
  return j(await fetch(`${PAY}/subscription`, { headers: { Authorization: `Bearer ${mint(userId)}` } }));
}

async function createPaidUser() {
  const tg = 830000 + Math.floor(Math.random() * 100000);
  const username = `dev-${tg}`;

  const acc = await j(await post(`${AUTH}/internal/account-resolve`,
    { provider: 'telegram', providerUid: String(tg), username }, K));
  if (!acc.userId) die(`аккаунт не заведён: ${JSON.stringify(acc)}`);
  console.log(`аккаунт   ${acc.userId} (telegram ${tg})`);

  const co = await j(await post(`${PAY}/internal/checkout`,
    { tgUserId: tg, tgUsername: username, currency: 'RUB', paymentMethod: 'SBP' }, K));
  const oid = co.payUrl && new URL(co.payUrl).searchParams.get('oid');
  if (!oid) die(`счёт не выставлен: ${JSON.stringify(co)}`);
  console.log(`счёт      ${oid} на ${co.amount} ${co.currency}`);

  await post(`${MOCK}/pay/confirm`, { contractId: oid });

  const sub = await subscription(acc.userId);
  if (!sub.active) die(`подписка не активировалась: ${JSON.stringify(sub)}`);
  console.log(`подписка  ${sub.plan}/${sub.status}, до ${new Date(sub.end).toISOString().slice(0, 10)}`);
  return acc.userId;
}

const existing = process.argv[2];
const userId = existing ?? (await createPaidUser());

if (existing) {
  const sub = await subscription(userId);
  console.log(`подписка  ${sub.active ? `${sub.plan}/${sub.status}, до ${new Date(sub.end).toISOString().slice(0, 10)}` : 'НЕ АКТИВНА'}`);
}

const { code } = await j(await post(`${AUTH}/internal/code/mint`, { userId }, K));
if (!code) die('код не выдан');

console.log(`\nuserId: ${userId}`);
console.log(`ссылка (код живёт 10 минут, открыть в Safari на телефоне):\n`);
console.log(`${FE}/onboard?u=${userId}#code=${code}`);
console.log(`\nсвежий код тому же пользователю: node scripts/dev-onboard.mjs ${userId}`);
