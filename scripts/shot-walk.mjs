import { chromium } from "playwright";
import { openSeeded, planFor } from "./harness.mjs";
const b = await chromium.launch({ headless: true });
const { context, page } = await openSeeded(b, { baseUrl: process.argv[2], plan: planFor("3.5"), landing: "/story/ch3-walk" });
await page.waitForTimeout(800);
await page.screenshot({ path: "ch3-walk.png", fullPage: true });
await context.close(); await b.close(); console.log("ok");
