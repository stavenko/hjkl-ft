// Sync v2: массовая миграция (200 суточных батчей) + скриншот прогресс-бара +
// обрыв миграции на середине с рестартом «по новой». Свежие dev-аккаунты.
import { chromium } from "playwright";
const FE = "https://renorma-fit-dev.pages.dev";
const SYNC = "https://sync-worker-dev.vg-stavenko.workers.dev";
const PAY = "https://payment-worker-dev.vg-stavenko.workers.dev";
const SECRET = "dev-secret-change-in-production";
const SHOT = process.env.SHOT_PATH || "/tmp/migration-progress.png";

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
async function mintAccount(tag) {
  const u = `${tag}-${Date.now()}`;
  const nowSec = Math.floor(Date.now() / 1000);
  const t = await signJwt({ sub: u, iat: nowSec, exp: nowSec + 3650 * 86400, caps: [], token_id: "tok1" });
  const co = await fetch(PAY + "/test/guest-checkout", { method: "POST", headers: { "Content-Type": "application/json" }, body: JSON.stringify({ planId: "test" }) });
  if (!co.ok) { console.error(`guest-checkout failed: HTTP ${co.status}`); process.exit(1); }
  const { claimId, secret } = await co.json();
  const cl = await fetch(PAY + "/claim", { method: "POST", headers: { "Content-Type": "application/json", Authorization: "Bearer " + t }, body: JSON.stringify({ claimId, secret }) });
  if (!cl.ok) { console.error(`claim failed: HTTP ${cl.status}`); process.exit(1); }
  return { u, t };
}
const sp = (t, path, body) => fetch(SYNC + path, {
  method: "POST",
  headers: { Authorization: `Bearer ${t}`, "Content-Type": "application/json" },
  body: JSON.stringify(body),
});

const seed = async (page, u, t) => {
  page.__uid = u;
  await page.goto(FE, { waitUntil: "domcontentloaded" }).catch(() => {});
  await page.evaluate(({ u, t }) => {
    localStorage.clear();
    localStorage.setItem("user_id", u); localStorage.setItem("auth_token", t);
    localStorage.setItem("token_id", "tok1"); localStorage.setItem("auth_ctx", "browser");
    localStorage.setItem("pwa_dismissed", "true");
    localStorage.setItem("paywall_skipped_date", new Date().toISOString().slice(0, 10));
  }, { u, t });
};
const idb = (page, fn, arg) => page.evaluate(fn, { uid: page.__uid, arg });
const getVersion = (page) => idb(page, async ({ uid }) => {
  const db = await new Promise((res, rej) => { const q = indexedDB.open(`hjkl-ft-${uid}`); q.onsuccess = () => res(q.result); q.onerror = () => rej(q.error); });
  const row = await new Promise((res) => { const tx = db.transaction(["_sync_meta"], "readonly"); const rq = tx.objectStore("_sync_meta").get("v2_version"); rq.onsuccess = () => res(rq.result); rq.onerror = () => res(null); });
  db.close(); return row ? Number(row.value) : null;
});
const diaryCount = (page) => idb(page, async ({ uid }) => {
  const db = await new Promise((res, rej) => { const q = indexedDB.open(`hjkl-ft-${uid}`); q.onsuccess = () => res(q.result); q.onerror = () => rej(q.error); });
  const n = await new Promise((res) => { const tx = db.transaction(["diary"], "readonly"); const rq = tx.objectStore("diary").count(); rq.onsuccess = () => res(rq.result); });
  db.close(); return n;
});

const blockSync = (ctx) => ctx.route(`${SYNC}/**`, (r) => r.abort());
const unblockSync = (ctx) => ctx.unroute(`${SYNC}/**`);
const dayStr = (offsetDays) => new Date(Date.now() - offsetDays * 86400e3 - 4 * 3600e3).toLocaleDateString("sv");
const isoAt = (day) => `${day}T10:00:00.000Z`;

/// Залить в per-user БД fixture: одна еда + запись дневника на каждые из N суток
/// (+ вес каждые 10 суток) — N суточных батчей миграции.
const seedDays = (page, prefix, nDays) => idb(page, async ({ uid, arg }) => {
  const db = await new Promise((res, rej) => { const q = indexedDB.open(`hjkl-ft-${uid}`); q.onsuccess = () => res(q.result); q.onerror = () => rej(q.error); });
  await new Promise((res) => {
    const tx = db.transaction(["foods", "diary", "weight_entries"], "readwrite");
    const f0 = arg.days[arg.days.length - 1].iso;
    tx.objectStore("foods").put({ id: `${arg.prefix}-food`, name: `Миграционная еда ${arg.prefix}`, kcal: 100, protein: 5, fat: 2, carbs: 10, nutrients: {}, package_weight: null, is_recipe: false, recipe_id: null, archived: false, created_at: f0, updated_at: f0 });
    for (const [i, d] of arg.days.entries()) {
      tx.objectStore("diary").put({ id: `${arg.prefix}-e${i + 1}`, food_id: `${arg.prefix}-food`, date: d.day, time: "12:00", grams: 100 + i, waste_grams: 0, meal_label: "lunch", deleted: false, created_at: d.iso, updated_at: d.iso });
      if ((i + 1) % 10 === 0) {
        tx.objectStore("weight_entries").put({ id: `${arg.prefix}-w${i + 1}`, date: d.day, weight_kg: 80 - i * 0.01, no_water: false, no_food: false, no_wash: false, used_toilet: false, morning: true, created_at: d.iso, updated_at: d.iso });
      }
    }
    tx.oncomplete = res;
  });
  db.close();
}, { prefix, days: Array.from({ length: nDays }, (_, i) => ({ day: dayStr(i + 1), iso: isoAt(dayStr(i + 1)) })) });

