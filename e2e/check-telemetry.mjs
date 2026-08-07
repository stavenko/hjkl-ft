// Доходит ли ошибка из БРАУЗЕРА до нашего хранилища.
//
// curl'ом ручка проверена отдельно; здесь проверяется то, чего curl не видит:
// CORS с настоящего источника приложения и живой JWT из его localStorage.
// Отправляем событие со страницы приложения и вычитываем его обратно из
// Analytics Engine по коду, который сами же и придумали.
//
// Прогон: node check-telemetry.mjs
import { chromium } from "playwright";
import { execSync } from "node:child_process";
import { readFileSync as read } from "node:fs";

const BASE = process.env.BASE || "https://renorma-fit-dev.pages.dev";
const BUG = process.env.BUG || "https://bug-report-worker-dev.vg-stavenko.workers.dev";
const DATASET = process.env.DATASET || "renorma_client_errors_dev";
const ACCOUNT = "a2809d140765b9e7915c5644c249863b";

const CF_TOKEN = read("../.env", "utf8")
  .split("\n")
  .find((l) => l.startsWith("CLOUDFLARE_API_TOKEN="))
  ?.split("=")
  .slice(1)
  .join("=")
  .trim()
  .replace(/^['"]|['"]$/g, "");

let failed = 0;
const check = (ok, what) => {
  console.log(`${ok ? "  ok  " : "  ПРОВАЛ "} ${what}`);
  if (!ok) failed++;
};

// Свой аккаунт на прогон — чтобы событие нельзя было спутать с чужим.
const uid = `telemetry-${Date.now().toString(36)}`;
const token = execSync(`node ../frontend/scripts/seed-test-subscription.mjs ${uid}`, {
  encoding: "utf8",
}).match(/^token:\s+(\S+)$/m)[1];
// Метка прогона: по ней и найдём запись.
const marker = `проба ${uid}`;

const b = await chromium.launch();
const ctx = await b.newContext({ viewport: { width: 390, height: 844 } });
await ctx.addInitScript(
  ([u, t]) => {
    sessionStorage.setItem("update_auto_applied", "1");
    localStorage.setItem("user_id", u);
    localStorage.setItem("auth_token", t);
    localStorage.setItem("token_id", "tok1");
    localStorage.setItem("auth_ctx", "browser");
  },
  [uid, token],
);
const page = await ctx.newPage();
await page.goto(`${BASE}/`, { waitUntil: "domcontentloaded" });
await page.waitForTimeout(3000);

// Отправка ИЗ СТРАНИЦЫ приложения: тот же источник, те же заголовки, что у
// настоящей телеметрии.
const status = await page.evaluate(
  async ([bug, tok, mark]) => {
    const r = await fetch(`${bug}/event`, {
      method: "POST",
      headers: { "Content-Type": "application/json", Authorization: `Bearer ${tok}` },
      body: JSON.stringify({
        code: "AAA111",
        kind: "probe.browser",
        subject: "проба из браузера",
        cause: mark,
        build: "e2e",
        platform: "android_chrome",
      }),
    });
    return r.status;
  },
  [BUG, token, marker],
);
console.log("\n1. Отправка со страницы приложения");
check(status === 202, `воркер принял (HTTP ${status})`);
await b.close();

if (!CF_TOKEN) {
  console.log("  (CLOUDFLARE_API_TOKEN не найден в .env — чтение пропущено)");
  process.exit(failed ? 1 : 0);
}

// Точка становится видимой в SQL не сразу — ждём, но не вечно.
console.log("\n2. Чтение обратно из Analytics Engine");
const sql = async (q) => {
  const r = await fetch(
    `https://api.cloudflare.com/client/v4/accounts/${ACCOUNT}/analytics_engine/sql`,
    { method: "POST", headers: { Authorization: `Bearer ${CF_TOKEN}` }, body: q },
  );
  const text = await r.text();
  try {
    return JSON.parse(text);
  } catch {
    // Ответ не-JSON — это отказ самого SQL API; показываем как есть, иначе
    // разбор упадёт неинформативной синтаксической ошибкой.
    console.log("   SQL API:", text.slice(0, 200));
    return {};
  }
};
let row = null;
for (let i = 0; i < 12 && !row; i++) {
  await new Promise((r) => setTimeout(r, 10000));
  const res = await sql(
    // `timestamp` обязан быть в SELECT: сортировать по невыбранному столбцу
    // Analytics Engine не умеет и отвечает «unable to find type of column».
    `SELECT index1 AS code, blob1 AS kind, blob3 AS cause, blob6 AS user, timestamp
     FROM ${DATASET} WHERE blob6 = '${uid}' ORDER BY timestamp DESC LIMIT 1 FORMAT JSON`,
  );
  row = (res.data || [])[0] || null;
}
check(!!row, "запись найдена");
if (row) {
  console.log("   ", JSON.stringify(row));
  check(row.kind === "probe.browser", "вид сохранился");
  check(row.cause === marker, "причина сохранилась целиком");
  check(row.user === uid, "пользователь тот самый");
}

console.log(failed ? `\nПРОВАЛОВ: ${failed}` : "\nвсё сошлось");
process.exit(failed ? 1 : 0);
