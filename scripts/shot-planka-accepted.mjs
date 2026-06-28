// One-off: open ch3-fat, press "accept", screenshot the post-accept state.
//   node scripts/shot-planka-accepted.mjs [baseUrl] [outPath]
import { chromium } from "playwright";
import { openSeeded, planFor } from "./harness.mjs";

const BASE = process.argv[2] || "https://hjkl-ft.pages.dev";
const OUT = process.argv[3] || "planka-accepted.png";

const browser = await chromium.launch({ headless: true });
const { context, page } = await openSeeded(browser, {
  baseUrl: BASE, plan: planFor("3.0"), landing: "/story/ch3-fat",
});
await page.getByRole("button", { name: /Принять планку/ }).click();
await page.waitForTimeout(900);
await page.screenshot({ path: OUT, fullPage: true });
await context.close();
await browser.close();
console.log("saved", OUT);
