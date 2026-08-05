// Поиск КБЖУ по названию — самый видимый запрос к модели. Его цикл попыток был
// переписан на общий `generate`, поэтому путь проверяется живьём: вводим название,
// жмём «Новый продукт» и ждём, пока в форме появятся числа.
import { chromium } from "playwright";
import { DEFAULT_URL } from "./harness.mjs";

const BASE = process.env.FE || DEFAULT_URL;
const PAY = process.env.PAY || "https://payment-worker-dev.vg-stavenko.workers.dev";
const SECRET = process.env.JWT_SECRET || "dev-secret-change-in-production";
const NAME = process.env.NAME || "Гречка";

let fail = 0;
const check = (n, ok, extra = "") => { console.log(`${ok ? "OK " : "FAIL"} ${n}${extra ? " — " + extra : ""}`); if (!ok) fail++; };

const b64url = (buf) => Buffer.from(buf).toString("base64url");
const uid = `kbju-${Date.now()}`;
const now = Math.floor(Date.now() / 1000);
const data = b64url(JSON.stringify({ alg: "HS256", typ: "JWT" })) + "." +
  b64url(JSON.stringify({ sub: uid, iat: now, exp: now + 7200, caps: [], token_id: "t" }));
const key = await crypto.subtle.importKey("raw", new TextEncoder().encode(SECRET),
  { name: "HMAC", hash: "SHA-256" }, false, ["sign"]);
const token = `${data}.${b64url(new Uint8Array(await crypto.subtle.sign("HMAC", key, new TextEncoder().encode(data))))}`;

const co = await (await fetch(`${PAY}/test/guest-checkout`, { method: "POST",
  headers: { "Content-Type": "application/json" }, body: JSON.stringify({ planId: "test" }) })).json();
await fetch(`${PAY}/claim`, { method: "POST",
  headers: { "Content-Type": "application/json", Authorization: `Bearer ${token}` },
  body: JSON.stringify({ claimId: co.claimId, secret: co.secret }) });

const b = await chromium.launch({ headless: true });
const ctx = await b.newContext({ viewport: { width: 430, height: 932 }, serviceWorkers: "block" });
const page = await ctx.newPage();
await page.goto(BASE, { waitUntil: "domcontentloaded" });
await page.evaluate(({ uid, token }) => {
  localStorage.clear();
  localStorage.setItem("user_id", uid);
  localStorage.setItem("auth_token", token);
  localStorage.setItem("token_id", "t");
  localStorage.setItem("auth_ctx", "browser");
  localStorage.setItem("pwa_dismissed", "true");
}, { uid, token });

await page.goto(`${BASE}/diary/add`, { waitUntil: "domcontentloaded" });
await page.waitForSelector("#splash", { state: "detached", timeout: 25000 }).catch(() => {});
await page.waitForSelector('[data-testid="diary-add-input-search"]', { timeout: 25000 });
await page.fill('[data-testid="diary-add-input-search"]', NAME);
await page.waitForTimeout(1200);
await page.locator('[data-testid="diary-add-btn-new-food"]').first().click();

// Форма открылась пустой — вводим название и жмём «Заполнить» (это и есть запрос
// КБЖУ к модели).
await page.waitForTimeout(1500);
const nameInput = page.locator('input[type="text"]').first();
await nameInput.fill(NAME);
await page.getByText("Заполнить", { exact: true }).first().click();

// Ждём, пока в форме появятся заполненные калории.
let kcal = "";
for (let i = 0; i < 60; i++) {
  kcal = await page.evaluate(() => {
    const inputs = [...document.querySelectorAll("input")].map((i) => i.value).filter((v) => /^\d+([.,]\d+)?$/.test(v));
    return inputs.join(",");
  });
  if (kcal) break;
  await page.waitForTimeout(2000);
}
const text = (await page.locator("body").innerText()).replace(/\s+/g, " ").slice(0, 300);
check("модель вернула КБЖУ", kcal !== "", `значения: ${kcal || "пусто"}`);
check("экран не показывает ошибку", !/ошибк|error|не удалось/i.test(text), text.slice(0, 160));

console.log(fail === 0 ? "\n=== ALL OK ===" : `\n=== FAILURES: ${fail} ===`);
await b.close();
process.exit(fail === 0 ? 0 : 1);
