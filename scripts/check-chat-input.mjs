// Verify the chat input on dev: no mic button; empty input shows a grey DISABLED send.
import { chromium } from "playwright";
import { readFileSync } from "fs";
const FE = "https://9f808062.renorma-fit-dev.pages.dev";
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
await page.goto(FE + "/chat", { waitUntil: "domcontentloaded" });
await page.waitForSelector("#splash", { state: "detached", timeout: 20000 }).catch(() => {});
await page.waitForSelector('[data-testid="chat-input"]', { timeout: 20000 });
await page.waitForTimeout(800);

const empty = await page.evaluate(() => {
  const mic = document.querySelector('[data-testid="chat-record-voice"]');
  const send = document.querySelector('[data-testid="chat-send"]');
  return {
    micPresent: !!mic,
    sendPresent: !!send,
    sendDisabled: send ? send.disabled : null,
    sendBg: send ? getComputedStyle(send).backgroundColor : null,
    sendPointer: send ? getComputedStyle(send).pointerEvents : null,
  };
});
console.log("empty input:", JSON.stringify(empty));

await page.fill('[data-testid="chat-input"]', "привет");
await page.waitForTimeout(300);
const typed = await page.evaluate(() => {
  const send = document.querySelector('[data-testid="chat-send"]');
  return { sendPresent: !!send, sendDisabled: send ? send.disabled : null, sendBg: send ? getComputedStyle(send).backgroundColor : null };
});
console.log("with text:  ", JSON.stringify(typed));

await page.fill('[data-testid="chat-input"]', "");
await page.waitForTimeout(300);
await page.screenshot({ path: "/private/tmp/claude-501/-Users-vasilijstavenko-projects-hjkl-ft/56df53af-a1ed-4117-8e82-8a1f8aad90e8/scratchpad/chat-empty.png", clip: { x: 0, y: 640, width: 430, height: 280 } });
await b.close();
