// Verify the finalize-recipe modal on dev: labels, placeholder "0", red error on empty submit.
import { chromium } from "playwright";
import { readFileSync } from "fs";
const FE = "https://9b0482e8.renorma-fit-dev.pages.dev";
const uid = readFileSync("/tmp/repro_uid.txt", "utf8").trim();
const token = readFileSync("/tmp/repro_token.txt", "utf8").trim();

const b = await chromium.launch({ headless: true });
const ctx = await b.newContext({ viewport: { width: 430, height: 920 }, serviceWorkers: "block" });
const page = await ctx.newPage();
await page.goto(FE, { waitUntil: "domcontentloaded" });
await page.evaluate(({ uid, token }) => {
  localStorage.clear();
  localStorage.setItem("user_id", uid);
  localStorage.setItem("auth_token", token);
  localStorage.setItem("token_id", "tok1");
  localStorage.setItem("auth_ctx", "browser");
  localStorage.setItem("pwa_dismissed", "true");
  localStorage.setItem("paywall_skipped_date", new Date().toISOString().slice(0, 10));
}, { uid, token });
await page.goto(FE + "/recipes", { waitUntil: "domcontentloaded" });
await page.waitForSelector("#splash", { state: "detached", timeout: 20000 }).catch(() => {});
await page.waitForSelector('[data-testid="recipes-btn-new"]', { timeout: 20000 });
// Seeded stale token leaves the app-checking overlay up; it intercepts clicks but the page under it is live. Remove it for this UI check.
await page.waitForTimeout(3000);
await page.evaluate(() => document.querySelector('[data-testid="app-checking"]')?.remove());
await page.click('[data-testid="recipes-btn-new"]');
await page.waitForSelector('[data-testid="recipe-detail-btn-finalize"]', { timeout: 20000 });
await page.click('[data-testid="recipe-detail-btn-finalize"]');
await page.waitForSelector('[data-testid="recipe-detail-input-final-weight"]', { timeout: 10000 });

const before = await page.evaluate(() => {
  const inp = document.querySelector('[data-testid="recipe-detail-input-final-weight"]');
  const modal = document.querySelector(".modal-card-body");
  return {
    placeholder: inp.placeholder,
    inputDanger: inp.classList.contains("is-danger"),
    errorShown: !!document.querySelector('[data-testid="recipe-detail-final-weight-error"]'),
    bodyText: modal.innerText.replace(/\s+/g, " ").trim(),
  };
});
console.log("before submit:", JSON.stringify(before, null, 1));

await page.click('[data-testid="recipe-detail-btn-finalize-submit"]');
await page.waitForTimeout(400);
const after = await page.evaluate(() => {
  const inp = document.querySelector('[data-testid="recipe-detail-input-final-weight"]');
  const err = document.querySelector('[data-testid="recipe-detail-final-weight-error"]');
  return {
    inputDanger: inp.classList.contains("is-danger"),
    errorText: err ? err.textContent.trim() : null,
    modalStillOpen: !!document.querySelector(".modal.is-active"),
  };
});
console.log("after empty submit:", JSON.stringify(after, null, 1));
await page.screenshot({ path: "scripts/recipe-finalize-error.png" });

await page.fill('[data-testid="recipe-detail-input-final-weight"]', "300");
await page.waitForTimeout(200);
const typed = await page.evaluate(() => {
  const inp = document.querySelector('[data-testid="recipe-detail-input-final-weight"]');
  return {
    inputDanger: inp.classList.contains("is-danger"),
    errorShown: !!document.querySelector('[data-testid="recipe-detail-final-weight-error"]'),
  };
});
console.log("after typing 300:", JSON.stringify(typed, null, 1));
await b.close();
