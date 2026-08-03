// «После создания ключа и получения доступа мини-апп всё равно показывает
// „Получить доступ к re:Norma“». Кнопку выбирает hasAccess из /miniapp/me,
// который = auth-worker /internal/has-entered (в аккаунте отмечена хотя бы одна
// доступная глава истории). Проходим ЖИВОЙ путь: оплата → код → создание ключа
// → приложение, и смотрим, что говорит has-entered.
import { chromium } from "playwright";
const FE = process.env.FE || "https://renorma-fit-dev.pages.dev";
const AUTH = process.env.AUTH || "https://auth-worker-dev.vg-stavenko.workers.dev";
const PAY = process.env.PAY || "https://payment-worker-dev.vg-stavenko.workers.dev";
const MOCK = process.env.LAVA_MOCK || "https://lava-mock-dev.vg-stavenko.workers.dev";
const K = process.env.INTERNAL_KEY || "dev-internal-push-key";
const j = async (r) => JSON.parse(await r.text());
const post = (u, b, h = {}) => fetch(u, { method: "POST", headers: { "Content-Type": "application/json", ...h }, body: JSON.stringify(b) });

const tg = 850000 + Math.floor(Math.random() * 100000);
const { userId } = await j(await post(`${AUTH}/internal/account-resolve`,
  { provider: "telegram", providerUid: String(tg), username: `ma-${tg}` }, { "X-Internal-Key": K }));
const co = await j(await post(`${PAY}/internal/checkout`,
  { tgUserId: tg, tgUsername: `ma-${tg}`, currency: "RUB", paymentMethod: "SBP" }, { "X-Internal-Key": K }));
await post(`${MOCK}/pay/confirm`, { contractId: new URL(co.payUrl).searchParams.get("oid") });
const { code } = await j(await post(`${AUTH}/internal/code/mint`, { userId }, { "X-Internal-Key": K }));

const b = await chromium.launch({ headless: true });
const ctx = await b.newContext({ viewport: { width: 430, height: 932 }, serviceWorkers: "block" });
const p = await ctx.newPage();
const logs = [];
p.on("console", (m) => logs.push(`${m.type()}: ${m.text().slice(0, 200)}`));
p.on("requestfinished", async (r) => {
  if (r.url().includes("/chapters/available")) {
    const resp = await r.response();
    logs.push(`REQ chapters/available -> HTTP ${resp && resp.status()}`);
  }
});
const cdp = await ctx.newCDPSession(p);
await cdp.send("WebAuthn.enable");
await cdp.send("WebAuthn.addVirtualAuthenticator", {
  options: { protocol: "ctap2", transport: "internal", hasResidentKey: true,
             hasUserVerification: true, isUserVerified: true, automaticPresenceSimulation: true },
});
await p.goto(`${FE}/onboard?u=${userId}#code=${code}`, { waitUntil: "domcontentloaded" });
await p.waitForTimeout(14000);

// Создаём ключ ровно как пользователь: имя → «Зарегистрироваться».
await p.locator('[data-testid="onboard-input-name"]').fill("Проверка");
await p.locator('[data-testid="onboard-btn-register"]').click();
await p.waitForTimeout(6000);
const afterKey = (await p.locator("body").innerText().catch(() => "")).replace(/\s+/g, " ").slice(0, 80);

// Уходим в приложение и даём ему прожиться (истории, главы и т.п.).
await p.evaluate(() => localStorage.setItem("pwa_dismissed", "1"));
await p.goto(FE, { waitUntil: "domcontentloaded" });
await p.waitForTimeout(20000);
const inApp = (await p.locator("body").innerText().catch(() => "")).replace(/\s+/g, " ").slice(0, 100);
const authed = await p.evaluate(() => !!localStorage.getItem("auth_token"));

const entered = await j(await post(`${AUTH}/internal/has-entered`, { userId }, { "X-Internal-Key": K }));
console.log(JSON.stringify({ userId, afterKey, inApp, authed, entered }, null, 2));
console.log("--- интересное из консоли:");
console.log(logs.filter((l) => /chapters|report_entered|error/i.test(l)).slice(0, 10).join("\n") || "(ничего)");
console.log("сборка:", await (await fetch(`${FE}/version.json?cb=${Math.random()}`)).text());
await b.close();
