import { chromium } from "playwright";
import { openSeeded, planFor } from "./harness.mjs";
const url = process.argv[2];
const b = await chromium.launch({ headless: true });
// settings with BMI
let s = await openSeeded(b, { baseUrl: url, plan: planFor("2.0"), landing: "/settings" });
const inp = s.page.locator('[data-testid="settings-input-height"]');
await inp.waitFor({ timeout: 8000 }); await inp.fill("180"); await inp.blur(); await s.page.waitForTimeout(400);
await s.page.screenshot({ path: "settings-bmi.png", fullPage: true }); await s.context.close();
// ch3-no-loss section
let n = await openSeeded(b, { baseUrl: url, plan: planFor("3.1"), landing: "/story/ch3-no-loss" });
await n.page.waitForTimeout(700);
await n.page.screenshot({ path: "ch3-noloss.png", fullPage: true }); await n.context.close();
await b.close(); console.log("ok");
