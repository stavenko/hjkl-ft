// Снимки трёх экранов обходного пути: предложение, ввод кода, «нет доступа».
import { chromium } from "playwright";

const BASE = "https://renorma-fit-dev.pages.dev";
const OUT = "/Users/vasilijstavenko/projects/hjkl-ft";

async function openAuth(browser, uid) {
  const ctx = await browser.newContext({ viewport: { width: 390, height: 844 } });
  const page = await ctx.newPage();
  await page.goto(`${BASE}/?u=${uid}`, { waitUntil: "domcontentloaded" });
  await page.getByTestId("pwa-btn-dismiss").click({ timeout: 40000 });
  await page.getByTestId("auth-btn-try-passkey").waitFor({ timeout: 40000 });
  for (let i = 0; i < 6; i++) {
    await page.getByTestId("auth-btn-try-passkey").click({ timeout: 20000 });
    const ok = await page
      .getByTestId("auth-error")
      .waitFor({ timeout: 10000 })
      .then(() => true)
      .catch(() => false);
    if (ok) break;
  }
  return { ctx, page };
}

const b = await chromium.launch();

{
  const { ctx, page } = await openAuth(b, "pk-state-test");
  await page.screenshot({ path: `${OUT}/auth-tg-offer.png` });
  await page.getByTestId("auth-btn-tg-login").click();
  await page.getByTestId("codeauth-btn-send").waitFor({ timeout: 40000 });
  await page.waitForTimeout(3000);
  await page.screenshot({ path: `${OUT}/auth-tg-code.png` });
  await ctx.close();
}
{
  const { ctx, page } = await openAuth(b, "11111111-2222-3333-4444-555555555555");
  await page.getByTestId("auth-btn-tg-login").click();
  await page.getByTestId("auth-link-bot").waitFor({ timeout: 40000 });
  await page.screenshot({ path: `${OUT}/auth-no-access.png` });
  await ctx.close();
}

await b.close();
console.log("готово");
