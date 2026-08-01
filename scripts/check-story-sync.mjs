// Прогресс историй должен появляться на втором устройстве СРАЗУ, без перезапуска.
// Репро бага: stories::init() засевает набор просмотренных кадров при старте —
// раньше первого синка сессии, — поэтому прилетевший синком прогресс виден
// только со следующего запуска.
import { chromium } from "playwright";
const FE = process.env.FE || "https://renorma-fit-dev.pages.dev";
const SYNC = "https://sync-worker-dev.vg-stavenko.workers.dev";
const PAY = "https://payment-worker-dev.vg-stavenko.workers.dev";
const SECRET = "dev-secret-change-in-production";

let fail = 0;
const check = (name, ok, extra = "") => { console.log(`${ok ? "OK " : "FAIL"} ${name}${extra ? " — " + extra : ""}`); if (!ok) fail++; };

const b64url = (buf) => Buffer.from(buf).toString("base64url");
async function signJwt(payload) {
  const enc = new TextEncoder();
  const data = b64url(JSON.stringify({ alg: "HS256", typ: "JWT" })) + "." + b64url(JSON.stringify(payload));
  const key = await crypto.subtle.importKey("raw", enc.encode(SECRET), { name: "HMAC", hash: "SHA-256" }, false, ["sign"]);
  const sig = await crypto.subtle.sign("HMAC", key, enc.encode(data));
  return data + "." + b64url(new Uint8Array(sig));
}
const uid = `v2story-${Date.now()}`;
const nowSec = Math.floor(Date.now() / 1000);
const token = await signJwt({ sub: uid, iat: nowSec, exp: nowSec + 3650 * 86400, caps: [], token_id: "tok1" });
{
  const co = await fetch(PAY + "/test/guest-checkout", { method: "POST", headers: { "Content-Type": "application/json" }, body: JSON.stringify({ planId: "test" }) });
  if (!co.ok) { console.error(`guest-checkout failed: HTTP ${co.status}`); process.exit(1); }
  const { claimId, secret } = await co.json();
  const cl = await fetch(PAY + "/claim", { method: "POST", headers: { "Content-Type": "application/json", Authorization: "Bearer " + token }, body: JSON.stringify({ claimId, secret }) });
  if (!cl.ok) { console.error(`claim failed: HTTP ${cl.status}`); process.exit(1); }
}
console.log("account:", uid, "| FE:", FE);

const sp = (path, body) => fetch(SYNC + path, {
  method: "POST", headers: { Authorization: `Bearer ${token}`, "Content-Type": "application/json" }, body: JSON.stringify(body),
});
const seed = async (page) => {
  await page.goto(FE, { waitUntil: "domcontentloaded" }).catch(() => {});
  await page.evaluate(({ uid, token }) => {
    localStorage.clear();
    localStorage.setItem("user_id", uid); localStorage.setItem("auth_token", token);
    localStorage.setItem("token_id", "tok1"); localStorage.setItem("auth_ctx", "browser");
    localStorage.setItem("pwa_dismissed", "true");
    localStorage.setItem("paywall_skipped_date", new Date().toISOString().slice(0, 10));
  }, { uid, token });
};
/// Заполнить персону (рост/год/пол) — без неё дашборд занят экраном «Персона»
/// и трея историй нет.
const seedPersona = (page) => page.evaluate((uid) => new Promise((res) => {
  const q = indexedDB.open(`hjkl-ft-${uid}`);
  q.onsuccess = () => {
    const db = q.result;
    const tx = db.transaction(["profile", "_outbox"], "readwrite");
    tx.objectStore("profile").put({
      key: "profile", sex: "male", height_cm: 180, birth_year: 1985, goal: "lose",
      cycle_start: null, steps_planka: null, updated_at: new Date().toISOString(),
    });
    // Как настоящая мутация: строка обязана уехать в журнал, иначе устройство B
    // после adopt останется без персоны (adopt стирает локальный profile).
    tx.objectStore("_outbox").put({
      seq: String(Date.now() * 1000).padStart(20, "0"),
      store: "profile", op: "upsert", id: "profile", ts: Date.now(),
    });
    tx.oncomplete = () => { db.close(); res(true); };
    tx.onerror = () => { db.close(); res(false); };
  };
  q.onerror = () => res(false);
}), uid);

/// Есть ли у кружка истории акцентная дуга (= остались непросмотренные кадры).
const hasArc = (page, storyId) => page.evaluate((id) => {
  const btn = document.querySelector(`[data-testid="story-circle"][data-story-id="${id}"]`);
  if (!btn) return null;
  return [...btn.querySelectorAll("circle")].some(c => (c.getAttribute("stroke") || "").toLowerCase() === "#34d399");
}, storyId);
const viewedFlag = (page) => page.evaluate((uid) => new Promise((res) => {
  const q = indexedDB.open(`hjkl-ft-${uid}`);
  q.onsuccess = () => { const db = q.result; const tx = db.transaction(["app_flags"], "readonly");
    const rq = tx.objectStore("app_flags").get("story_viewed");
    rq.onsuccess = () => { db.close(); res(rq.result ? JSON.parse(rq.result.value) : null); };
    rq.onerror = () => { db.close(); res(null); }; };
  q.onerror = () => res(null);
}), uid);

