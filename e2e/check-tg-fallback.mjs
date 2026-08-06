// Экран входа установленного PWA: что происходит, когда ключ не сработал.
//
// В headless-браузере credentials.get отказывает сам (аутентификатора нет) —
// это и есть проверяемый случай: отказ хранилища, а не сети. При сетевом отказе
// обходной путь не предлагается, и это тоже правильное поведение, поэтому такой
// прогон засчитывается отдельно, а не как провал.
//
// Прогон: node check-tg-fallback.mjs
import { chromium } from "playwright";

const BASE = "https://renorma-fit-dev.pages.dev";
const PAID = "pk-state-test"; // оплачен, в приложение не входил
const UNKNOWN = "11111111-2222-3333-4444-555555555555"; // не платил

let failed = 0;
const check = (ok, what) => {
  console.log(`${ok ? "  ok  " : "  ПРОВАЛ "} ${what}`);
  if (!ok) failed++;
};

async function openAuth(browser, uid) {
  const ctx = await browser.newContext();
  const page = await ctx.newPage();
  await page.goto(`${BASE}/?u=${uid}`, { waitUntil: "domcontentloaded" });
  // Установленное приложение сюда попадает уже мимо подсказки об установке;
  // в браузере она первая, поэтому закрываем её и попадаем на экран входа.
  await page.getByTestId("pwa-btn-dismiss").click({ timeout: 40000 });
  await page.getByTestId("auth-btn-try-passkey").waitFor({ timeout: 40000 });
  return { ctx, page };
}

/// Сколько раз страница ходила за кодом. Читается из журнала ресурсов самой
/// страницы: постоянный слушатель запросов заставляет Playwright ждать
/// незавершённых навигаций, и все последующие ожидания зависают.
const codeRequestCount = (page) =>
  page.evaluate(() =>
    performance
      .getEntriesByType("resource")
      .filter((e) => e.name.includes("/code/request")).length,
  );

/// Нажать «Войти» и дождаться исхода. Возвращает текст ошибки.
async function failLogin(page) {
  // Первое нажатие иногда приходится на момент, когда обработчики Leptos ещё не
  // навешаны, и пропадает без следа. Признак того, что нажатие принято, —
  // появившаяся плашка ошибки; пока её нет, жмём ещё раз.
  for (let attempt = 0; attempt < 6; attempt++) {
    await page.getByTestId("auth-btn-try-passkey").click({ timeout: 20000 });
    const shown = await page
      .getByTestId("auth-error")
      .waitFor({ timeout: 10000 })
      .then(() => true)
      .catch(() => false);
    if (shown) {
      return (await page.getByTestId("auth-error").innerText()).trim();
    }
  }
  console.log("   НЕ ДОЖДАЛИСЬ ошибки. На экране:");
  console.log("   текст:", (await page.innerText("body")).slice(0, 400).replace(/\n+/g, " | "));
  throw new Error("нажатие «Войти» не дало ни входа, ни ошибки");
}

const isNetwork = (msg) => /связаться с сервером|интернет/i.test(msg);

async function run() {
  const browser = await chromium.launch();

  // ── 1. Оплаченный аккаунт: отказ ключа → предложение Telegram → экран кода ──
  {
    const { ctx, page } = await openAuth(browser, PAID);
    const msg = await failLogin(page);
    console.log("\n1. Оплачен, в приложение не входил");
    console.log("   сообщение об отказе:", JSON.stringify(msg));
    check(!/отменен/i.test(msg), "отказ не назван отменой пользователя");

    if (isNetwork(msg)) {
      // Сеть отвалилась — обходной путь и не должен предлагаться.
      check(
        !(await page.getByTestId("auth-tg-offer").isVisible().catch(() => false)),
        "при сетевом отказе Telegram не предлагается",
      );
      console.log("   (сетевой отказ — остальные проверки этого блока пропущены)");
    } else {
      await page.getByTestId("auth-tg-offer").waitFor({ timeout: 10000 });
      check(true, "показан блок «Войти через Telegram»");

      await page.getByTestId("auth-btn-tg-login").click();
      await page.getByTestId("codeauth-btn-send").waitFor({ timeout: 40000 });
      check(true, "перешли на экран кода");
      await page.waitForTimeout(3000);
      const n = await codeRequestCount(page);
      check(n === 1, `код запрошен сам, ровно один раз (запросов: ${n})`);
      check(await page.getByTestId("auth-link-site").isVisible(), "внизу ссылка renorma.app");
    }
    await ctx.close();
  }

  // ── 2. Аккаунт без оплаты: тот же путь ведёт в бота ──
  {
    const { ctx, page } = await openAuth(browser, UNKNOWN);
    const msg = await failLogin(page);
    console.log("\n2. Не оплачен");
    if (isNetwork(msg)) {
      console.log("   (сетевой отказ — блок пропущен)");
    } else {
      await page.getByTestId("auth-btn-tg-login").click({ timeout: 20000 });
      await page.getByTestId("auth-link-bot").waitFor({ timeout: 40000 });
      const href = await page.getByTestId("auth-link-bot").getAttribute("href");
      check(/t\.me\//.test(href), `ссылка на бота: ${href}`);
      check(
        !(await page.getByTestId("codeauth-btn-send").isVisible().catch(() => false)),
        "поля ввода кода нет — платить, а не входить",
      );
      check(await page.getByTestId("auth-link-site").isVisible(), "внизу ссылка renorma.app");
    }
    await ctx.close();
  }

  await browser.close();
  console.log(failed ? `\nПРОВАЛОВ: ${failed}` : "\nвсё сошлось");
  process.exit(failed ? 1 : 0);
}

run();
