import { chromium } from "playwright";
import { openSeeded, planFor } from "./harness.mjs";
const b = await chromium.launch({ headless: true });
const { context, page } = await openSeeded(b, { baseUrl: process.argv[2], plan: planFor("3.8"), landing: "/" });
await page.waitForTimeout(900);
await page.screenshot({ path: "ch3-structure.png", fullPage: true });
await context.close(); await b.close(); console.log("ok");
