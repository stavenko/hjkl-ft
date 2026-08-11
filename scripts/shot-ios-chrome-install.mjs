// Снимок экрана установки, каким его видит Chrome на iPhone.
//
// Ветка `ios_chrome` выбирается по User-Agent, поэтому проверять её надо ИМЕННО
// с этим UA: под обычным браузером рисуется другая инструкция. Заодно
// проверяется, что оба снимка шагов реально отдаются (404 у картинки на экране
// установки — это молча пустая инструкция).
import { chromium } from "playwright";

const BASE = process.env.FE || "https://renorma-fit-dev.pages.dev";
const OUT = process.env.SHOT || "/tmp/ios-chrome-install.png";
const UID = "demo-user-123";
// Chrome на iOS: WebKit внутри, «CriOS» в UA.
const UA = "Mozilla/5.0 (iPhone; CPU iPhone OS 18_5 like Mac OS X) AppleWebKit/605.1.15 " +
  "(KHTML, like Gecko) CriOS/137.0.7151.107 Mobile/15E148 Safari/604.1";

const b = await chromium.launch({ headless: true });
const context = await b.newContext({
  userAgent: UA, viewport: { width: 390, height: 844 },
  deviceScaleFactor: 3, isMobile: true, hasTouch: true, serviceWorkers: "block",
});
const page = await context.newPage();
const missing = [];
page.on("response", (r) => {
  if (r.url().includes("/onboard-img/") && r.status() >= 400) missing.push(`${r.status()} ${r.url()}`);
});
await page.goto(`${BASE}/?u=${UID}`, { waitUntil: "domcontentloaded" });
await page.waitForTimeout(10000);

// UA печатается специально: ветка выбирается по нему, и если он вдруг не доехал
// до страницы, проверка меряла бы не то, что думает.
console.log("UA в странице:", await page.evaluate(() => navigator.userAgent));
const text = (await page.locator("body").innerText()).replace(/\s+/g, " ");
console.log("текст:", text.slice(0, 260));
const shots = await page.$$eval(".step-shot", (els) => els.map((e) => e.getAttribute("src")));
console.log("снимки шагов:", JSON.stringify(shots));
console.log("не отдались:", missing.length ? JSON.stringify(missing) : "нет");
await page.screenshot({ path: OUT, fullPage: true });
console.log("снято:", OUT);

await context.close();
await b.close();
