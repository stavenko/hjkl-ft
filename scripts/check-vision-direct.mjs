// Сквозная проверка ПРЯМОГО пути картинок: приложение → ai-worker → сторонний
// провайдер (Qwen VL), без очереди ocr-queue.
//
// Гость сеется как в остальных проверках, но токен НАСТОЯЩИЙ (dev-JWT) и подписка
// активируется через payment-worker — иначе ai-worker закрыт гейтом 402.
//
// Usage: node scripts/check-vision-direct.mjs <frontend-url> <label.jpg> [dish.jpg]
import { chromium } from "playwright";
import { readFileSync } from "node:fs";

const FE = process.argv[2] || "https://renorma-fit-dev.pages.dev";
const LABEL = process.argv[3];
const DISH = process.argv[4];
const PAY = process.env.PAY || "https://payment-worker-dev.vg-stavenko.workers.dev";
const SECRET = process.env.JWT_SECRET || "dev-secret-change-in-production";
if (!LABEL) {
  console.error("нужен путь к фотографии этикетки");
  process.exit(1);
}

const uid = "vision-" + Date.now();

// dev-JWT + активная подписка (тот же приём, что в scripts/mint-ai-token.mjs).
const b64url = (buf) => Buffer.from(buf).toString("base64url");
const now = Math.floor(Date.now() / 1000);
const data = b64url(JSON.stringify({ alg: "HS256", typ: "JWT" })) + "." +
  b64url(JSON.stringify({ sub: uid, iat: now, exp: now + 7200, caps: [], token_id: "t" }));
const key = await crypto.subtle.importKey("raw", new TextEncoder().encode(SECRET),
  { name: "HMAC", hash: "SHA-256" }, false, ["sign"]);
const token = `${data}.${b64url(new Uint8Array(await crypto.subtle.sign("HMAC", key, new TextEncoder().encode(data))))}`;
const co = await (await fetch(`${PAY}/test/guest-checkout`, {
  method: "POST", headers: { "Content-Type": "application/json" },
  body: JSON.stringify({ planId: "test" }),
})).json();
const claim = await fetch(`${PAY}/claim`, {
  method: "POST",
  headers: { "Content-Type": "application/json", Authorization: `Bearer ${token}` },
  body: JSON.stringify({ claimId: co.claimId, secret: co.secret }),
});
if (!claim.ok) {
  console.error(`подписку активировать не удалось: ${claim.status} ${await claim.text()}`);
  process.exit(1);
}

const b = await chromium.launch({ headless: true });
const ctx = await b.newContext({ viewport: { width: 430, height: 920 }, serviceWorkers: "block", deviceScaleFactor: 2 });
const page = await ctx.newPage();
page.on("console", (m) => { if (m.type() === "error") console.log("CONSOLE ERROR:", m.text().slice(0, 300)); });

// Куда реально уходят картинки — это и есть предмет проверки.
const calls = [];
page.on("request", (r) => {
  const u = r.url();
  if (u.includes("/chat/completions") || u.includes("ocr-queue")) calls.push(`${r.method()} ${u}`);
});

await page.goto(FE, { waitUntil: "domcontentloaded" });
await page.evaluate(async ({ uid, token }) => {
  const del = (n) => new Promise((r) => { const q = indexedDB.deleteDatabase(n); q.onsuccess = q.onerror = q.onblocked = () => r(); });
  await del("hjkl-ft"); await del(`hjkl-ft-${uid}`);
  localStorage.clear();
  localStorage.setItem("user_id", uid);
  localStorage.setItem("auth_token", token);
  localStorage.setItem("pwa_dismissed", "true");
  localStorage.setItem("profile_sex", "male");
}, { uid, token });
await page.goto(FE, { waitUntil: "domcontentloaded" });

