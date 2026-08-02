// Обнуление тестового пользователя: заводим настоящий аккаунт, наполняем его
// данными во всех воркерах, снимаем карточку, стираем и проверяем, что нигде
// ничего не осталось. Плюс проверка безопасности: ручки стирания недостижимы
// снаружи, а админская ручка — без прав администратора.
const AUTH = "https://auth-worker-dev.vg-stavenko.workers.dev";
const PAY = "https://payment-worker-dev.vg-stavenko.workers.dev";
const SYNC = "https://sync-worker-dev.vg-stavenko.workers.dev";
const SUPPORT = "https://support-worker-dev.vg-stavenko.workers.dev";
const BUG = "https://bug-report-worker-dev.vg-stavenko.workers.dev";
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

/// Завести настоящий аккаунт через внутренний резолв идентичности + вход по коду.
async function makeAccount(tag) {
  const uid_hint = `${tag}-${Date.now()}-${Math.floor(Math.random() * 1e6)}`;
  let r = await post(`${AUTH}/internal/account-resolve`,
    { provider: "telegram", providerUid: uid_hint, username: tag },
    { "X-Internal-Key": INTERNAL_KEY });
  if (!r.ok) throw new Error(`account-resolve: HTTP ${r.status} ${await r.text()}`);
  const { userId } = await j(r);
  r = await post(`${AUTH}/internal/code/mint`, { userId }, { "X-Internal-Key": INTERNAL_KEY });
  if (!r.ok) throw new Error(`code/mint: HTTP ${r.status} ${await r.text()}`);
  const { code } = await j(r);
  r = await post(`${AUTH}/code/verify`, { userId, code });
  if (!r.ok) throw new Error(`code/verify: HTTP ${r.status} ${await r.text()}`);
  const { token } = await j(r);
  if (!token) throw new Error("code/verify не вернул токен");
  return { userId, token, tgUid: uid_hint };
}

// ── 0. Безопасность: ручки стирания не отвечают из интернета даже с ключом ──
for (const [name, url] of [
  ["auth", `${AUTH}/internal/user-wipe`],
  ["sync", `${SYNC}/internal/user-wipe?user_id=x`],
  ["support", `${SUPPORT}/internal/user-wipe`],
  ["bug-report", `${BUG}/internal/user-wipe`],
]) {
  const r = await post(url, { userId: "probe" }, { "X-Internal-Key": INTERNAL_KEY });
  check(`безопасность: ${name} не отдаёт wipe снаружи`, r.status === 404, `HTTP ${r.status}`);
}

// ── 1. Жертва: аккаунт с данными во всех системах ──
const victim = await makeAccount("wipe-victim");
console.log("victim:", victim.userId);

// оплата (тестовая) → подписка
{
  const co = await post(`${PAY}/test/guest-checkout`, { planId: "test" });
  const { claimId, secret } = await j(co);
  const cl = await post(`${PAY}/claim`, { claimId, secret }, { Authorization: `Bearer ${victim.token}` });
  check("жертва: подписка активирована", cl.ok, `HTTP ${cl.status}`);
}
// дневник: инициализировать журнал синхронизации
{
  const push = await post(`${SYNC}/sync/v2/push`,
    { base_version: 0, changes: [{ store: "foods", op: "upsert", ts: Date.now(),
      row: { id: "wipe-f1", name: "Тестовая еда", kcal: 1, protein: 0, fat: 0, carbs: 0,
             nutrients: {}, package_weight: null, is_recipe: false, recipe_id: null,
             archived: false, created_at: new Date().toISOString(), updated_at: new Date().toISOString() } }] },
    { Authorization: `Bearer ${victim.token}` });
  const v = await j(push);
  const init = await post(`${SYNC}/sync/v2/init`, { expected_version: v.version },
    { Authorization: `Bearer ${victim.token}` });
  check("жертва: дневник записан", push.ok && init.ok, `push ${push.status}, init ${init.status}`);
}
// поддержка
{
  const m = await post(`${SUPPORT}/message`, { client_id: `w-${Date.now()}`, text: "тестовое сообщение" },
    { Authorization: `Bearer ${victim.token}` });
  check("жертва: сообщение в поддержку записано", m.ok, `HTTP ${m.status}`);
}
// баг-репорт
{
  const b = await post(`${BUG}/report`, { title: "тест", area: "прочее", steps_to_reproduce: "-",
    expected: "-", actual: "-", severity: "low", app_version: "test" },
    { Authorization: `Bearer ${victim.token}` });
  check("жертва: баг-репорт записан", b.ok, `HTTP ${b.status}`);
}

