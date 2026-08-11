// Ссылка «Открыть приложение» из Mini App: `/onboard?u=<user_id>` без разового кода.
//
// Её работа — отдать ПЕРСОНАЛЬНЫЙ манифест и показать, как поставить ярлык на
// рабочий стол. Значит от нового браузера, где нет ни кода, ни сессии, требуется:
//   1. в HTML стоит <link rel="manifest" href="/manifest.json?u=…"> — иначе
//      «На экран „Домой“» снимет общий манифест со start_url "/" и поставленное
//      приложение потеряет аккаунт;
//   2. манифест по этому адресу и правда персональный (start_url и id с ?u);
//   3. на экране — инструкция по установке, а НЕ «Ссылка устарела» (кода нет не
//      потому, что он протух, а потому, что он здесь и не нужен).
//
// Для сравнения проверяется и вход БЕЗ `?u`: он по-прежнему уводит на корень.
import { chromium } from "playwright";

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
  const context = await b.newContext({ serviceWorkers: "block" });
  const page = await context.newPage();
  await page.goto(`${BASE}/onboard?u=${UID}`, { waitUntil: "domcontentloaded" });
  await page.waitForTimeout(9000);

  const href = await page.getAttribute('link[rel="manifest"]', "href");
  check("ссылка на манифест персональная", href === `/manifest.json?u=${UID}`, String(href));

  const mf = await page.evaluate(async (h) => (await fetch(h)).json(), href);
  check("манифест ведёт в аккаунт", mf.start_url === `/?u=${UID}` && mf.id === `/app-${UID}`,
    `${mf.start_url} · ${mf.id}`);

  const install = await page.locator('[data-testid="pwa-btn-dismiss"]').count();
  const stale = await page.locator('[data-testid="onboard-failed"]').count();
  check("показан экран установки", install > 0, `кнопок установки ${install}`);
  check("«Ссылка устарела» не показывается", stale === 0);

  await context.close();
}

// ── без ?u — по-прежнему уводит на корень ───────────────────────────────────
{
  const context = await b.newContext({ serviceWorkers: "block" });
  const page = await context.newPage();
  await page.goto(`${BASE}/onboard`, { waitUntil: "domcontentloaded" });
  await page.waitForTimeout(6000);
  const path = new URL(page.url()).pathname;
  check("вход без ?u уходит на корень", path === "/", page.url());
  await context.close();
}

await b.close();
console.log(fail ? `\n${fail} провалов` : "\nвсё сошлось");
process.exit(fail ? 1 : 0);
