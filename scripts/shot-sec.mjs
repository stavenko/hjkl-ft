import { chromium } from "playwright";
import { openSeeded, planFor } from "./harness.mjs";
const b = await chromium.launch({ headless: true });
const { context, page } = await openSeeded(b, { baseUrl: process.argv[2], plan: planFor("3.0"), landing: "/story/ch3-deficit" });
await page.getByRole("button", { name: /Принять планку/ }).waitFor({ timeout: 8000 }).catch(()=>{});
await page.screenshot({ path: "ch3-deficit.png", fullPage: true });
await context.close(); await b.close(); console.log("ok");
