// Зависшее обновление: через 15 с человек получает объяснение и кнопку выхода.
//
// Зависнуть перезагрузка может в двух РАЗНЫХ местах, и проверяются оба:
//   1. ДО навигации — старая страница жива, обновление «идёт» и не идёт;
//   2. ПОСЛЕ — новая страница встала на сплэше, WASM ещё не запустился.
// Во втором случае никакого Rust нет, поэтому сторож живёт в index.html; отметка
// о начале лежит в sessionStorage и переживает переход.
//
// Заодно проверяется, что кнопка делает то, что обещает: сносит кэши и
// сервис-воркер и грузит приложение заново.
import { chromium } from "playwright";

const BASE = process.env.FE || "https://renorma-fit-dev.pages.dev";

let fail = 0;
const check = (n, ok, extra = "") => {
  console.log(`${ok ? "OK  " : "FAIL"} ${n}${extra ? " — " + extra : ""}`);
  if (!ok) fail++;
};

const b = await chromium.launch({ headless: true });

// ── 1. Зависание ДО навигации ───────────────────────────────────────────────
{
  const ctx = await b.newContext({ viewport: { width: 430, height: 932 }, serviceWorkers: "block" });
  const page = await ctx.newPage();
  await page.goto(BASE, { waitUntil: "domcontentloaded" });
  await page.waitForTimeout(3000);

  // Взводим сторож ровно так, как это делает `update::reload()`, но саму
  // перезагрузку не запускаем — это и есть «зависла до навигации».
  await page.evaluate(() => window.__rnUpdateArm());
  check("сразу после начала обновления ничего не показано",
    await page.locator('[data-testid="update-stuck"]').count() === 0);

  await page.waitForTimeout(16000);
  const shown = await page.locator('[data-testid="update-stuck"]').count();
  check("через 15 с показано сообщение о зависании", shown > 0);
  const text = await page.locator('[data-testid="update-stuck"]').innerText().catch(() => "");
  check("сказано, что обновление слишком долгое", text.includes("слишком долго"), text.slice(0, 80));
  check("названа возможная причина", text.includes("сетевых проблем"));
  check("есть кнопка перезагрузки",
    await page.locator('[data-testid="update-stuck-btn"]').count() > 0);
  await page.screenshot({ path: "/tmp/update-stuck.png", fullPage: true });
  await ctx.close();
}

// ── 2. Зависание ПОСЛЕ навигации, на сплэше ─────────────────────────────────
{
  const ctx = await b.newContext({ viewport: { width: 430, height: 932 }, serviceWorkers: "block" });
  const page = await ctx.newPage();
  // Рубим WASM: страница загрузится, а приложение не поднимется — ровно то, как
  // выглядит зависшее обновление после перехода.
  await page.route("**/*_bg.wasm", (r) => r.abort("connectionfailed"));
  // Отметка о начале обновления — как будто её поставила прошлая жизнь страницы.
  await page.addInitScript(() => {
    try { sessionStorage.setItem("update_started_at", String(Date.now())); } catch (e) {}
  });
  await page.goto(BASE, { waitUntil: "domcontentloaded" });

  check("на сплэше сначала тихо",
    await page.locator('[data-testid="update-stuck"]').count() === 0);
  await page.waitForTimeout(17000);
  check("через 15 с на сплэше показано то же сообщение",
    await page.locator('[data-testid="update-stuck"]').count() > 0);
  await page.screenshot({ path: "/tmp/update-stuck-splash.png", fullPage: true });
  await ctx.close();
}

// ── 3. Кнопка действительно сбрасывает ──────────────────────────────────────
{
  const ctx = await b.newContext({ viewport: { width: 430, height: 932 } });
  const page = await ctx.newPage();
  await page.goto(BASE, { waitUntil: "domcontentloaded" });
  await page.waitForTimeout(6000);
  // Кладём мусор в кэш — по нему и увидим, что уборка состоялась.
  await page.evaluate(async () => {
    const c = await caches.open("проверка-сброса");
    await c.put("/marker", new Response("x"));
  });
  check("мусор в кэше есть",
    (await page.evaluate(() => caches.keys())).includes("проверка-сброса"));

  await page.evaluate(() => window.__rnUpdateArm());
  await page.waitForTimeout(16000);
  await page.locator('[data-testid="update-stuck-btn"]').click();
  await page.waitForTimeout(9000);

  const keys = await page.evaluate(() => caches.keys());
  check("кэши снесены", !keys.includes("проверка-сброса"), JSON.stringify(keys));
  const marker = await page.evaluate(() => {
    try { return sessionStorage.getItem("update_started_at"); } catch (e) { return "?"; }
  });
  check("отметка об обновлении снята", marker === null, String(marker));
  check("приложение загрузилось заново",
    await page.locator('[data-testid="update-stuck"]').count() === 0);
  await ctx.close();
}

await b.close();
console.log("\nснимки: /tmp/update-stuck.png, /tmp/update-stuck-splash.png");
console.log(fail ? `\n${fail} провалов` : "\nвсё сошлось");
process.exit(fail ? 1 : 0);
