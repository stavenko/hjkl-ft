// Путь из мини-аппа: ссылка с одноразовым кодом открывается в Яндексе, оттуда
// пользователь уходит в Chrome. Код обязан ДОЖИТЬ до Chrome — в Яндексе не должно
// выполняться ничего, кроме экрана перехода. Дальше в Chrome: сперва ключ, потом PWA.
import { chromium } from "playwright";
const FE = process.env.FE || "https://renorma-fit-dev.pages.dev";
const AUTH = "https://auth-worker-dev.vg-stavenko.workers.dev";
const PAY = "https://payment-worker-dev.vg-stavenko.workers.dev";
const MOCK = process.env.LAVA_MOCK || "https://lava-mock-dev.vg-stavenko.workers.dev";
const K = process.env.INTERNAL_KEY || "dev-internal-push-key";
const YA = "Mozilla/5.0 (Linux; arm_64; Android 15; 25078RA3EY) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/148.0.7778.104 YaBrowser/26.6.5.104.00 Mobile Safari/537.36";
const CHROME = "Mozilla/5.0 (Linux; Android 15; Pixel 8) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/148.0.0.0 Mobile Safari/537.36";

let fail = 0;
const check = (name, ok, extra = "") => { console.log(`${ok ? "OK " : "FAIL"} ${name}${extra ? " — " + extra : ""}`); if (!ok) fail++; };
const j = async (r) => { const t = await r.text(); try { return JSON.parse(t); } catch { return { _raw: t }; } };
const post = (u, b, h = {}) => fetch(u, { method: "POST", headers: { "Content-Type": "application/json", ...h }, body: JSON.stringify(b) });

// Оплативший пользователь + свежий код, как их выдаёт мини-апп.
const tg = 800000 + Math.floor(Math.random() * 100000);
let r = await post(`${AUTH}/internal/account-resolve`,
  { provider: "telegram", providerUid: String(tg), username: `hop-${tg}` }, { "X-Internal-Key": K });
const { userId } = await j(r);
r = await post(`${PAY}/internal/checkout`,
  { tgUserId: tg, tgUsername: `hop-${tg}`, currency: "RUB", paymentMethod: "SBP" }, { "X-Internal-Key": K });
const co = await j(r);
await post(`${MOCK}/pay/confirm`, { contractId: new URL(co.payUrl).searchParams.get("oid") });
r = await post(`${AUTH}/internal/code/mint`, { userId }, { "X-Internal-Key": K });
const { code } = await j(r);
// Ровно та ссылка, которую открывает кнопка «Получить доступ к re:Norma»:
// user_id в запросе, одноразовый код — во ФРАГМЕНТЕ.
const link = `${FE}/onboard?u=${userId}#code=${code}`;
console.log("ссылка:", link.replace(code, "<код>"));

const b = await chromium.launch({ headless: true });

// ── 1. Яндекс: только экран перехода, никакой работы приложения ──
{
  const ctx = await b.newContext({ viewport: { width: 430, height: 932 }, userAgent: YA, serviceWorkers: "block" });
  const p = await ctx.newPage();
  p.on("console", (m) => { if (/panicked/.test(m.text())) console.log("[ya]", m.text().slice(0, 160)); });
  await p.goto(link, { waitUntil: "domcontentloaded" });
  await p.waitForTimeout(12000);
  check("Яндекс: показан экран перехода в Chrome",
    await p.locator('[data-testid="pwa-yandex-screen"]').isVisible().catch(() => false));
  check("Яндекс: страница онбординга НЕ отрисована",
    !(await p.locator('[data-testid="onboard-btn-register"]').isVisible().catch(() => false))
      && !(await p.locator('[data-testid="onboard-verifying"]').isVisible().catch(() => false)));
  check("Яндекс: сессия не создана", !(await p.evaluate(() => !!localStorage.getItem("auth_token"))));
  await ctx.close();
}

// Главное: код не сгорел в Яндексе.
{
  const probe = await post(`${AUTH}/code/verify`, { userId, code: "000000" });
  check("контроль: неверный код отвергается", !probe.ok, `HTTP ${probe.status}`);
}

// ── 2. Chrome: код принимается, сперва ключ, потом установка PWA ──
{
  const ctx = await b.newContext({ viewport: { width: 430, height: 932 }, userAgent: CHROME, serviceWorkers: "block" });
  // На настоящем телефоне с блокировкой экрана платформенный аутентификатор есть;
  // в headless-Chromium его нет, поэтому подставляем — иначе приложение честно
  // решит, что ключ невозможен, и пропустит шаг.
  await ctx.addInitScript(() => {
    if (window.PublicKeyCredential) {
      window.PublicKeyCredential.isUserVerifyingPlatformAuthenticatorAvailable = async () => true;
    }
  });
  const p = await ctx.newPage();
  p.on("console", (m) => { if (/panicked/.test(m.text())) console.log("[chrome]", m.text().slice(0, 160)); });
  await p.goto(link, { waitUntil: "domcontentloaded" });
  await p.waitForTimeout(14000);
  const text = (await p.locator("body").innerText().catch(() => "")).replace(/\s+/g, " ");
  const stale = text.includes("Ссылка устарела");
  check("Chrome: ссылка НЕ протухла (код дожил из Яндекса)", !stale, text.slice(0, 90));
  check("Chrome: предложено создать ключ",
    await p.locator('[data-testid="onboard-btn-register"]').isVisible().catch(() => false), text.slice(0, 90));
  check("Chrome: сессия создана кодом", await p.evaluate(() => !!localStorage.getItem("auth_token")));
  check("Chrome: инструкция установки ещё НЕ показана (сначала ключ)",
    !text.includes("Нажмите на кебаб"));
  await ctx.close();
}

console.log(fail === 0 ? "\n=== ALL OK ===" : `\n=== FAILURES: ${fail} ===`);
await b.close();
process.exit(fail === 0 ? 0 : 1);