const waitVersion = async (page, tries, stepMs) => {
  let v = null;
  for (let i = 0; i < tries && v === null; i++) { await page.waitForTimeout(stepMs); v = await getVersion(page); }
  return v;
};

const b = await chromium.launch({ headless: true });

// ── 1. Массовая миграция: 200 суток, прогресс-бар, скриншот ──
const big = await mintAccount("v2big");
console.log("big account:", big.u);
const ctxD = await b.newContext({ viewport: { width: 430, height: 920 }, serviceWorkers: "block" });
await blockSync(ctxD);
const D = await ctxD.newPage();
D.on("console", (m) => { const t = m.text(); if (/panicked|sync v2:/.test(t)) console.log("[D]", t.slice(0, 200)); });
await seed(D, big.u, big.t);
await D.goto(FE, { waitUntil: "domcontentloaded" });
await D.waitForTimeout(9000);
await seedDays(D, "big", 200);
await unblockSync(ctxD);
await D.reload({ waitUntil: "domcontentloaded" });

const overlay = D.locator('[data-testid="migration-overlay"]');
const appeared = await overlay.waitFor({ state: "visible", timeout: 30000 }).then(() => true).catch(() => false);
check("массовая миграция: страница «Делаем миграцию данных» показана", appeared);
if (appeared) {
  // Дать прогрессу отъехать от нуля и снять кадр.
  await D.waitForTimeout(6000);
  await D.screenshot({ path: SHOT });
  console.log("  скриншот:", SHOT, "| текст:", (await overlay.innerText().catch(() => "")).replace(/\s+/g, " ").slice(0, 120));
}
const verD = await waitVersion(D, 60, 4000);
check("массовая миграция: завершилась, версия установлена", Number.isInteger(verD) && verD >= 200, `v=${verD}`);
check("массовая миграция: оверлей скрыт после завершения", !(await overlay.isVisible().catch(() => false)));
const rpD = await sp(big.t, "/sync/v2/pull", { since_version: 0 });
const jD = rpD.status === 200 ? await rpD.json() : { version: -1, batches: [] };
check("массовая миграция: store инициализирован, батчей == версии", rpD.status === 200 && jD.version === verD && jD.batches.length === jD.version, `HTTP ${rpD.status}, v=${jD.version}`);
const jStr = JSON.stringify(jD.batches);
check("массовая миграция: крайние записи (1-я и 200-я) в журнале", jStr.includes('"big-e1"') && jStr.includes('"big-e200"'));

// ── 2. Обрыв миграции на середине → рестарт по новой → init ──
const intr = await mintAccount("v2intr");
console.log("interrupt account:", intr.u);
const ctxE = await b.newContext({ viewport: { width: 430, height: 920 }, serviceWorkers: "block" });
await blockSync(ctxE);
const E = await ctxE.newPage();
E.on("console", (m) => { const t = m.text(); if (/panicked|sync v2:/.test(t)) console.log("[E]", t.slice(0, 200)); });
await seed(E, intr.u, intr.t);
await E.goto(FE, { waitUntil: "domcontentloaded" });
await E.waitForTimeout(9000);
await seedDays(E, "intr", 60);
await unblockSync(ctxE);
// Пропустить 15 батчей, дальше рвать сеть — обрыв на середине миграции.
let pushCount = 0;
await ctxE.route(`${SYNC}/sync/v2/push`, (r) => { pushCount++; if (pushCount > 15) r.abort(); else r.continue(); });
await E.reload({ waitUntil: "domcontentloaded" });
await E.waitForTimeout(20000);
const verE0 = await getVersion(E);
check("обрыв: версия НЕ установлена (init не был вызван)", verE0 === null, `v=${verE0}, pushes=${pushCount}`);
const probe = await sp(intr.t, "/sync/v2/pull", { since_version: 0 });
check("обрыв: store остался неинициализированным", probe.status === 409, `HTTP ${probe.status}`);
// Сеть вернулась → рестарт миграции по новой (поверх уже залитых батчей).
await ctxE.unroute(`${SYNC}/sync/v2/push`);
await E.reload({ waitUntil: "domcontentloaded" });
const verE = await waitVersion(E, 45, 4000);
check("обрыв: повторная миграция завершилась, версия установлена", Number.isInteger(verE) && verE > 60, `v=${verE}`);
const rpE = await sp(intr.t, "/sync/v2/pull", { since_version: 0 });
const jE = rpE.status === 200 ? await rpE.json() : { version: -1, batches: [] };
check("обрыв: store инициализирован, батчей == версии", rpE.status === 200 && jE.version === verE && jE.batches.length === jE.version, `HTTP ${rpE.status}, v=${jE.version}`);

// Свежий клиент F реплеит журнал (с повторёнными батчами) → данные корректны.
const ctxF = await b.newContext({ viewport: { width: 430, height: 920 }, serviceWorkers: "block" });
const F = await ctxF.newPage();
F.on("console", (m) => { const t = m.text(); if (/panicked|sync v2:/.test(t)) console.log("[F]", t.slice(0, 200)); });
await seed(F, intr.u, intr.t);
await F.goto(FE, { waitUntil: "domcontentloaded" });
await F.waitForTimeout(18000);
let dc = 0;
for (let i = 0; i < 5; i++) {
  dc = await diaryCount(F);
  if (dc === 60) break;
  await F.reload({ waitUntil: "domcontentloaded" }); await F.waitForTimeout(8000);
}
check("обрыв: свежий клиент собрал ровно все записи из журнала с повторами", dc === 60, `diary=${dc}`);

console.log(fail === 0 ? "\n=== ALL OK ===" : `\n=== FAILURES: ${fail} ===`);
await b.close();
process.exit(fail === 0 ? 0 : 1);
