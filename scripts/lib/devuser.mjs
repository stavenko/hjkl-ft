// Завести на DEV худеющего с оплаченной подпиской.
//
// Тем же порядком, каким пользуется бот: account-resolve → счёт → подтверждение
// оплаты моком lava → проверка, что подписка активна. Обходных путей нет; всё
// держится на `dev-internal-push-key`, которого в проде не существует.
//
// Общее место для `dev-onboard.mjs` (выдать ссылку человеку) и браузерных
// проверок (им нужен НАСТОЯЩИЙ платный пользователь: без активной подписки
// приложение показывает замок и до проверяемого не доходит).

import { createHmac } from 'node:crypto';

export const AUTH = process.env.AUTH_BASE ?? 'https://auth-worker-dev.vg-stavenko.workers.dev';
export const PAY = process.env.PAY_BASE ?? 'https://payment-worker-dev.vg-stavenko.workers.dev';
export const MOCK = process.env.LAVA_MOCK ?? 'https://lava-mock-dev.vg-stavenko.workers.dev';
export const JWT_SECRET = process.env.JWT_SECRET ?? 'dev-secret-change-in-production';
const KEY = { 'X-Internal-Key': process.env.INTERNAL_KEY ?? 'dev-internal-push-key' };

const post = (u, body, h = {}) =>
  fetch(u, { method: 'POST', headers: { 'Content-Type': 'application/json', ...h }, body: JSON.stringify(body) });
const j = async (r) => {
  const t = await r.text();
  try { return JSON.parse(t); } catch { throw new Error(`${r.status}: ${t.slice(0, 200)}`); }
};

/// Токен пользователя тем же секретом, что у dev-воркеров.
export function mintToken(sub, secret = JWT_SECRET) {
  const b64 = (x) => Buffer.from(x).toString('base64url');
  const si = `${b64(JSON.stringify({ alg: 'HS256', typ: 'JWT' }))}.` +
             `${b64(JSON.stringify({ sub, iat: 0, exp: 4102444800, caps: [], token_id: 'devtest' }))}`;
  return `${si}.${b64(createHmac('sha256', secret).update(si).digest())}`;
}

/// Статус подписки ГЛАЗАМИ ПРИЛОЖЕНИЯ — тем же запросом, что делает фронтенд.
export async function subscription(userId) {
  return j(await fetch(`${PAY}/subscription`, { headers: { Authorization: `Bearer ${mintToken(userId)}` } }));
}

/// Новый пользователь с оплаченной подпиской. Возвращает `{ userId, tg, sub }`.
export async function createPaidUser(prefix = 'dev') {
  const tg = 830000 + Math.floor(Math.random() * 100000);
  const username = `${prefix}-${tg}`;
  const acc = await j(await post(`${AUTH}/internal/account-resolve`,
    { provider: 'telegram', providerUid: String(tg), username }, KEY));
  if (!acc.userId) throw new Error(`аккаунт не заведён: ${JSON.stringify(acc)}`);

  const co = await j(await post(`${PAY}/internal/checkout`,
    { tgUserId: tg, tgUsername: username, currency: 'RUB', paymentMethod: 'SBP' }, KEY));
  const oid = co.payUrl && new URL(co.payUrl).searchParams.get('oid');
  if (!oid) throw new Error(`счёт не выставлен: ${JSON.stringify(co)}`);
  await post(`${MOCK}/pay/confirm`, { contractId: oid });

  const sub = await subscription(acc.userId);
  if (!sub.active) throw new Error(`подписка не активировалась: ${JSON.stringify(sub)}`);
  return { userId: acc.userId, tg, sub };
}

/// Одноразовый код входа для ссылки онбординга.
export async function mintLoginCode(userId) {
  const { code } = await j(await post(`${AUTH}/internal/code/mint`, { userId }, KEY));
  if (!code) throw new Error('код не выдан');
  return code;
}
