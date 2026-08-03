// Мини-апп в Telegram обязан перестать предлагать «Получить доступ к re:Norma»
// после того, как человек СОЗДАЛ КЛЮЧ И ВОШЁЛ В ПРИЛОЖЕНИЕ.
//
// Кнопку выбирает hasAccess из /miniapp/me = auth-worker /internal/has-entered,
// то есть «в аккаунте отмечена доступная глава истории». Отметку ставит только
// само приложение (POST /chapters/available). Здесь прогоняется живой путь —
// оплата → код → создание ключа → приложение — и проверяется, что после него
// кнопка меняется. Раньше приложение эту отметку не ставило вовсе.
//
// Мини-апп вне Telegram проверяет подписанную initData: на dev токен бота —
// публичная заглушка из wrangler.toml, поэтому initData подписывается здесь.
import { chromium } from "playwright";
import crypto from "node:crypto";

const TG = process.env.TG_URL || "https://telegram-worker-dev.vg-stavenko.workers.dev";
const FE = process.env.FE || "https://renorma-fit-dev.pages.dev";
const AUTH = process.env.AUTH || "https://auth-worker-dev.vg-stavenko.workers.dev";
const PAY = process.env.PAY || "https://payment-worker-dev.vg-stavenko.workers.dev";
const MOCK = process.env.LAVA_MOCK || "https://lava-mock-dev.vg-stavenko.workers.dev";
const IKEY = process.env.INTERNAL_KEY || "dev-internal-push-key";
const BOT = process.env.BOT_TOKEN || "dev-telegram-bot-token";

let fail = 0;
const check = (name, ok, extra = "") => { console.log(`${ok ? "OK " : "FAIL"} ${name}${extra ? " — " + extra : ""}`); if (!ok) fail++; };
const j = async (r) => { const t = await r.text(); try { return JSON.parse(t); } catch { return { _raw: t }; } };
const post = (u, b, h = {}) => fetch(u, { method: "POST", headers: { "Content-Type": "application/json", ...h }, body: JSON.stringify(b) });

function initData(id, username) {
  const user = JSON.stringify({ id, username, first_name: "E2E" });
  const fields = { auth_date: String(Math.floor(Date.now() / 1000)), user };
  const dcs = Object.keys(fields).sort().map((k) => `${k}=${fields[k]}`).join("\n");
  const secret = crypto.createHmac("sha256", "WebAppData").update(BOT).digest();
  const hash = crypto.createHmac("sha256", secret).update(dcs).digest("hex");
  return new URLSearchParams({ ...fields, hash }).toString();
}

const tg = 860000 + Math.floor(Math.random() * 100000);
const username = `ab-${tg}`;
const { userId } = await j(await post(`${AUTH}/internal/account-resolve`,
  { provider: "telegram", providerUid: String(tg), username }, { "X-Internal-Key": IKEY }));
const co = await j(await post(`${PAY}/internal/checkout`,
  { tgUserId: tg, tgUsername: username, currency: "RUB", paymentMethod: "SBP" }, { "X-Internal-Key": IKEY }));
await post(`${MOCK}/pay/confirm`, { contractId: new URL(co.payUrl).searchParams.get("oid") });

const b = await chromium.launch({ headless: true });

// Открыть мини-апп и прочитать надпись на главной кнопке.
async function miniappButton() {
  const ctx = await b.newContext({ viewport: { width: 390, height: 844 } });
  const page = await ctx.newPage();
  await page.route("**/telegram-web-app.js", (r) => r.fulfill({ contentType: "application/javascript", body: "" }));
  await page.addInitScript((d) => {
    window.Telegram = { WebApp: {
      initData: d.initData, initDataUnsafe: { user: d.user },
      ready() {}, expand() {}, openLink(u) { window.__opened = u; },
      themeParams: {}, colorScheme: "light", setHeaderColor() {}, setBottomBarColor() {},
      onEvent() {}, offEvent() {},
    } };
  }, { initData: initData(tg, username), user: { id: tg, username, first_name: "E2E" } });
  await page.goto(TG, { waitUntil: "networkidle" });
  await page.waitForSelector("#access:not(.hidden)", { timeout: 20000 });
  await page.waitForTimeout(800);
  const text = (await page.textContent("#createBtn").catch(() => "")).trim();
  await ctx.close();
  return text;
}

// ── До входа в приложение: доступ ещё не получен ──
{
  const me = await j(await post(`${TG}/miniapp/me`, { initData: initData(tg, username) }));
  check("оплачено, но в приложение не входили: hasAccess=false", me.hasAccess === false, JSON.stringify(me.hasAccess));
  const btn = await miniappButton();
  check("кнопка предлагает получить доступ", btn === "Получить доступ к re:Norma", btn);
}

// ── Живой путь: код → создание ключа → приложение ──
{
  const { code } = await j(await post(`${AUTH}/internal/code/mint`, { userId }, { "X-Internal-Key": IKEY }));
  const ctx = await b.newContext({ viewport: { width: 430, height: 932 }, serviceWorkers: "block" });
  const p = await ctx.newPage();
  const cdp = await ctx.newCDPSession(p);
  await cdp.send("WebAuthn.enable");
  await cdp.send("WebAuthn.addVirtualAuthenticator", {
    options: { protocol: "ctap2", transport: "internal", hasResidentKey: true,
               hasUserVerification: true, isUserVerified: true, automaticPresenceSimulation: true },
  });
  await p.goto(`${FE}/onboard?u=${userId}#code=${code}`, { waitUntil: "domcontentloaded" });
  await p.waitForTimeout(14000);
  await p.locator('[data-testid="onboard-input-name"]').fill("Проверка");
  await p.locator('[data-testid="onboard-btn-register"]').click();
  await p.waitForTimeout(6000);
  check("ключ создан", await p.evaluate(() => !!localStorage.getItem("auth_token")));
  // Дальше пользователь попадает в приложение — именно оно отмечает вход.
  await p.evaluate(() => localStorage.setItem("pwa_dismissed", "1"));
  await p.goto(FE, { waitUntil: "domcontentloaded" });
  await p.waitForTimeout(20000);
  check("приложение открылось",
    /Главная|Дневник|Персона/.test(await p.locator("body").innerText().catch(() => "")));
  await ctx.close();
}

// ── После входа: мини-апп обязан показать «Открыть приложение» ──
{
  const entered = await j(await post(`${AUTH}/internal/has-entered`, { userId }, { "X-Internal-Key": IKEY }));
  check("приложение отметило вход на сервере", entered.entered === true, JSON.stringify(entered));
  const me = await j(await post(`${TG}/miniapp/me`, { initData: initData(tg, username) }));
  check("мини-апп видит доступ: hasAccess=true", me.hasAccess === true, JSON.stringify(me.hasAccess));
  const btn = await miniappButton();
  check("кнопка стала «Открыть приложение»", btn === "Открыть приложение", btn);
}

console.log(fail === 0 ? "\n=== ALL OK ===" : `\n=== FAILURES: ${fail} ===`);
await b.close();
process.exit(fail === 0 ? 0 : 1);
