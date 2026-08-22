// КАРТИНКИ ИСТОРИЙ ОБНОВЛЯЮТСЯ НА УСТРОЙСТВЕ — проверка обоих механизмов сразу.
//
// Адрес у картинки постоянный, содержимое меняется: переснятый кадр приезжает на
// прод, а человек продолжает видеть прошлый. Ровно это и случилось с кадром железа.
// Здесь проверяется, что так больше не будет:
//
//   1. кэш с ЧУЖИМ именем сносится при установке service worker'а — так уходит
//      старьё, накопленное под прежним `CACHE_NAME`;
//   2. картинка отдаётся из кэша, но следом перекачивается (stale-while-revalidate)
//      — так доезжают правки, сделанные уже после установки.
//
//   node scripts/check-story-img-cache.mjs
import { chromium } from "playwright";
import { DEFAULT_URL } from "./harness.mjs";

const BASE = process.env.FE || DEFAULT_URL;
const IMG = "/story-img/iron-highlight.gif";
const FAKE = "GIF89a-подделка-для-проверки";

const browser = await chromium.launch({ headless: true });
const context = await browser.newContext();
const page = await context.newPage();
await page.goto(BASE, { waitUntil: "domcontentloaded" });
await page.waitForFunction(() => navigator.serviceWorker.controller !== null, null, { timeout: 30000 });
// Приложение на старте уходит на свой маршрут (онбординг/вход), и evaluate,
// начатый до этого, падает «execution context destroyed». Даём навигации случиться.
await page.waitForTimeout(6000);

// 1. Чужой кэш сносится. Кладём его РУКАМИ и заставляем service worker
//    переустановиться — иначе `activate` уже позади и уборка не повторится.
await page.evaluate(async () => {
  const c = await caches.open("ft-v39");
  await c.put("/story-img/iron-highlight.gif", new Response("старьё"));
});
const before = await page.evaluate(() => caches.keys());
await page.evaluate(async () => {
  for (const r of await navigator.serviceWorker.getRegistrations()) await r.unregister();
});
await page.reload({ waitUntil: "domcontentloaded" });
await page.waitForFunction(() => navigator.serviceWorker.controller !== null, null, { timeout: 30000 });
await page.waitForTimeout(2000);
const after = await page.evaluate(() => caches.keys());
const swept = !after.includes("ft-v39");
console.log(`кэши до: [${before}] после переустановки: [${after}] → чужой снесён: ${swept ? "да" : "НЕТ"}`);

// 2. Stale-while-revalidate. Подменяем картинку в ЖИВОМ кэше: первый запрос обязан
//    вернуть подделку (мгновенно, в том числе офлайн), а следующий — настоящий файл.
const name = after.find((n) => n.startsWith("ft-v")) || "ft-v40";
await page.evaluate(async ({ name, IMG, FAKE }) => {
  const c = await caches.open(name);
  await c.put(IMG, new Response(FAKE, { headers: { "Content-Type": "image/gif" } }));
}, { name, IMG, FAKE });
const first = await page.evaluate((IMG) => fetch(IMG).then((r) => r.text()).then((t) => t.slice(0, 30)), IMG);
await page.waitForTimeout(2500);
const second = await page.evaluate((IMG) =>
  caches.match(IMG).then((r) => (r ? r.text() : "")).then((t) => t.slice(0, 30)), IMG);
const servedStale = first === FAKE;
const refreshed = second !== FAKE && second.length > 0;
console.log(`первый ответ из кэша: ${servedStale ? "да" : "НЕТ"} · кэш перекачан следом: ${refreshed ? "да" : "НЕТ"}`);

await context.close();
await browser.close();
if (!swept || !servedStale || !refreshed) process.exit(1);
console.log("\nобе проверки прошли");
