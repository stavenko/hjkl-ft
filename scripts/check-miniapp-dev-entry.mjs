// Отладочный вход на тестовую версию в мини-аппе: взводится ДЕСЯТЬЮ тапами по
// строке версии (между тапами < 500 мс), затем предупреждение и уход на смоук.
// Мини-апп отдаётся телеграм-воркером; вне Telegram он показывает заглушку,
// поэтому страницу грузим и проверяем саму механику на её разметке.
import { chromium } from "playwright";
const MINIAPP = process.env.MINIAPP || "https://tg-dev.renorma.app/miniapp";

let fail = 0;
const check = (name, ok, extra = "") => { console.log(`${ok ? "OK " : "FAIL"} ${name}${extra ? " — " + extra : ""}`); if (!ok) fail++; };

const b = await chromium.launch({ headless: true });
const ctx = await b.newContext({ viewport: { width: 430, height: 932 } });
const p = await ctx.newPage();
// Страница подгружает настоящий SDK Telegram, который вне мессенджера отдаёт
// пустой initData и уводит в заглушку. Подменяем сам скрипт — иначе клиентскую
// механику не проверить. Проверка initData на сервере от этого не страдает.
await ctx.route("**/telegram-web-app.js", (route) =>
  route.fulfill({
    status: 200,
    contentType: "application/javascript",
    body: 'window.Telegram={WebApp:{ready(){},expand(){},initData:"probe",initDataUnsafe:{},' +
          'openLink(u){window.__opened=u;},themeParams:{},colorScheme:"light",' +
          'setHeaderColor(){},setBottomBarColor(){},onEvent(){},offEvent(){}}};',
  }));
const resp = await p.goto(MINIAPP, { waitUntil: "domcontentloaded" });
check("мини-апп отдаётся", resp && resp.ok(), `HTTP ${resp && resp.status()}`);
await p.waitForTimeout(2500);

const tag = p.locator("#buildTag");
const btn = p.locator("#devSmokeBtn");
check("строка версии на месте", await tag.isVisible().catch(() => false),
  (await tag.innerText().catch(() => "")).trim());
check("кнопка тестовой версии скрыта", !(await btn.isVisible().catch(() => false)));

// Медленные тапы не взводят.
for (let i = 0; i < 6; i++) { await tag.click({ force: true }); await p.waitForTimeout(700); }
check("медленные тапы не открывают кнопку", !(await btn.isVisible().catch(() => false)));

// Десять быстрых — взводят.
for (let i = 0; i < 10; i++) { await tag.click({ force: true }); await p.waitForTimeout(60); }
check("десять быстрых тапов открыли кнопку", await btn.isVisible().catch(() => false));
check("кнопка красная", (await btn.evaluate((el) => getComputedStyle(el).backgroundColor)) === "rgb(224, 48, 79)",
  await btn.evaluate((el) => getComputedStyle(el).backgroundColor));
check("надпись на кнопке", (await btn.innerText()).includes("тестовой версии"), await btn.innerText());

// Предупреждение и «перевёрнутые» кнопки.
await btn.click();
const warn = p.locator("#devWarn");
check("показано предупреждение", await warn.isVisible().catch(() => false));
const wtext = (await warn.innerText().catch(() => "")).replace(/\s+/g, " ");
check("текст предупреждения на месте",
  wtext.includes("служит для отладки приложения") && wtext.includes("Если вы разработчик"),
  wtext.slice(0, 90));
const goBg = await p.locator("#devWarnGo").evaluate((el) => getComputedStyle(el).backgroundColor);
const cancelBg = await p.locator("#devWarnCancel").evaluate((el) => getComputedStyle(el).backgroundImage
  + " | " + getComputedStyle(el).backgroundColor);
check("«Продолжить» выглядит второстепенной (прозрачная)", goBg === "rgba(0, 0, 0, 0)", goBg);
check("«Отменить» выглядит основной (синяя заливка)", cancelBg.includes("gradient"), cancelBg.slice(0, 60));

// Отмена закрывает.
await p.locator("#devWarnCancel").click();
await p.waitForTimeout(300);
check("отмена закрывает предупреждение", !(await warn.isVisible().catch(() => false)));

console.log(fail === 0 ? "\n=== ALL OK ===" : `\n=== FAILURES: ${fail} ===`);
await b.close();
process.exit(fail === 0 ? 0 : 1);
