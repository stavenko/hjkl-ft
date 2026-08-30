// Сбой модели человеку показывается ПОНЯТНО, а не дампом.
//
// Повод: при исчерпанной квоте Workers AI на экран уезжала простыня «LLM output
// error: ModelExecution("HTTP 500: {\"error\":\"AI.run: JsValue(AiError: 4006: you
// have used up your daily free allocation of 10,000 neurons…». Человек не может ни
// прочесть это, ни что-то с этим сделать.
//
// Проверка идёт тем же путём, что и живой человек: вводит название, жмёт
// «Заполнить» и смотрит, ЧТО написано на экране, когда запрос не удался.
import { chromium } from "playwright";
import { DEFAULT_URL } from "./harness.mjs";

const BASE = process.env.FE || DEFAULT_URL;
const PAY = process.env.PAY || "https://payment-worker-dev.vg-stavenko.workers.dev";
const SECRET = process.env.JWT_SECRET || "dev-secret-change-in-production";
const NAME = process.env.NAME || "Гречка";

let fail = 0;
const check = (n, ok, extra = "") => {
  console.log(`${ok ? "OK " : "FAIL"} ${n}${extra ? " — " + extra : ""}`);
  if (!ok) fail++;
};

const b64url = (buf) => Buffer.from(buf).toString("base64url");
const uid = `unavail-${Date.now()}`;
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

// Сбой модели воспроизводим сами: подменяем ответ ai-воркера на ту самую пятисотку
// с 4006. Так проверка не зависит от того, исчерпана квота прямо сейчас или нет.
// Адрес воркера узнаётся и по имени, и по проксирующему пути `/api/ai/…`: общий
// прогон подставляет свой сервер, и по имени воркера запрос там не опознаётся.
await page.route((u) => /\/chat\/completions/.test(String(u))
    && /ai-worker|\/api\/ai\//.test(String(u)), (route) =>
  route.fulfill({
    status: 500,
    contentType: "application/json",
    body: JSON.stringify({
      error: "AI.run: JsValue(AiError: 4006: you have used up your daily free allocation of " +
        "10,000 neurons, please upgrade to Cloudflare's Workers Paid plan if you would like to " +
        "continue usage.)",
    }),
  }));

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
await page.waitForTimeout(1500);
await page.locator('input[type="text"]').first().fill(NAME);
await page.getByText("Заполнить", { exact: true }).first().click();

// Ждём, пока попытки закончатся и на экране появится сообщение.
let text = "";
for (let i = 0; i < 40; i++) {
  text = (await page.locator("body").innerText()).replace(/ /g, " ");
  if (/Сервис временно недоступен/.test(text)) break;
  await page.waitForTimeout(2000);
}
const shown = (/Сервис временно недоступен[\s\S]{0,80}/.exec(text) || [""])[0].trim();
console.log("на экране:", JSON.stringify(shown));

check("человеку сказано понятной фразой", /Сервис временно недоступен/.test(text), shown);
check("назван код ошибки", /Код ошибки:\s*[0-9A-F]{6}/.test(text),
  (/Код ошибки:\s*\S+/.exec(text) || ["нет"])[0]);
check("сказано, что чиним", /Мы уже чиним/.test(text));
// Ни одного слова из технического дампа быть не должно.
for (const junk of ["LLM output error", "ModelExecution", "AiError", "neurons", "JsValue", "HTTP 500"]) {
  check(`дампа нет: «${junk}»`, !text.includes(junk));
}

await page.screenshot({ path: process.env.SHOT || "/tmp/service-unavailable.png" });
console.log(fail === 0 ? "\n=== ALL OK ===" : `\n=== FAILURES: ${fail} ===`);
await b.close();
process.exit(fail === 0 ? 0 : 1);
