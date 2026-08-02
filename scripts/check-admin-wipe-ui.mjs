// Интерфейс обнуления в админке: карточка пользователя открывается по строке,
// кнопка заряжается только 10 быстрыми нажатиями подряд, дальше подтверждение,
// после — пошаговый отчёт. Проверяем и защиту: медленные нажатия не взводят.
import { chromium } from "playwright";
const ADMIN = process.env.ADMIN || "https://renorma-admin-dev.pages.dev";
const AUTH = "https://auth-worker-dev.vg-stavenko.workers.dev";
const PAY = "https://payment-worker-dev.vg-stavenko.workers.dev";
const SUPPORT = "https://support-worker-dev.vg-stavenko.workers.dev";
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

async function makeAccount(tag) {
  const hint = `${tag}-${Date.now()}-${Math.floor(Math.random() * 1e6)}`;
  let r = await post(`${AUTH}/internal/account-resolve`,
    { provider: "telegram", providerUid: hint, username: tag }, { "X-Internal-Key": INTERNAL_KEY });
  const { userId } = await j(r);
  r = await post(`${AUTH}/internal/code/mint`, { userId }, { "X-Internal-Key": INTERNAL_KEY });
  const { code } = await j(r);
  r = await post(`${AUTH}/code/verify`, { userId, code });
  const { token } = await j(r);
  return { userId, token };
}

// Жертва проходит НАСТОЯЩИЙ путь оплаты (телеграм-чекаут + оплата в моке lava):
// только такие платежи попадают в список пользователей, и только у них есть
// контракт — то есть шаг «отмена подписки у провайдера» реально отрабатывает.
const MOCK = process.env.LAVA_MOCK || "https://lava-mock-dev.vg-stavenko.workers.dev";
const tgId = 700000 + Math.floor(Math.random() * 200000);
// Имя уникально на прогон: в списке живут жертвы прошлых запусков.
const VICTIM_NAME = `ui-victim-${tgId}`;
const victim = await (async () => {
  let r = await post(`${AUTH}/internal/account-resolve`,
    { provider: "telegram", providerUid: String(tgId), username: VICTIM_NAME },
    { "X-Internal-Key": INTERNAL_KEY });
  const { userId } = await j(r);
  r = await post(`${PAY}/internal/checkout`,
    { tgUserId: tgId, tgUsername: VICTIM_NAME, currency: "RUB", paymentMethod: "SBP" },
    { "X-Internal-Key": INTERNAL_KEY });
  const co = await j(r);
  if (!r.ok) throw new Error(`checkout: HTTP ${r.status} ${JSON.stringify(co)}`);
  const contractId = new URL(co.payUrl).searchParams.get("oid");
  const pay = await post(`${MOCK}/pay/confirm`, { contractId });
  check("подготовка: платёж жертвы проведён", pay.ok, `HTTP ${pay.status}`);
  // Жертва входит по коду — как реальный пользователь по ссылке из бота, чтобы
  // у неё был токен (его тоже показываем и стираем).
  r = await post(`${AUTH}/internal/code/mint`, { userId }, { "X-Internal-Key": INTERNAL_KEY });
  const { code } = await j(r);
  await post(`${AUTH}/code/verify`, { userId, code });
  return { userId, contractId };
})();
console.log("victim:", victim.userId, "contract:", victim.contractId);
// Оператор-администратор.
const admin = await makeAccount("ui-admin");
{
  const rq = await post(`${SUPPORT}/admin/request`, {}, { Authorization: `Bearer ${admin.token}` });
  const { code } = await j(rq);
  const ap = await post(`${SUPPORT}/admin/approve`, { code }, { "X-Admin-Secret": ADMIN_APPROVE_SECRET });
  check("подготовка: оператор одобрен", rq.ok && ap.ok);
}

const b = await chromium.launch({ headless: true });
const ctx = await b.newContext({ viewport: { width: 430, height: 932 } });
const p = await ctx.newPage();
p.on("console", (m) => { const t = m.text(); if (/panicked/.test(t)) console.log("[admin]", t.slice(0, 200)); });
await p.goto(ADMIN, { waitUntil: "domcontentloaded" });
await p.evaluate(({ uid, token }) => {
  localStorage.clear();
  localStorage.setItem("user_id", uid);
  localStorage.setItem("auth_token", token);
}, { uid: admin.userId, token: admin.token });
await p.goto(ADMIN, { waitUntil: "domcontentloaded" });
await p.waitForTimeout(4000);

