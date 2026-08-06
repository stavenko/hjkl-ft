// Как выглядит экран установки PWA в Яндекс.Браузере на iPhone.
// Подменяем User-Agent на настоящий (снят пробником на телефоне) и снимаем
// страницу целиком.
import { chromium } from "playwright";

const BASE = process.env.BASE || "https://renorma-fit-dev.pages.dev";
const UA =
  "Mozilla/5.0 (iPhone; CPU iPhone OS 18_7_2 like Mac OS X) AppleWebKit/605.1.15 " +
  "(KHTML, like Gecko) Version/18.0 YaBrowser/26.6.7.3 SA/3 Mobile/15E148 Safari/604.1";

const b = await chromium.launch();
const ctx = await b.newContext({
  userAgent: UA,
  viewport: { width: 390, height: Number(process.env.H || 844) },
  deviceScaleFactor: 2,
  isMobile: true,
  hasTouch: true,
});
const page = await ctx.newPage();
await page.goto(`${BASE}/?u=05775000-0000-0000-0000-000000000000`, {
  waitUntil: "domcontentloaded",
});
// Гифки грузятся отдельными запросами — без ожидания в кадр попадут пустые рамки.
await page.waitForTimeout(9000);
await page.screenshot({
  path: "/Users/vasilijstavenko/projects/hjkl-ft/pwa-ios-yandex.png",
  fullPage: true,
});
console.log("снято: pwa-ios-yandex.png");
await b.close();
