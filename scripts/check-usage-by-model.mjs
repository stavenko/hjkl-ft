// РАСХОД ПО МОДЕЛЯМ ВИДЕН В АДМИНКЕ — от запроса к модели до строки на экране.
//
// Учёт токенов у нас свой: чужого API, который скажет «столько потрачено», у
// провайдера нет — только консоль. Поэтому проверяется вся цепочка: запрос к
// сторонней модели → usage из ответа → payment-worker → UsageDO → админка.
//
//   node scripts/check-usage-by-model.mjs
import { chromium } from "playwright";

const AI = process.env.AI || "https://ai-worker-dev.vg-stavenko.workers.dev";
const PAY = process.env.PAY || "https://payment-worker-dev.vg-stavenko.workers.dev";
const SUPPORT = process.env.SUPPORT || "https://support-worker-dev.vg-stavenko.workers.dev";
const ADMIN = process.env.ADMIN || "https://renorma-admin-dev.pages.dev";
const SECRET = process.env.JWT_SECRET || "dev-secret-change-in-production";
const APPROVE = process.env.ADMIN_APPROVE_SECRET || "dev-admin-approve-secret";
const MODEL = process.env.MODEL || "qwen3.8-27b";

const b64 = (b) => Buffer.from(b).toString("base64url");
const mint = async (sub) => {
  const now = Math.floor(Date.now() / 1000);
  const head = b64(JSON.stringify({ alg: "HS256", typ: "JWT" })) + "." +
    b64(JSON.stringify({ sub, iat: now, exp: now + 7200, caps: [], token_id: "t" }));
  const key = await crypto.subtle.importKey("raw", new TextEncoder().encode(SECRET),
    { name: "HMAC", hash: "SHA-256" }, false, ["sign"]);
  const sig = await crypto.subtle.sign("HMAC", key, new TextEncoder().encode(head));
  return `${head}.${b64(new Uint8Array(sig))}`;
};
const post = (url, body, headers = {}) => fetch(url, { method: "POST",
  headers: { "Content-Type": "application/json", ...headers }, body: JSON.stringify(body) });

let failed = 0;
const check = (what, ok, detail = "") => {
  console.log(`${ok ? "✅" : "❌"} ${what}${detail ? ` — ${detail}` : ""}`);
  if (!ok) failed++;
};

// 1. Один живой запрос к сторонней модели: он и есть источник токенов.
const user = `usage-user-${Date.now()}`;
const userToken = await mint(user);
const co = await (await post(`${PAY}/test/guest-checkout`, { planId: "test" })).json();
await post(`${PAY}/claim`, { claimId: co.claimId, secret: co.secret },
  { Authorization: `Bearer ${userToken}` });
const ai = await post(`${AI}/chat/completions`, {
  model: MODEL, messages: [{ role: "user", content: "Ответь одним словом: да." }],
  stream: true, think: false, max_tokens: 32,
}, { Authorization: `Bearer ${userToken}` });
check("запрос к сторонней модели прошёл", ai.ok, `HTTP ${ai.status}`);
await ai.text();

// 2. Отчёт админа. Учёт пишется в wait_until — даём ему дойти.
const adminSub = `usage-admin-${Date.now()}`;
const adminToken = await mint(adminSub);
const { code } = await (await post(`${SUPPORT}/admin/request`, {},
  { Authorization: `Bearer ${adminToken}` })).json();
await post(`${SUPPORT}/admin/approve`, { code }, { "X-Admin-Secret": APPROVE });

let row = null;
for (let i = 0; i < 10 && !row; i++) {
  await new Promise((r) => setTimeout(r, 2000));
  const rep = await (await fetch(`${PAY}/admin/usage`,
    { headers: { Authorization: `Bearer ${adminToken}` } })).json();
  row = (rep.byModel || []).find((m) => m.model === MODEL);
}
check(`токены записаны на модель ${MODEL}`, !!row,
  row ? `${row.inTokens} ↓ / ${row.outTokens} ↑ · source ${row.source}` : "строки нет");
check("источник отделён от Workers AI", row?.source === "thirdparty", row?.source ?? "—");

// 3. Экран админки: строка модели видна человеку, а не только в JSON.
const b = await chromium.launch({ headless: true });
const ctx = await b.newContext({ viewport: { width: 430, height: 932 } });
const page = await ctx.newPage();
await page.goto(ADMIN, { waitUntil: "domcontentloaded" });
await page.evaluate(({ uid, token }) => {
  localStorage.clear();
  localStorage.setItem("user_id", uid);
  localStorage.setItem("auth_token", token);
}, { uid: adminSub, token: adminToken });
await page.goto(ADMIN, { waitUntil: "domcontentloaded" });
await page.waitForTimeout(6000);
// Вкладка подписана «Токены», а заголовок внутри — «Нейроны»: кликать надо по
// вкладке, и надёжнее по её testid, чем по подписи.
const usageTab = page.locator('[data-testid="tab-usage"]');
await usageTab.waitFor({ timeout: 15000 });
await usageTab.click();
await page.waitForTimeout(5000);
const text = (await page.locator("body").innerText()).replace(/\s+/g, " ");
check("в админке есть раздел «По моделям»", text.includes("По моделям"));
check("в админке видно имя модели", text.includes(MODEL),
  text.includes(MODEL) ? "" : text.slice(0, 160));
// Раздел моделей стоит последним, а прокручивается НЕ страница, а контейнер
// экрана: `fullPage` его не разворачивает, поэтому листаем контейнер сами.
await page.evaluate(() => {
  const box = [...document.querySelectorAll("*")]
    .find((el) => el.scrollHeight > el.clientHeight + 40 && getComputedStyle(el).overflowY !== "visible");
  if (box) box.scrollTop = box.scrollHeight;
});
await page.waitForTimeout(600);
await page.screenshot({ path: process.env.SHOT || "/tmp/admin-usage-by-model.png" });
await ctx.close();
await b.close();
process.exit(failed ? 1 : 0);
