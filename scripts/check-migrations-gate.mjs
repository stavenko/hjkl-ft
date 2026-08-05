// База принадлежит ПОЛЬЗОВАТЕЛЮ и открывается только для него.
//
// Device-global «hjkl-ft» больше нет: до входа базы не существует вовсе, а миграции
// идут в базе пользователя. Проверяем три вещи на одной вкладке:
//   1. без сессии база не заводится ни под каким именем;
//   2. после входа база пользователя появляется и мигрируется;
//   3. миграция 2 удаляет старую «hjkl-ft», если та осталась с прежних сборок.
import { chromium } from "playwright";

const BASE = process.env.FE || "https://renorma-fit-dev.pages.dev";
const PAY = process.env.PAY || "https://payment-worker-dev.vg-stavenko.workers.dev";
const SECRET = process.env.JWT_SECRET || "dev-secret-change-in-production";

let fail = 0;
const check = (n, ok, extra = "") => { console.log(`${ok ? "OK " : "FAIL"} ${n}${extra ? " — " + extra : ""}`); if (!ok) fail++; };

const b64url = (buf) => Buffer.from(buf).toString("base64url");
const uid = `mgate-${Date.now()}`;
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

const listDbs = () => page.evaluate(async () => (await indexedDB.databases()).map((d) => d.name));

const versionOf = (dbName) => page.evaluate(async (name) => {
  const dbs = await indexedDB.databases();
  if (!dbs.some((d) => d.name === name)) return "базы нет";
  const db = await new Promise((res, rej) => {
    const q = indexedDB.open(name);
    q.onsuccess = () => res(q.result); q.onerror = () => rej(q.error);
  });
  if (!db.objectStoreNames.contains("app_flags")) { db.close(); return "стора нет"; }
  const rows = await new Promise((res) => {
    const rq = db.transaction(["app_flags"], "readonly").objectStore("app_flags").getAll();
    rq.onsuccess = () => res(rq.result); rq.onerror = () => res([]);
  });
  db.close();
  return rows.find((r) => r.key === "db_schema_version")?.value ?? "нет версии";
}, dbName);

const waitVersion = async (dbName, ms = 90000) => {
  const t0 = Date.now();
  let v = "";
  while (Date.now() - t0 < ms) {
    v = await versionOf(dbName);
    if (/^\d+$/.test(String(v))) return v;
    await page.waitForTimeout(2000);
  }
  return v;
};

// 1. БЕЗ СЕССИИ. Заходим на онбординг — состояние там Ready, то есть все условия
//    запуска миграций выполнены. Базы всё равно не должно появиться: открывать
//    нечего, пользователь неизвестен.
await page.goto(BASE, { waitUntil: "domcontentloaded" });
await page.evaluate(() => { localStorage.clear(); localStorage.setItem("pwa_dismissed", "true"); });
// Заводим «hjkl-ft» руками — как она осталась бы от прежних сборок. Миграция 2
// обязана её убрать после входа.
await page.evaluate(() => new Promise((res) => {
  const q = indexedDB.open("hjkl-ft", 1);
  q.onupgradeneeded = () => q.result.createObjectStore("legacy", { keyPath: "id" });
  q.onsuccess = () => { q.result.close(); res(); };
  q.onerror = () => res();
}));
await page.goto(`${BASE}/onboard`, { waitUntil: "domcontentloaded" });
await page.waitForSelector("#splash", { state: "detached", timeout: 20000 }).catch(() => {});
await page.waitForTimeout(12000);
const before = await listDbs();
console.log(`без сессии базы: ${JSON.stringify(before)}`);
check("без сессии база пользователя не заводится",
  !before.some((n) => String(n).startsWith("hjkl-ft-")), JSON.stringify(before));

// 2. ПОСЛЕ ВХОДА. База пользователя появляется и мигрируется.
await page.evaluate(({ uid, token }) => {
  localStorage.setItem("user_id", uid);
  localStorage.setItem("auth_token", token);
  localStorage.setItem("token_id", "t");
  localStorage.setItem("auth_ctx", "browser");
  localStorage.setItem("pwa_dismissed", "true");
}, { uid, token });
await page.goto(BASE, { waitUntil: "domcontentloaded" });
await page.waitForSelector("#splash", { state: "detached", timeout: 20000 }).catch(() => {});
const own = await waitVersion(`hjkl-ft-${uid}`);
console.log(`база пользователя hjkl-ft-${uid}: версия ${own}`);
check("база пользователя создана и мигрирована", /^\d+$/.test(String(own)), String(own));

// 3. Миграция 2 убрала старую device-global базу.
const after = await listDbs();
console.log(`после входа базы: ${JSON.stringify(after)}`);
check("старая база hjkl-ft удалена", !after.includes("hjkl-ft"), JSON.stringify(after));

console.log(fail === 0 ? "\n=== ALL OK ===" : `\n=== FAILURES: ${fail} ===`);
await b.close();
process.exit(fail === 0 ? 0 : 1);
