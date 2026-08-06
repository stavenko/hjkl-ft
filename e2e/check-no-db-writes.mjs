// До входа приложение не должно ничего считать и никуда писать.
//
// Роутер смонтирован под экранами установки и входа, поэтому дашборд оживал
// раньше базы: считал индикаторы по пустоте и пытался положить результат туда,
// где хранить некому. Расчёт был бессмысленным, запись — потерянной, а человек
// получал три сообщения об ошибке, с которыми ничего сделать не мог.
//
// Прогон: node check-no-db-writes.mjs
import { chromium } from "playwright";
import { execSync } from "node:child_process";

const BASE = process.env.BASE || "https://renorma-fit-dev.pages.dev";

let failed = 0;
const check = (ok, what) => {
  console.log(`${ok ? "  ok  " : "  ПРОВАЛ "} ${what}`);
  if (!ok) failed++;
};

const b = await chromium.launch();

/// Загрузить приложение и собрать жалобы «без активной базы» из консоли.
/// `session` — [user_id, token] или null для запуска без сессии.
async function run(session) {
  const ctx = await b.newContext({ viewport: { width: 390, height: 844 } });
  await ctx.addInitScript((s) => {
    sessionStorage.setItem("update_auto_applied", "1");
    if (!s) return;
    localStorage.setItem("user_id", s[0]);
    localStorage.setItem("auth_token", s[1]);
    localStorage.setItem("token_id", "tok1");
    localStorage.setItem("auth_ctx", "browser");
  }, session);
  const page = await ctx.newPage();
  const hits = [];
  const started = [];
  page.on("console", (m) => {
    const t = m.text();
    if (/без активной базы/.test(t)) hits.push(t);
    if (/обслуживание данных: база открыта/.test(t)) started.push(t);
  });
  await page.goto(`${BASE}/`, { waitUntil: "domcontentloaded" });
  await page
    .waitForFunction(() => !!navigator.serviceWorker.controller, null, { timeout: 40000 })
    .catch(() => {});
  await page.waitForTimeout(2500);
  const dismiss = page.getByTestId("pwa-btn-dismiss");
  if (await dismiss.isVisible().catch(() => false)) await dismiss.click();
  await page.waitForTimeout(20000);
  return { ctx, page, hits, started };
}

// ── 1. Без сессии: считать нечего, писать некуда ──
{
  const { ctx, page, hits, started } = await run(null);
  console.log("\n1. Запуск без сессии");
  check(started.length === 0, "обслуживание данных не начиналось — ждёт базу");
  if (hits.length) console.log("   записи:", [...new Set(hits)].join(" | "));
  check(hits.length === 0, `в базу не пишут (попыток: ${hits.length})`);
  // Журнал приложения — для того, с чем человек может что-то сделать.
  const body = await page.innerText("body");
  check(!/базы пользователя ещё нет/.test(body), "и человеку об этом не сообщают");
  await ctx.close();
}

// ── 2. С сессией: дашборд живёт и считает как обычно ──
{
  const uid = `nodb-${Date.now().toString(36)}`;
  const token = execSync(`node ../frontend/scripts/seed-test-subscription.mjs ${uid}`, {
    encoding: "utf8",
  }).match(/^token:\s+(\S+)$/m)[1];
  const { ctx, page, hits, started } = await run([uid, token]);
  console.log("\n2. Запуск с сессией");
  check(started.length === 1, `обслуживание данных отработало (${started.length})`);
  if (hits.length) console.log("   записи:", [...new Set(hits)].join(" | "));
  check(
    await page.getByTestId("nav-diary").isVisible().catch(() => false),
    "приложение открылось",
  );
  check(hits.length === 0, `лишних записей нет (${hits.length})`);
  await ctx.close();
}

await b.close();
console.log(failed ? `\nПРОВАЛОВ: ${failed}` : "\nвсё сошлось");
process.exit(failed ? 1 : 0);