for (let i = 0; i < 40; i++) {
  const ok = await page.evaluate(async (uid) => {
    const dbs = await indexedDB.databases();
    if (!dbs.some((d) => d.name === `hjkl-ft-${uid}`)) return false;
    return await new Promise((res) => {
      const q = indexedDB.open(`hjkl-ft-${uid}`);
      q.onsuccess = () => { const ok = q.result.objectStoreNames.contains("app_flags"); q.result.close(); res(ok); };
      q.onerror = () => res(false);
    });
  }, uid).catch(() => false);
  if (ok) break;
  await page.waitForTimeout(500);
}
await page.evaluate(async (uid) => {
  const db = await new Promise((res, rej) => { const q = indexedDB.open(`hjkl-ft-${uid}`); q.onsuccess = () => res(q.result); q.onerror = () => rej(q.error); });
  const flags = [
    { key: "push_onboarding_dismissed", value: "true" },
    { key: "welcome_shown", value: "true" },
    { key: "ft_subscription", value: JSON.stringify({ plan: "monthly", end: Date.now() + 30 * 864e5, active: true, start: Date.now(), status: "paid", no_renew: false, provider: "lava" }) },
  ];
  await new Promise((res) => { const tx = db.transaction(["app_flags"], "readwrite"); const os = tx.objectStore("app_flags"); for (const f of flags) os.put(f); tx.oncomplete = res; });
  db.close();
}, uid);

/// Один прогон вкладки с фотографиями: подсунуть файл, нажать кнопку, дождаться,
/// пока кнопка перестанет считать секунды.
async function shot(tab, inputId, buttonText, file, outPng) {
  await page.goto(`${FE}/diary/add`, { waitUntil: "domcontentloaded" });
  await page.waitForSelector("#splash", { state: "detached", timeout: 20000 }).catch(() => {});
  await page.waitForTimeout(1500);
  // /diary/add — это поиск по своим продуктам; редактор с вкладками открывается
  // кнопкой «Добавить новый продукт».
  // Во втором прогоне список уже не пуст, и кнопки может не быть — тогда редактор
  // открыт сразу.
  await page.getByText("Добавить новый продукт", { exact: false }).first()
    .click({ timeout: 5000 }).catch(() => {});
  await page.waitForTimeout(800);
  await page.getByText(tab, { exact: false }).first().click({ timeout: 8000 }).catch(async () => {
    console.log(`вкладка «${tab}» не найдена; на экране:\n` +
      (await page.evaluate(() => document.body.innerText)).slice(0, 400));
    throw new Error("вкладка не открылась");
  });
  await page.waitForTimeout(400);
  await page.setInputFiles(`#${inputId}`, file);
  await page.waitForTimeout(1200);
  const btn = page.getByRole("button", { name: buttonText }).first();
  await btn.click();
  const started = Date.now();
  let last = "";
  while (Date.now() - started < 180000) {
    last = (await btn.textContent()) || "";
    if (!/с$/.test(last.trim())) break;
    await page.waitForTimeout(1000);
  }
  const secs = Math.round((Date.now() - started) / 1000);
  await page.screenshot({ path: outPng, fullPage: true });
  const values = await page.evaluate(() =>
    [...document.querySelectorAll("input")].map((i) => `${i.getAttribute("placeholder") || i.id || ""}=${i.value}`).filter((s) => s.includes("=") && s.split("=")[1]));
  const err = await page.evaluate(() => {
    const n = [...document.querySelectorAll("*")].find((e) => e.children.length === 0 && /не удалось|ошибк|error/i.test(e.textContent || ""));
    return n ? n.textContent.trim().slice(0, 200) : null;
  });
  console.log(`\n=== ${tab} · ${secs} с`);
  console.log("поля:", JSON.stringify(values));
  if (err) console.log("ОШИБКА В UI:", err);
}

// Одна вкладка за прогон: после первого распознавания список продуктов уже не
// пуст и путь до редактора другой — чистый прогон надёжнее ветвления.
if (DISH === "dish") {
  await shot("По фото еды", "fitems-photo-input", /Определить еду/, LABEL, "vision-direct-dish.png");
} else {
  await shot("По этикетке", "food-photo-input", /Определить еду/, LABEL, "vision-direct-label.png");
}

console.log("\nзапросы:", JSON.stringify(calls, null, 1));
await b.close();
