// Живая проверка на ДЕВЕ: что происходит, когда у человека срывается продление.
//
// Сценарий повторяет сентябрьскую историю: подписчик оплатил через бота, потом lava
// не смогла списать, а на третьи сутки закрыла подписку. Проверяем, что теперь ни
// один шаг не проходит молча — тело вебхука ложится в архив, тип события читается
// правильно, отмена доезжает до подписки, а человек попадает в рассылку.
//
// Запуск: node scripts/check-subscription-notices.mjs
const AUTH = process.env.AUTH || "https://auth-worker-dev.vg-stavenko.workers.dev";
const PAY = process.env.PAY || "https://payment-worker-dev.vg-stavenko.workers.dev";
const SUPPORT = process.env.SUPPORT || "https://support-worker-dev.vg-stavenko.workers.dev";
const MOCK = process.env.LAVA_MOCK || "https://lava-mock-dev.vg-stavenko.workers.dev";
const INTERNAL_KEY = process.env.INTERNAL_KEY || "dev-internal-push-key";
const HOOK_KEY = process.env.LAVA_HOOK_KEY || "dev-lava-hook-key";
const ADMIN_APPROVE_SECRET = process.env.ADMIN_APPROVE_SECRET || "dev-admin-approve-secret";

let failed = 0;
const check = (name, ok, detail = "") => {
  console.log(`${ok ? "  ok  " : "ПРОВАЛ"} ${name}${ok || !detail ? "" : " — " + detail}`);
  if (!ok) failed++;
};
const post = (url, body, headers = {}) =>
  fetch(url, { method: "POST", headers: { "Content-Type": "application/json", ...headers }, body: JSON.stringify(body) });
const j = async (r) => { try { return await r.json(); } catch { return {}; } };

// ── подписчик: настоящий путь оплаты (телеграм-чекаут + оплата в моке) ───────
const tgId = 700000 + Math.floor(Math.random() * 200000);
const NAME = `notice-victim-${tgId}`;
let r = await post(`${AUTH}/internal/account-resolve`,
  { provider: "telegram", providerUid: String(tgId), username: NAME },
  { "X-Internal-Key": INTERNAL_KEY });
const { userId } = await j(r);
r = await post(`${PAY}/internal/checkout`,
  { tgUserId: tgId, tgUsername: NAME, currency: "RUB", paymentMethod: "SBP" },
  { "X-Internal-Key": INTERNAL_KEY });
const co = await j(r);
if (!r.ok) throw new Error(`checkout: HTTP ${r.status} ${JSON.stringify(co)}`);
const contractId = new URL(co.payUrl).searchParams.get("oid");
check("подготовка: платёж проведён", (await post(`${MOCK}/pay/confirm`, { contractId })).ok);
console.log(`подписчик ${userId}, контракт ${contractId}`);

// ── оператор ────────────────────────────────────────────────────────────────
const admin = await (async () => {
  const id = "notice-admin-" + Date.now();
  let a = await post(`${AUTH}/internal/account-resolve`,
    { provider: "telegram", providerUid: id, username: id }, { "X-Internal-Key": INTERNAL_KEY });
  const { userId: uid } = await j(a);
  a = await post(`${AUTH}/internal/code/mint`, { userId: uid }, { "X-Internal-Key": INTERNAL_KEY });
  const { code } = await j(a);
  const v = await j(await post(`${AUTH}/code/verify`, { userId: uid, code }));
  const rq = await j(await post(`${SUPPORT}/admin/request`, {}, { Authorization: `Bearer ${v.token}` }));
  await post(`${SUPPORT}/admin/approve`, { code: rq.code }, { "X-Admin-Secret": ADMIN_APPROVE_SECRET });
  return { userId: uid, token: v.token };
})();
const asAdmin = { Authorization: `Bearer ${admin.token}` };
const events = async () => (await j(await fetch(`${PAY}/admin/webhook-events`, { headers: asAdmin }))).events || [];
const card = async (uid) => j(await fetch(`${PAY}/admin/user-card?user_id=${encodeURIComponent(uid)}`, { headers: asAdmin }));
const sub = async (uid = userId) => (await card(uid)).subscription || {};

const hook = (body) => post(`${PAY}/webhook/lava`, body, { "X-Api-Key": HOOK_KEY });
const stamp = Date.now();

// ── 1. Сорванное продление ──────────────────────────────────────────────────
const failedEvent = {
  eventType: "subscription.recurring.payment.failed",
  contractId, parentContractId: contractId,
  buyer: { email: co.email || `${NAME}@example.org` },
  status: "subscription-failed",
  errorMessage: "Недостаточно средств на карте",
  timestamp: `${stamp}`,
  id: `evt-failed-${stamp}`,
};
check("вебхук о сорванном списании принят", (await hook(failedEvent)).ok);
{
  const e = (await events()).find((x) => x.event_type === "subscription.recurring.payment.failed");
  check("он лёг в архив", !!e);
  check("тип события распознан как «неудача»", e?.kind === "failed", e?.kind);
  check("причина отказа сохранена", e?.error_message === "Недостаточно средств на карте", e?.error_message);
  check("тело сохранено целиком", (() => {
    try { return JSON.parse(e.payload).errorMessage === "Недостаточно средств на карте"; } catch { return false; }
  })());
}
// Повтор того же события не плодит строк в архиве (по нашему прогону, а не по всем).
await hook(failedEvent);
check("повтор события не дублируется в архиве",
  (await events()).filter((x) => (x.payload || "").includes(`evt-failed-${stamp}`)).length === 1);
