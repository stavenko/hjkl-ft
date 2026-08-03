// Экран онбординга «создать ключ»: нажатие «Уже есть аккаунт? Войти» не должно
// подменять надпись на кнопке регистрации («Создаю…»). Вместо этого форма
// уходит с экрана и остаётся спиннер с надписью «Заходим в приложение».
//
// Церемонию входа подвешиваем (навсегда pending promise) — именно так выглядит
// на телефоне открытый системный диалог ключа, и именно это состояние проверяем.
import { chromium } from "playwright";
const FE = process.env.FE || "https://renorma-fit-dev.pages.dev";
const AUTH = process.env.AUTH || "https://auth-worker-dev.vg-stavenko.workers.dev";
const PAY = process.env.PAY || "https://payment-worker-dev.vg-stavenko.workers.dev";
const MOCK = process.env.LAVA_MOCK || "https://lava-mock-dev.vg-stavenko.workers.dev";
const K = process.env.INTERNAL_KEY || "dev-internal-push-key";
const CHROME = "Mozilla/5.0 (Linux; Android 15; Pixel 8) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/148.0.0.0 Mobile Safari/537.36";

let fail = 0;
const check = (name, ok, extra = "") => { console.log(`${ok ? "OK " : "FAIL"} ${name}${extra ? " — " + extra : ""}`); if (!ok) fail++; };
const j = async (r) => { const t = await r.text(); try { return JSON.parse(t); } catch { return { _raw: t }; } };
const post = (u, b, h = {}) => fetch(u, { method: "POST", headers: { "Content-Type": "application/json", ...h }, body: JSON.stringify(b) });

// Оплативший пользователь и свежий одноразовый код — как их выдаёт мини-апп.
const tg = 810000 + Math.floor(Math.random() * 100000);
let r = await post(`${AUTH}/internal/account-resolve`,
  { provider: "telegram", providerUid: String(tg), username: `si-${tg}` }, { "X-Internal-Key": K });
const { userId } = await j(r);
r = await post(`${PAY}/internal/checkout`,
  { tgUserId: tg, tgUsername: `si-${tg}`, currency: "RUB", paymentMethod: "SBP" }, { "X-Internal-Key": K });
const co = await j(r);
await post(`${MOCK}/pay/confirm`, { contractId: new URL(co.payUrl).searchParams.get("oid") });
r = await post(`${AUTH}/internal/code/mint`, { userId }, { "X-Internal-Key": K });
const { code } = await j(r);
const link = `${FE}/onboard?u=${userId}#code=${code}`;

const b = await chromium.launch({ headless: true });
const ctx = await b.newContext({ viewport: { width: 430, height: 932 }, userAgent: CHROME, serviceWorkers: "block" });
// В headless-Chromium нет платформенного аутентификатора, приложение честно
// пропустило бы шаг с ключом. Подставляем наличие — и подвешиваем саму церемонию.
await ctx.addInitScript(() => {
  if (window.PublicKeyCredential) {
    window.PublicKeyCredential.isUserVerifyingPlatformAuthenticatorAvailable = async () => true;
  }
  if (navigator.credentials) {
    navigator.credentials.get = () => new Promise(() => {});
  }
});
const p = await ctx.newPage();
await p.goto(link, { waitUntil: "domcontentloaded" });
await p.waitForTimeout(14000);

const regBtn = p.locator('[data-testid="onboard-btn-register"]');
const loginBtn = p.locator('[data-testid="onboard-btn-login"]');
const nameInput = p.locator('[data-testid="onboard-input-name"]');
const spinner = p.locator('[data-testid="onboard-signing-in"]');

check("экран создания ключа показан", await regBtn.isVisible().catch(() => false),
  (await p.locator("body").innerText().catch(() => "")).replace(/\s+/g, " ").slice(0, 80));
check("до нажатия спиннера нет", !(await spinner.isVisible().catch(() => false)));

await loginBtn.click();
await p.waitForTimeout(1200);

check("поле имени убрано", !(await nameInput.isVisible().catch(() => false)));
check("кнопка регистрации убрана", !(await regBtn.isVisible().catch(() => false)));
check("кнопка «Войти» убрана", !(await loginBtn.isVisible().catch(() => false)));
check("показан спиннер", await spinner.isVisible().catch(() => false));
check("спиннер крутится (есть анимация)",
  await spinner.locator(".ft-spinner").isVisible().catch(() => false));
const text = (await spinner.innerText().catch(() => "")).trim();
check("надпись «Заходим в приложение»", text === "Заходим в приложение", text);
const body = (await p.locator("body").innerText().catch(() => "")).replace(/\s+/g, " ");
check("надписи «Создаю…» на экране нет", !/Создаю/.test(body), body.slice(0, 90));

console.log(fail === 0 ? "\n=== ALL OK ===" : `\n=== FAILURES: ${fail} ===`);
await b.close();
process.exit(fail === 0 ? 0 : 1);