check("вкладка переименована в «Пользователи»",
  (await p.locator('[data-testid="tab-payments"]').innerText().catch(() => "")).includes("Пользователи"));

await p.locator('[data-testid="tab-payments"]').click();
await p.waitForTimeout(4000);
// ВАЖНО: берём строку СВОЕЙ жертвы, а не первую попавшуюся — иначе тест
// обнулит чужой аккаунт.
// Строку выбираем по идентификатору пользователя: в списке живут жертвы
// прошлых прогонов с похожими именами.
const row = p.locator(`[data-testid="no-access-row"][data-user-id="${victim.userId}"]`).first();
check("список пользователей не пуст", await p.locator('[data-testid="no-access-row"]').first().isVisible().catch(() => false));
check("строка жертвы найдена", await row.isVisible().catch(() => false), victim.userId);

await row.click();
const modal = p.locator('[data-testid="user-modal"]');
await modal.waitFor({ state: "visible", timeout: 15000 }).catch(() => {});
check("карточка пользователя открылась", await modal.isVisible().catch(() => false));
await p.waitForTimeout(2500);
const text = (await modal.innerText().catch(() => "")).replace(/\s+/g, " ");
console.log("  карточка:", text.slice(0, 220));
check("в карточке: время создания аккаунта", /создан \d{2}\.\d{2}\.\d{4}/.test(text));
check("в карточке: токены", /Токены · [1-9]/.test(text), text.match(/Токены · \d+/)?.[0] || "");
check("в карточке: оплата", /Оплата/.test(text) && /оплачен \d{2}\./.test(text));

// Медленные нажатия НЕ взводят кнопку.
const arm = p.locator('[data-testid="user-wipe-arm"]');
for (let i = 0; i < 4; i++) { await arm.click(); await p.waitForTimeout(700); }
check("медленные нажатия не взводят кнопку",
  !(await p.locator('[data-testid="wipe-confirm"]').isVisible().catch(() => false)),
  (await arm.innerText()).replace(/\s+/g, " "));

// 10 быстрых нажатий подряд → подтверждение.
for (let i = 0; i < 10; i++) { await arm.click({ delay: 0 }); await p.waitForTimeout(60); }
const confirm = p.locator('[data-testid="wipe-confirm"]');
await confirm.waitFor({ state: "visible", timeout: 5000 }).catch(() => {});
check("10 быстрых нажатий открыли подтверждение", await confirm.isVisible().catch(() => false));
const ctext = (await confirm.innerText().catch(() => "")).replace(/\s+/g, " ");
check("в подтверждении описано, что произойдёт",
  ctext.includes("подписка будет отменена") || ctext.includes("подписка") && ctext.includes("никогда не существовало"),
  ctext.slice(0, 160));

await p.locator('[data-testid="wipe-confirm-yes"]').click();
const report = p.locator('[data-testid="wipe-report"]');
await report.waitFor({ state: "visible", timeout: 30000 }).catch(() => {});
const rtext = (await report.innerText().catch(() => "")).replace(/\s+/g, " ");
console.log("  отчёт:", rtext.slice(0, 260));
check("показан пошаговый отчёт", rtext.includes("аккаунт, ключи и токены"));
check("в отчёте нет провалившихся шагов", !rtext.includes("✗"), rtext.slice(0, 200));
check("ошибка не показана", !(await p.locator('[data-testid="user-modal-error"]').isVisible().catch(() => false)));

// Сервер подтверждает, что пользователя больше нет.
{
  const r = await fetch(`${PAY}/admin/user-card?user_id=${victim.userId}`,
    { headers: { Authorization: `Bearer ${admin.token}` } });
  const card = await j(r);
  check("после обнуления: аккаунта нет", card?.auth?.exists === false);
  check("после обнуления: платежей нет", (card?.claims?.length ?? 0) === 0);
}

console.log(fail === 0 ? "\n=== ALL OK ===" : `\n=== FAILURES: ${fail} ===`);
await b.close();
process.exit(fail === 0 ? 0 : 1);