check("подписка НЕ тронута срывом продления", (await sub()).status === "paid", JSON.stringify(await sub()));

// ── 2. Событие неизвестного типа ────────────────────────────────────────────
check("вебхук неизвестного типа принят", (await hook({
  eventType: "subscription.something.we.have.never.seen",
  contractId, parentContractId: contractId,
  timestamp: `${stamp}`, id: `evt-unknown-${stamp}`,
})).ok);
{
  const e = (await events()).find((x) => x.event_type === "subscription.something.we.have.never.seen");
  check("неизвестное событие тоже в архиве", !!e);
  check("и помечено как неизвестное, а не как «неудача»", e?.kind === "unknown", e?.kind);
}

// ── 3. Отмена ───────────────────────────────────────────────────────────────
check("вебхук об отмене принят", (await hook({
  eventType: "subscription.cancelled",
  contractId, parentContractId: contractId,
  willExpireAt: new Date(Date.now() + 12 * 86400000).toISOString(),
  timestamp: `${stamp}`, id: `evt-cancel-${stamp}`,
})).ok);
{
  const s = await sub();
  check("подписка помечена отменённой", s.status === "cancelled", JSON.stringify(s));
  check("доступ пока сохранён", s.active === true, JSON.stringify(s));
}

// ── 4. Отмена под именем, которого нет в нашей таблице ──────────────────────
{
  // Второй подписчик: проверяем, что отмену узнаю́т и по смыслу слова в типе.
  const tg2 = 700000 + Math.floor(Math.random() * 200000);
  const n2 = `notice-victim2-${tg2}`;
  await post(`${AUTH}/internal/account-resolve`,
    { provider: "telegram", providerUid: String(tg2), username: n2 }, { "X-Internal-Key": INTERNAL_KEY });
  const c2 = await j(await post(`${PAY}/internal/checkout`,
    { tgUserId: tg2, tgUsername: n2, currency: "RUB", paymentMethod: "SBP" },
    { "X-Internal-Key": INTERNAL_KEY }));
  const contract2 = new URL(c2.payUrl).searchParams.get("oid");
  await post(`${MOCK}/pay/confirm`, { contractId: contract2 });
  await hook({
    eventType: "contract.subscription.canceled",   // другое написание, которого мы не знали
    contractId: contract2, parentContractId: contract2,
    timestamp: `${stamp}`, id: `evt-cancel2-${stamp}`,
  });
  const uid2 = (await j(await post(`${AUTH}/internal/account-resolve`,
    { provider: "telegram", providerUid: String(tg2), username: n2 }, { "X-Internal-Key": INTERNAL_KEY }))).userId;
  const s2 = await sub(uid2);
  check("отмена опознана по смыслу типа, а не по точному имени", s2.status === "cancelled", JSON.stringify(s2));
}

// ── 5. Письмо lava «не удалось продлить» ────────────────────────────────────
{
  const c = await card(userId);
  const email = (c.claims || []).map((x) => x.email).find(Boolean);
  check("у платежа есть адрес для чеков", !!email, JSON.stringify(c.claims || []).slice(0, 200));
  if (email) {
    const rr = await j(await post(`${PAY}/internal/receipt`,
      { email, messageId: `<mail-${stamp}@lava>`, amount: 5000, currency: "RUB",
        bodyText: "…", kind: "renewal_failed" },
      { "X-Internal-Key": INTERNAL_KEY }));
    check("письмо о срыве продления привязано к платежу", rr.bound === true, JSON.stringify(rr));
  }
}

// ── 6. Рассылка вдогонку ────────────────────────────────────────────────────
{
  // Ручка отдаёт список страницами (на деве накопилось ~2400 тестовых аккаунтов).
  // Список отсортирован по свежести платежа, а наш подписчик заплатил только что —
  // значит он на первой странице.
  const page = await j(await post(`${PAY}/admin/notify-cancelled`, { dryRun: true, offset: 0, limit: 25 }, asAdmin));
  check("сухой прогон ничего не отправляет", page.dryRun === true && page.sent === 0, JSON.stringify(page).slice(0, 160));
  check("ответ говорит, докуда дошли", page.scanned === 25 && page.nextOffset === 25, `scanned=${page.scanned} next=${page.nextOffset}`);
  const mine = (page.users || []).find((u) => u.userId === userId);
  check("подписчик с отменённой подпиской попал в список", !!mine, `кандидатов на странице: ${page.candidates}`);
  check("в списке видно, сколько дней доступа осталось", (mine?.daysLeft ?? -1) > 0, JSON.stringify(mine));
  check("видно, есть ли куда писать", mine?.tgUserId != null || mine?.skipped === "no_telegram", JSON.stringify(mine));
}

console.log(failed ? `\nПРОВАЛОВ: ${failed}` : "\nвсё сошлось");
process.exit(failed ? 1 : 0);
