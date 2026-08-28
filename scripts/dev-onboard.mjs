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

import { createPaidUser, mintLoginCode, subscription } from './lib/devuser.mjs';

const FE = process.env.FE_BASE ?? 'https://renorma-fit-dev.pages.dev';
const die = (m) => { console.error(`ПРОВАЛ: ${m}`); process.exit(1); };

const existing = process.argv[2];
let userId = existing;
if (!userId) {
  const made = await createPaidUser('dev').catch((e) => die(e.message));
  userId = made.userId;
  console.log(`аккаунт   ${userId} (telegram ${made.tg})`);
}
const sub = await subscription(userId).catch((e) => die(e.message));
console.log(`подписка  ${sub.active ? `${sub.plan}/${sub.status}, до ${new Date(sub.end).toISOString().slice(0, 10)}` : 'НЕ АКТИВНА'}`);

const code = await mintLoginCode(userId).catch((e) => die(e.message));

console.log(`\nuserId: ${userId}`);
console.log(`ссылка (код живёт 10 минут, открыть в Safari на телефоне):\n`);
console.log(`${FE}/onboard?u=${userId}#code=${code}`);
console.log(`\nсвежий код тому же пользователю: node scripts/dev-onboard.mjs ${userId}`);
