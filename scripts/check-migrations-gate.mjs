// Отметка о прогоне миграций привязана к ИМЕНИ БАЗЫ, а не к сессии.
//
// Приложение стартует на гостевой базе `hjkl-ft`, а при входе переключается на
// `hjkl-ft-<user_id>`. Гостевая при этом НЕ копируется, значит версия из неё не
// переезжает — и базу пользователя надо мигрировать отдельно. Проверяем, что версия
// появляется в ОБЕИХ базах: иначе общая защёлка «один раз за сессию» оставила бы
// пользовательскую немигрированной, а именно она и содержит данные.
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

// Версия базы по ИМЕНИ базы. Открываем существующую: несуществующую indexedDB.open
// создал бы пустой, и «версии нет» было бы неотличимо от «базы нет».
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

const waitVersion = async (dbName, ms = 60000) => {
  const t0 = Date.now();
  let v = "";
  while (Date.now() - t0 < ms) {
    v = await versionOf(dbName);
    if (/^\d+$/.test(String(v))) return v;
    await page.waitForTimeout(2000);
  }
  return v;
};

// 1. ОНБОРДИНГ на гостевой базе. Сессии нет, но `initial_state` выдаёт входу на
//    /onboard состояние Ready — значит миграции здесь пройдут. Запрета на это нет:
//    важно лишь, что они отрабатывают вхолостую и не мешают. (Просто «/» без сессии
//    даёт Auth, а не Ready, и туда миграции не заходят вовсе.)
await page.goto(BASE, { waitUntil: "domcontentloaded" });
await page.evaluate(() => { localStorage.clear(); localStorage.setItem("pwa_dismissed", "true"); });
await page.goto(`${BASE}/onboard`, { waitUntil: "domcontentloaded" });
await page.waitForSelector("#splash", { state: "detached", timeout: 20000 }).catch(() => {});
const guest = await waitVersion("hjkl-ft");
console.log(`гостевая база hjkl-ft: версия ${guest}`);
check("на онбординге миграции проходят вхолостую и не падают",
  /^\d+$/.test(String(guest)), String(guest));

// 2. База пользователя: та же вкладка, сессия появилась. Версия обязана появиться и
//    здесь — своим прогоном, а не унаследованной из гостевой.
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
check("база пользователя мигрирована отдельно", /^\d+$/.test(String(own)), String(own));
check("версии сошлись", String(guest) === String(own), `гостевая ${guest}, своя ${own}`);

console.log(fail === 0 ? "\n=== ALL OK ===" : `\n=== FAILURES: ${fail} ===`);
await b.close();
process.exit(fail === 0 ? 0 : 1);
