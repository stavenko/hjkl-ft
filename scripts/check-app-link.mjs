// Ссылка «Открыть приложение» из Mini App: корень С АККАУНТОМ — `/?u=<user_id>`.
//
// Её работа одна: человек открывает её в браузере и СРАЗУ ставит ярлык на рабочий
// стол — так возвращают удалённую иконку. Значит от чистого браузера, где нет ни
// сессии, ни кода, требуется:
//   1. в HTML стоит <link rel="manifest" href="/manifest.json?u=…">, иначе
//      «На экран „Домой“» снимет общий манифест со start_url "/" и поставленное
//      приложение потеряет аккаунт;
//   2. манифест по этому адресу и правда персональный (start_url и id с ?u);
//   3. на экране — инструкция по установке, без всякого онбординга и входа.
//
// Для сравнения проверяется голый корень: там манифест обязан остаться общим.
import { chromium, devices } from "playwright";

const BASE = process.env.FE || "https://renorma-fit-dev.pages.dev";
const UID = "demo-user-123";

let fail = 0;
const check = (n, ok, extra = "") => {
  console.log(`${ok ? "OK  " : "FAIL"} ${n}${extra ? " — " + extra : ""}`);
  if (!ok) fail++;
};

const b = await chromium.launch({ headless: true });

// ── ссылка из Mini App, чистый браузер ──────────────────────────────────────
{
  // Ссылку открывают С ТЕЛЕФОНА — оттуда её и присылает Mini App. Браузер без
  // телефонного User-Agent попадает в ДЕСКТОПНУЮ ветку экрана установки, где
  // вместо инструкции стоит «приложение предназначено для мобильных устройств»
  // (см. pwa_prompt.rs): ставить его на компьютер незачем.
  const context = await b.newContext({ ...devices["iPhone 13"], serviceWorkers: "block" });
  const page = await context.newPage();
  await page.goto(`${BASE}/?u=${UID}`, { waitUntil: "domcontentloaded" });
  await page.waitForTimeout(10000);

  const href = await page.getAttribute('link[rel="manifest"]', "href");
  check("ссылка на манифест персональная", href === `/manifest.json?u=${UID}`, String(href));

  const mf = await page.evaluate(async (h) => (await fetch(h)).json(), href);
  check("манифест ведёт в аккаунт", mf.start_url === `/?u=${UID}` && mf.id === `/app-${UID}`,
    `${mf.start_url} · ${mf.id}`);

  // Признак экрана установки — САМА ИНСТРУКЦИЯ, пронумерованные шаги. Кнопка
  // «Продолжить в браузере» для этого не годится: она есть только в десктопной
  // ветке, а с телефона выхода из инструкции нет — приложение обязано стоять
  // как PWA.
  const steps = await page.locator(".steps .step-num").count();
  check("сразу показана установка", steps > 0, `шагов инструкции ${steps}`);
  const text = (await page.locator("body").innerText()).replace(/\s+/g, " ");
  check("установка, а не онбординг и не «ссылка устарела»",
    text.includes("установить на рабочий стол") && !text.includes("Ссылка устарела"),
    text.slice(0, 90));

  await context.close();
}

// ── голый корень — манифест общий ───────────────────────────────────────────
{
  const context = await b.newContext({ serviceWorkers: "block" });
  const page = await context.newPage();
  await page.goto(BASE, { waitUntil: "domcontentloaded" });
  await page.waitForTimeout(6000);
  const href = await page.getAttribute('link[rel="manifest"]', "href");
  check("без ?u манифест остаётся общим", !String(href).includes("u="), String(href));
  await context.close();
}

await b.close();
console.log(fail ? `\n${fail} провалов` : "\nвсё сошлось");
process.exit(fail ? 1 : 0);
