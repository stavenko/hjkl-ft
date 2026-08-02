// Сброс доступа: пользователь возвращается в состояние «сразу после оплаты» —
// мини-апп снова показывает «Получить доступ к re:Norma», при этом деньги и
// личные данные остаются на месте.
const AUTH = "https://auth-worker-dev.vg-stavenko.workers.dev";
const PAY = "https://payment-worker-dev.vg-stavenko.workers.dev";
const SYNC = "https://sync-worker-dev.vg-stavenko.workers.dev";
const SUPPORT = "https://support-worker-dev.vg-stavenko.workers.dev";
const MOCK = process.env.LAVA_MOCK || "https://lava-mock-dev.vg-stavenko.workers.dev";
const INTERNAL_KEY = process.env.INTERNAL_KEY || "dev-internal-push-key";
const ADMIN_APPROVE_SECRET = process.env.ADMIN_APPROVE_SECRET || "dev-admin-approve-secret";

let fail = 0;
const check = (name, ok, extra = "") => {
  console.log(`${ok ? "OK " : "FAIL"} ${name}${extra ? " — " + extra : ""}`);
  if (!ok) fail++;
};
const j = async (r) => { const t = await r.text(); try { return JSON.parse(t); } catch { return { _raw: t }; } };
const post = (url, body, headers = {}) =>
  fetch(url, { method: "POST", headers: { "Content-Type": "application/json", ...headers }, body: JSON.stringify(body) });
/// Тот же сигнал, по которому мини-апп решает «Получить доступ» против «Открыть приложение».
const entered = async (userId) => {
  const r = await post(`${AUTH}/internal/has-entered`, { userId }, { "X-Internal-Key": INTERNAL_KEY });
  return (await j(r)).entered;
};

// ── Пользователь, прошедший полный путь: оплата → вход → данные ──
const tgId = 500000 + Math.floor(Math.random() * 200000);
let r = await post(`${AUTH}/internal/account-resolve`,
  { provider: "telegram", providerUid: String(tgId), username: `reset-${tgId}` },
  { "X-Internal-Key": INTERNAL_KEY });
const { userId } = await j(r);
r = await post(`${PAY}/internal/checkout`,
  { tgUserId: tgId, tgUsername: `reset-${tgId}`, currency: "RUB", paymentMethod: "SBP" },
  { "X-Internal-Key": INTERNAL_KEY });
const co = await j(r);
const contractId = new URL(co.payUrl).searchParams.get("oid");
check("подготовка: платёж проведён", (await post(`${MOCK}/pay/confirm`, { contractId })).ok);

r = await post(`${AUTH}/internal/code/mint`, { userId }, { "X-Internal-Key": INTERNAL_KEY });
const { code } = await j(r);
r = await post(`${AUTH}/code/verify`, { userId, code });
const { token } = await j(r);
check("подготовка: вход выполнен, есть токен", !!token);

// Личные данные: дневник + переписка + «вошёл в систему» (открытая глава).
{
  const push = await post(`${SYNC}/sync/v2/push`,
    { base_version: 0, changes: [{ store: "foods", op: "upsert", ts: Date.now(),
      row: { id: "reset-f1", name: "Личная еда", kcal: 1, protein: 0, fat: 0, carbs: 0,
             nutrients: {}, package_weight: null, is_recipe: false, recipe_id: null, archived: false,
             created_at: new Date().toISOString(), updated_at: new Date().toISOString() } }] },
    { Authorization: `Bearer ${token}` });
  const v = await j(push);
  await post(`${SYNC}/sync/v2/init`, { expected_version: v.version }, { Authorization: `Bearer ${token}` });
  await post(`${SUPPORT}/message`, { client_id: `r-${Date.now()}`, text: "личное сообщение" },
    { Authorization: `Bearer ${token}` });
  const ch = await post(`${AUTH}/chapters/available`, { chapter: "ch1" }, { Authorization: `Bearer ${token}` });
  check("подготовка: пользователь «вошёл в систему»", ch.ok && (await entered(userId)) === true);
}

