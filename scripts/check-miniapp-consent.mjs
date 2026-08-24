// СОГЛАСИЯ В МИНИ-АППКЕ ОПЛАТЫ — на живом воркере.
//
// Платёж происходит ЗДЕСЬ, а не на лендинге: в бота попадают и мимо сайта. Значит
// «Оплатить» обязана быть закрыта, пока человек не принял оферту и политику.
//
// Проверяется ПОВЕДЕНИЕ: мастер проходится до последнего шага, дальше смотрим саму
// кнопку. Счёт при этом создаётся настоящий (шаг «промокод» минтит инвойс), поэтому
// по умолчанию прогон идёт против DEV-воркера с мок-lava.
//
//   node scripts/check-miniapp-consent.mjs
//   MINIAPP=https://tg.renorma.app node scripts/check-miniapp-consent.mjs   # прод, только чтение
import { chromium } from "playwright";
import { createHmac } from "node:crypto";

const BASE = process.env.MINIAPP || "https://telegram-worker-dev.vg-stavenko.workers.dev";
// Тем же токеном, что у dev-воркера: без подписанного initData мини-аппка показывает
// заглушку «только внутри Telegram», а серверные ручки отвечают 401.
const BOT_TOKEN = process.env.TELEGRAM_BOT_TOKEN || "dev-telegram-bot-token";

/// Подписанный initData — как его прислал бы настоящий Telegram.
function signInitData(user) {
  const fields = {
    user: JSON.stringify(user),
    auth_date: String(Math.floor(Date.now() / 1000)),
    query_id: "AAacceptance",
  };
  const check = Object.keys(fields).sort().map((k) => `${k}=${fields[k]}`).join("\n");
  const secret = createHmac("sha256", "WebAppData").update(BOT_TOKEN).digest();
  const hash = createHmac("sha256", secret).update(check).digest("hex");
  return new URLSearchParams({ ...fields, hash }).toString();
}

let failed = 0;
const check = (what, ok, detail = "") => {
  console.log(`${ok ? "✅" : "❌"} ${what}${detail ? ` — ${detail}` : ""}`);
  if (!ok) failed++;
};

const browser = await chromium.launch({ headless: true });
const ctx = await browser.newContext({ viewport: { width: 430, height: 932 } });
const page = await ctx.newPage();

// Telegram.WebApp подделывается ДО загрузки: без него мини-аппка не знает, кто пришёл,
// и на первом же запросе получает отказ. initData пустой — dev-воркер его не требует.
const user = { id: 777000, username: "consent_probe", first_name: "Consent" };
const initData = signInitData(user);
// Подменяем САМ SDK: страница грузит telegram-web-app.js от telegram.org, и он
// перетирает любую заглушку, поставленную до него. Отдаём вместо него свой — с
// подписанным initData и подслушанным openLink (по нему видно, что документы
// уходят в системный браузер, а не открываются внутри мини-аппки).
await page.route("**/telegram-web-app.js", (route) =>
  route.fulfill({
    contentType: "application/javascript",
    body: `window.Telegram = { WebApp: {
      initData: ${JSON.stringify(initData)},
      initDataUnsafe: { user: ${JSON.stringify(user)} },
      ready() {}, expand() {}, close() {},
      openLink(u) { (window.__opened = window.__opened || []).push(u); },
      MainButton: { hide() {}, show() {}, setText() {}, onClick() {} },
      themeParams: {}, colorScheme: "light",
      setHeaderColor() {}, setBottomBarColor() {},
    } };`,
  }));
await page.goto(BASE, { waitUntil: "domcontentloaded" });
await page.waitForTimeout(2500);

const next = page.locator("#nextBtn");
check("мастер открылся", await next.isVisible());

// Шаг 1 — карта. Российская: дальше валюта не спрашивается.
// Кнопки выбора перерисовываются скриптом (`renderOptions`), и data-атрибутов у них
// не остаётся — жмём по подписи.
await page.locator("#worldSel button", { hasText: "Российская карта" }).click();
await next.click();
await page.waitForTimeout(1200);

// Шаг «промокод» → «Далее» создаёт счёт; ждём последний шаг.
await next.click();
await page.waitForSelector('.step[data-step="final"]:not(.hidden)', { timeout: 60000 });
await page.waitForTimeout(800);
check("дошли до шага «Проверьте заказ»",
  await page.locator('.step[data-step="final"]').isVisible());

const cOffer = page.locator("#cOffer");
const cPrivacy = page.locator("#cPrivacy");
check("галочка оферты есть", await cOffer.count() === 1);
check("галочка политики есть", await cPrivacy.count() === 1);
check("кнопка называется «Оплатить»", (await next.textContent())?.trim() === "Оплатить");

check("без галочек «Оплатить» закрыта", await next.isDisabled());
await cOffer.check();
await page.waitForTimeout(200);
check("одной галочки мало", await next.isDisabled());

await cPrivacy.check();
await page.waitForTimeout(200);
check("с обеими «Оплатить» открыта", !(await next.isDisabled()));

// Снять обратно — снова закрыта: гейт живой, а не одноразовый.
await cOffer.uncheck();
await page.waitForTimeout(200);
check("снятая галочка снова закрывает кнопку", await next.isDisabled());

// Ссылки: те самые страницы и системным браузером.
for (const [sel, want] of [["#cOffer", "https://renorma.app/offer"],
                           ["#cPrivacy", "https://renorma.app/privacy"]]) {
  const href = await page.locator(`${sel} ~ span a`).getAttribute("href");
  check(`ссылка ведёт на ${want}`, href === want, href ?? "нет ссылки");
}
await page.locator("#cOffer ~ span a").click();
await page.waitForTimeout(300);
const opened = await page.evaluate(() => window.__opened || []);
check("документ открывается системным браузером (openLink)",
  opened.includes("https://renorma.app/offer"), JSON.stringify(opened));

// Возврат на шаг назад и обратно — согласие спрашивается заново.
await cOffer.check();
await cPrivacy.check();
await page.waitForTimeout(200);
await page.locator("#backBtn").click();
await page.waitForTimeout(600);
await next.click();
await page.waitForSelector('.step[data-step="final"]:not(.hidden)', { timeout: 60000 });
await page.waitForTimeout(600);
check("после возврата галочки сброшены",
  !(await cOffer.isChecked()) && !(await cPrivacy.isChecked()));
check("после возврата «Оплатить» снова закрыта", await next.isDisabled());

await ctx.close();
await browser.close();
process.exit(failed ? 1 : 0);