// ── 2. Администратор (отдельный аккаунт, чтобы не стереть себя) ──
const admin = await makeAccount("wipe-admin");
{
  const rq = await post(`${SUPPORT}/admin/request`, {}, { Authorization: `Bearer ${admin.token}` });
  const { code } = await j(rq);
  const ap = await post(`${SUPPORT}/admin/approve`, { code }, { "X-Admin-Secret": ADMIN_APPROVE_SECRET });
  check("админ одобрен", rq.ok && ap.ok, `request ${rq.status}, approve ${ap.status}`);
}

// ── 3. Без прав администратора обнулить нельзя ──
{
  const r = await post(`${PAY}/admin/user-wipe`, { userId: victim.userId },
    { Authorization: `Bearer ${victim.token}` });
  check("безопасность: не-админ получает отказ", r.status === 403 || r.status === 401, `HTTP ${r.status}`);
  const anon = await post(`${PAY}/admin/user-wipe`, { userId: victim.userId });
  check("безопасность: без токена — отказ", anon.status === 401, `HTTP ${anon.status}`);
}

// ── 4. Карточка: видно, кого стираем ──
{
  const r = await fetch(`${PAY}/admin/user-card?user_id=${victim.userId}`,
    { headers: { Authorization: `Bearer ${admin.token}` } });
  const card = await j(r);
  check("карточка: получена", r.ok, `HTTP ${r.status}`);
  check("карточка: есть время создания аккаунта", Number.isFinite(card?.auth?.created_at),
    String(card?.auth?.created_at));
  check("карточка: перечислены токены", Array.isArray(card?.auth?.tokens) && card.auth.tokens.length >= 1,
    `tokens=${card?.auth?.tokens?.length}`);
  check("карточка: видна привязанная личность", card?.auth?.identity?.provider === "telegram");
  check("карточка: видно время оплаты",
    Array.isArray(card?.claims) && card.claims.some((c) => c.paid_at),
    `claims=${card?.claims?.length}`);
  check("карточка: ошибок обращения к auth нет", !card?.auth_error, String(card?.auth_error || ""));
}

// ── 5. Обнуление ──
let wipe;
{
  const r = await post(`${PAY}/admin/user-wipe`, { userId: victim.userId },
    { Authorization: `Bearer ${admin.token}` });
  wipe = await j(r);
  const bad = (wipe.steps || []).filter((s) => !s.ok);
  check("обнуление: все шаги успешны", r.status === 200 && wipe.ok === true,
    bad.length ? bad.map((s) => `${s.step}: ${s.error}`).join(" | ") : `HTTP ${r.status}`);
  console.log("  шаги:", (wipe.steps || []).map((s) => `${s.ok ? "✓" : "✗"} ${s.step}`).join(", "));
}

// ── 6. Проверка, что ничего не осталось ──
{
  const r = await fetch(`${PAY}/admin/user-card?user_id=${victim.userId}`,
    { headers: { Authorization: `Bearer ${admin.token}` } });
  const card = await j(r);
  check("после: аккаунта в auth нет", card?.auth?.exists === false, JSON.stringify(card?.auth?.exists));
  check("после: ключей и токенов нет",
    (card?.auth?.tokens?.length ?? 0) === 0 && (card?.auth?.passkeys?.length ?? 0) === 0);
  check("после: платежей нет", (card?.claims?.length ?? 0) === 0, `claims=${card?.claims?.length}`);
  check("после: подписка не активна", card?.subscription?.active === false,
    JSON.stringify(card?.subscription?.status));
}
{
  const r = await post(`${SYNC}/sync/v2/pull`, { since_version: 0 },
    { Authorization: `Bearer ${victim.token}` });
  const body = await j(r);
  check("после: дневник стёрт (store не инициализирован)",
    r.status === 409 && body?.error === "store_not_initialized", `HTTP ${r.status}`);
}
{
  const r = await fetch(`${SUPPORT}/messages?after_seq=0`, { headers: { Authorization: `Bearer ${victim.token}` } });
  const body = await j(r);
  check("после: переписка пуста", r.ok && (body?.messages?.length ?? 0) === 0,
    `HTTP ${r.status}, messages=${body?.messages?.length}`);
}
{
  // Токен жертвы больше не должен проходить проверку в auth-worker.
  const r = await post(`${AUTH}/token/validate`, {}, { Authorization: `Bearer ${victim.token}` });
  check("после: старый токен отклонён", !r.ok, `HTTP ${r.status}`);
}

console.log(fail === 0 ? "\n=== ALL OK ===" : `\n=== FAILURES: ${fail} ===`);
process.exit(fail === 0 ? 0 : 1);