// ── Оператор ──
const adminHint = `reset-admin-${Date.now()}`;
r = await post(`${AUTH}/internal/account-resolve`,
  { provider: "telegram", providerUid: adminHint, username: "reset-admin" }, { "X-Internal-Key": INTERNAL_KEY });
const admin = await j(r);
r = await post(`${AUTH}/internal/code/mint`, { userId: admin.userId }, { "X-Internal-Key": INTERNAL_KEY });
const { code: ac } = await j(r);
r = await post(`${AUTH}/code/verify`, { userId: admin.userId, code: ac });
const { token: adminToken } = await j(r);
r = await post(`${SUPPORT}/admin/request`, {}, { Authorization: `Bearer ${adminToken}` });
const { code: reqCode } = await j(r);
await post(`${SUPPORT}/admin/approve`, { code: reqCode }, { "X-Admin-Secret": ADMIN_APPROVE_SECRET });

// ── Права ──
{
  const anon = await post(`${PAY}/admin/user-reset`, { userId });
  check("безопасность: без токена — отказ", anon.status === 401, `HTTP ${anon.status}`);
  const notAdmin = await post(`${PAY}/admin/user-reset`, { userId }, { Authorization: `Bearer ${token}` });
  check("безопасность: не-админ — отказ", notAdmin.status === 403, `HTTP ${notAdmin.status}`);
}

// ── Сброс доступа ──
{
  const r = await post(`${PAY}/admin/user-reset`, { userId }, { Authorization: `Bearer ${adminToken}` });
  const rep = await j(r);
  const bad = (rep.steps || []).filter((s) => !s.ok);
  check("сброс: выполнен", r.status === 200 && rep.ok === true,
    bad.map((s) => `${s.step}: ${s.error}`).join(" | ") || `HTTP ${r.status}`);
  console.log("  шаги:", (rep.steps || []).map((s) => `${s.ok ? "✓" : "✗"} ${s.step}`).join(", "));
}

// ── Состояние «сразу после оплаты» ──
check("после: мини-апп снова покажет «Получить доступ»", (await entered(userId)) === false);
{
  const r = await post(`${AUTH}/token/validate`, {}, { Authorization: `Bearer ${token}` });
  check("после: старый токен больше не работает", !r.ok, `HTTP ${r.status}`);
}
{
  const r = await fetch(`${PAY}/admin/user-card?user_id=${userId}`, { headers: { Authorization: `Bearer ${adminToken}` } });
  const card = await j(r);
  check("после: аккаунт и личность сохранены",
    card?.auth?.exists === true && card?.auth?.identity?.provider === "telegram");
  check("после: ключей и токенов нет",
    (card?.auth?.tokens?.length ?? 0) === 0 && (card?.auth?.passkeys?.length ?? 0) === 0);
  check("после: платёж сохранён", (card?.claims ?? []).some((c) => c.paid_at), `claims=${card?.claims?.length}`);
  check("после: подписка активна", card?.subscription?.active === true, String(card?.subscription?.status));
}
// Личные данные на месте: журнал по-прежнему инициализирован (данные не стёрты).
{
  const fresh = await post(`${AUTH}/internal/code/mint`, { userId }, { "X-Internal-Key": INTERNAL_KEY });
  const { code: c2 } = await j(fresh);
  const v = await post(`${AUTH}/code/verify`, { userId, code: c2 });
  const { token: newToken } = await j(v);
  check("после: вход по коду снова возможен (честный онбординг)", !!newToken);
  const pull = await post(`${SYNC}/sync/v2/pull`, { since_version: 0 }, { Authorization: `Bearer ${newToken}` });
  const body = await j(pull);
  const foods = JSON.stringify(body?.batches ?? []).includes("reset-f1");
  check("после: дневник сохранён", pull.ok && foods, `HTTP ${pull.status}`);
  const msgs = await fetch(`${SUPPORT}/messages?after_seq=0`, { headers: { Authorization: `Bearer ${newToken}` } });
  const mb = await j(msgs);
  check("после: переписка сохранена", (mb?.messages?.length ?? 0) > 0, `messages=${mb?.messages?.length}`);
}

console.log(fail === 0 ? "\n=== ALL OK ===" : `\n=== FAILURES: ${fail} ===`);
process.exit(fail === 0 ? 0 : 1);
