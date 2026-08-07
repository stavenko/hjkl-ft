// Какой экран получает каждый браузер.
//
// Главное здесь — НЕ сломать Яндекс: его путь выстрадан замерами (intent оттуда
// не работает, поэтому там учат листу «Поделиться»). Mi добавлен рядом, и этот
// прогон следит, что рядом, а не вместо.
import { chromium } from "playwright";

const BASE = process.env.BASE || "https://renorma-fit-dev.pages.dev";

const UAS = {
  mi:
    "Mozilla/5.0 (Linux; Android 15; REDMI 15C Build/AP3A.240905.015.A2) AppleWebKit/537.36 " +
    "(KHTML, like Gecko) Chrome/135.0.7049.79 Mobile Safari/537.36 XiaoMi/MiuiBrowser/14.60.0-gn",
  yandex:
    "Mozilla/5.0 (Linux; Android 15; SM-A155F) AppleWebKit/537.36 (KHTML, like Gecko) " +
    "Chrome/135.0.0.0 YaBrowser/25.4.1.100 Mobile Safari/537.36",
  chrome:
    "Mozilla/5.0 (Linux; Android 15; Pixel 8) AppleWebKit/537.36 (KHTML, like Gecko) " +
    "Chrome/135.0.0.0 Mobile Safari/537.36",
};

let failed = 0;
const check = (ok, what) => {
  console.log(`${ok ? "  ok  " : "  ПРОВАЛ "} ${what}`);
  if (!ok) failed++;
};

const b = await chromium.launch();

async function open(ua) {
  const ctx = await b.newContext({
    userAgent: ua,
    viewport: { width: 390, height: 844 },
    isMobile: true,
    hasTouch: true,
  });
  const page = await ctx.newPage();
  await page.goto(`${BASE}/?u=00000000-1111-2222-3333-444444444444`, {
    waitUntil: "domcontentloaded",
  });
  await page.waitForTimeout(9000);
  return { ctx, page };
}

// ── 1. Mi Browser: свой экран с кнопкой-intent ──
{
  const { ctx, page } = await open(UAS.mi);
  console.log("\n1. Mi Browser");
  const shown = await page.getByTestId("pwa-mi-screen").isVisible().catch(() => false);
  check(shown, "показан экран Mi");
  if (shown) {
    const text = await page.innerText("body");
    check(
      /Приложение re:Norma работает в браузере Chrome\./.test(text),
      "фраза ровно та, что просили",
    );
    // Ничего сверх фразы и кнопки: сочинённые пояснения тут не нужны.
    check(!/ключ доступа|не установлен/.test(text), "лишнего текста нет");
    const href = await page.getByTestId("pwa-btn-open-chrome").getAttribute("href");
    check(/^intent:\/\//.test(href), "кнопка — intent-ссылка");
    check(/package=com\.android\.chrome/.test(href), "intent целится в Chrome");
    check(/[?&]u=00000000/.test(href), "адрес несёт ?u — аккаунт не потеряется");
    check(
      !(await page.getByTestId("pwa-yandex-screen").isVisible().catch(() => false)),
      "яндексовский экран сюда не подмешался",
    );
  }
  await ctx.close();
}

// ── 2. Яндекс: ровно прежний экран, ничего нового ──
{
  const { ctx, page } = await open(UAS.yandex);
  console.log("\n2. Яндекс.Браузер");
  check(
    await page.getByTestId("pwa-yandex-screen").isVisible().catch(() => false),
    "показан прежний экран Яндекса",
  );
  const shots = await page.$$eval("img", (els) => els.map((e) => e.getAttribute("src")));
  check(shots.includes("/onboard-img/hop-share.gif"), "гифка «Поделиться» на месте");
  check(shots.includes("/onboard-img/hop-chrome.gif"), "гифка «выбрать Chrome» на месте");
  check(
    !(await page.getByTestId("pwa-mi-screen").isVisible().catch(() => false)),
    "экран Mi сюда не подмешался",
  );
  check(
    !(await page.getByTestId("pwa-btn-open-chrome").isVisible().catch(() => false)),
    "intent-кнопки у Яндекса нет — оттуда она не работает",
  );
  await ctx.close();
}

// ── 3. Обычный Chrome на Android: прежняя инструкция по установке ──
{
  const { ctx, page } = await open(UAS.chrome);
  console.log("\n3. Chrome на Android");
  const text = await page.innerText("body");
  check(/Как установить на Android/.test(text), "показана инструкция по установке");
  check(
    !(await page.getByTestId("pwa-mi-screen").isVisible().catch(() => false)),
    "экран Mi сюда не подмешался",
  );
  await ctx.close();
}

await b.close();
console.log(failed ? `\nПРОВАЛОВ: ${failed}` : "\nвсё сошлось");
process.exit(failed ? 1 : 0);