const b = await chromium.launch({ headless: true });

// ── Устройство A: просматривает историю «?» целиком через UI ──
const ctxA = await b.newContext({ viewport: { width: 430, height: 920 }, serviceWorkers: "block" });
const A = await ctxA.newPage();
A.on("console", (m) => { const t = m.text(); if (/panicked/.test(t)) console.log("[A]", t.slice(0, 200)); });
await seed(A);
await A.goto(FE, { waitUntil: "domcontentloaded" });
await A.waitForTimeout(12000); // бутстрап: миграция пустой базы + init
await seedPersona(A);
await A.reload({ waitUntil: "domcontentloaded" });
await A.waitForTimeout(10000);
const circle = A.locator('[data-testid="story-circle"][data-story-id="welcome"]');
check("A: кружок истории отрисован", await circle.isVisible().catch(() => false));
check("A: до просмотра дуга есть", (await hasArc(A, "welcome")) === true);
await circle.click();
await A.waitForTimeout(700);
// Правая зона = «дальше»; кликаем, пока просмотрщик не закроется сам на последнем кадре.
for (let i = 0; i < 60; i++) {
  const open = await A.locator("text=×").first().isVisible().catch(() => false);
  if (!open) break;
  await A.mouse.click(360, 500);
  await A.waitForTimeout(180);
}
const flagA = await viewedFlag(A);
check("A: кадры отмечены просмотренными", Array.isArray(flagA) && flagA.length >= 5, `hashes=${flagA?.length}`);
check("A: дуга пропала после просмотра", (await hasArc(A, "welcome")) === false);
// Прогресс лежит в outbox; синк запускается на старте/фокусе — перезапускаем A.
let onServer = false;
for (let i = 0; i < 6 && !onServer; i++) {
  await A.reload({ waitUntil: "domcontentloaded" });
  await A.waitForTimeout(8000);
  const r = await sp("/sync/v2/pull", { since_version: 0 });
  if (r.status === 200) onServer = JSON.stringify(await r.json()).includes("story_viewed");
}
check("A: прогресс уехал в журнал", onServer);

// ── Устройство B: трей уже на экране, синк прилетает В ЭТУ ЖЕ сессию ──
// Первый заход — с заблокированным синком: создаём базу и персону, чтобы дашборд
// (а с ним трей) рисовался сразу. Меряем ВТОРУЮ сессию: в ней бутстрап приносит
// прогресс историй, и кольцо обязано обновиться без перезапуска.
const ctxB = await b.newContext({ viewport: { width: 430, height: 920 }, serviceWorkers: "block" });
await ctxB.route(`${SYNC}/**`, (r) => r.abort());
const B = await ctxB.newPage();
B.on("console", (m) => { const t = m.text(); if (/panicked/.test(t)) console.log("[B]", t.slice(0, 200)); });
await seed(B);
await B.goto(FE, { waitUntil: "domcontentloaded" });
await B.waitForTimeout(9000);
await seedPersona(B);
// Вторая сессия стартует с ЗАБЛОКИРОВАННЫМ синком: так состояние «до синка»
// проверяется детерминированно, без гонки с бутстрапом.
await B.reload({ waitUntil: "domcontentloaded" });
await B.waitForTimeout(7000);
check("B: трей историй на экране до прихода синка", await B.locator('[data-testid="story-circle"][data-story-id="welcome"]').isVisible().catch(() => false));
check("B: до синка кольцо непросмотренное", (await hasArc(B, "welcome")) === true);
check("B: до синка прогресса в базе нет", (await viewedFlag(B)) === null);
// Сеть вернулась + приложение вышло на передний план → синк БЕЗ перезапуска.
await ctxB.unroute(`${SYNC}/**`);
await B.evaluate(() => document.dispatchEvent(new Event("visibilitychange")));
let liveOk = false, flagArrived = false;
for (let i = 0; i < 20 && !liveOk; i++) {
  await B.waitForTimeout(3000);
  liveOk = (await hasArc(B, "welcome")) === false;
  if (!flagArrived) flagArrived = Array.isArray(await viewedFlag(B));
}
check("B: данные прогресса доехали в базу", flagArrived);
check("B: кольцо БЕЗ перезапуска показывает просмотренную историю", liveOk);

// Контроль: после перезапуска состояние обязано быть верным в любом случае.
await B.reload({ waitUntil: "domcontentloaded" });
await B.waitForTimeout(9000);
check("B: после перезапуска кольцо корректное", (await hasArc(B, "welcome")) === false);

console.log(fail === 0 ? "\n=== ALL OK ===" : `\n=== FAILURES: ${fail} ===`);
await b.close();
process.exit(fail === 0 ? 0 : 1);
