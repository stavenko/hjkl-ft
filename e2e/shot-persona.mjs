// Экран персоны при первом запуске: чистый и после разбора формы.
import { chromium } from "playwright";
import { execSync } from "node:child_process";

const BASE = process.env.BASE || "https://renorma-fit-dev.pages.dev";
const UID = `persona-shot-${Date.now().toString(36)}`;
const TOKEN = execSync(`node ../frontend/scripts/seed-test-subscription.mjs ${UID}`, {
  encoding: "utf8",
}).match(/^token:\s+(\S+)$/m)[1];

const b = await chromium.launch();
const ctx = await b.newContext({ viewport: { width: 390, height: 760 }, deviceScaleFactor: 2 });
await ctx.addInitScript(([uid, token]) => {
  sessionStorage.setItem("update_auto_applied", "1");
  localStorage.setItem("user_id", uid);
  localStorage.setItem("auth_token", token);
  localStorage.setItem("token_id", "tok1");
  localStorage.setItem("auth_ctx", "browser");
}, [UID, TOKEN]);
const p = await ctx.newPage();
await p.goto(`${BASE}/`, { waitUntil: "domcontentloaded" });
await p.waitForFunction(() => !!navigator.serviceWorker.controller, null, { timeout: 40000 })
  .catch(() => {});
await p.waitForTimeout(2500);
const d = p.getByTestId("pwa-btn-dismiss");
if (await d.isVisible().catch(() => false)) await d.click();
await p.getByTestId("persona-btn-done").waitFor({ timeout: 60000 });

await p.screenshot({ path: "/Users/vasilijstavenko/projects/hjkl-ft/persona-first-run.png" });
await p.getByTestId("persona-year").fill("90");
await p.getByTestId("persona-btn-done").click();
await p.waitForTimeout(700);
await p.screenshot({ path: "/Users/vasilijstavenko/projects/hjkl-ft/persona-errors.png" });
await b.close();
console.log("снято");
