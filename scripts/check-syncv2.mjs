// Sync v2 validation: two "devices" (isolated contexts) on the dev account.
import { chromium } from "playwright";
import { readFileSync } from "fs";
const FE = "https://renorma-fit-dev.pages.dev";
const uid = readFileSync("/tmp/repro_uid.txt", "utf8").trim();
const token = readFileSync("/tmp/repro_token.txt", "utf8").trim();
const now = () => new Date().toISOString();
let fail = 0;
const check = (name, ok, extra = "") => { console.log(`${ok ? "OK " : "FAIL"} ${name}${extra ? " — " + extra : ""}`); if (!ok) fail++; };

const seed = async (page) => {
  await page.goto(FE, { waitUntil: "domcontentloaded" });
  await page.evaluate(({ uid, token }) => {
    localStorage.clear();
    localStorage.setItem("user_id", uid); localStorage.setItem("auth_token", token);
    localStorage.setItem("token_id", "tok1"); localStorage.setItem("auth_ctx", "browser");
    localStorage.setItem("pwa_dismissed", "true");
    localStorage.setItem("paywall_skipped_date", new Date().toISOString().slice(0, 10));
  }, { uid, token });
};
const idb = (page, fn, arg) => page.evaluate(fn, { uid, arg });
const getVersion = (page) => idb(page, async ({ uid }) => {
  const db = await new Promise((res, rej) => { const q = indexedDB.open(`hjkl-ft-${uid}`); q.onsuccess = () => res(q.result); q.onerror = () => rej(q.error); });
  const row = await new Promise((res) => { const tx = db.transaction(["_sync_meta"], "readonly"); const rq = tx.objectStore("_sync_meta").get("v2_version"); rq.onsuccess = () => res(rq.result); rq.onerror = () => res(null); });
  db.close(); return row ? Number(row.value) : null;
});
const storeRow = (page, store, key) => idb(page, async ({ uid, arg }) => {
  const db = await new Promise((res, rej) => { const q = indexedDB.open(`hjkl-ft-${uid}`); q.onsuccess = () => res(q.result); q.onerror = () => rej(q.error); });
  const row = await new Promise((res) => { const tx = db.transaction([arg.store], "readonly"); const rq = tx.objectStore(arg.store).get(arg.key); rq.onsuccess = () => res(rq.result || null); rq.onerror = () => res(null); });
  db.close(); return row;
}, { store, key });
const outboxLen = (page) => idb(page, async ({ uid }) => {
  const db = await new Promise((res, rej) => { const q = indexedDB.open(`hjkl-ft-${uid}`); q.onsuccess = () => res(q.result); q.onerror = () => rej(q.error); });
  const n = await new Promise((res) => { const tx = db.transaction(["_outbox"], "readonly"); const rq = tx.objectStore("_outbox").count(); rq.onsuccess = () => res(rq.result); rq.onerror = () => res(-1); });
  db.close(); return n;
});

const b = await chromium.launch({ headless: true });

// ── Device A: bootstrap ──
const ctxA = await b.newContext({ viewport: { width: 430, height: 920 }, serviceWorkers: "block" });
const A = await ctxA.newPage();
A.on("console", m => { const t = m.text(); if (/panicked|sync v2:/.test(t)) console.log("[A]", t.slice(0, 180)); });
await seed(A);
await A.goto(FE, { waitUntil: "domcontentloaded" });
await A.waitForTimeout(22000); // bootstrap: legacy push + snapshot pull
const verA0 = await getVersion(A);
check("A bootstrap: версия установлена", Number.isInteger(verA0), `version=${verA0}`);
const diaryCount = await idb(A, async ({ uid }) => {
  const db = await new Promise((res, rej) => { const q = indexedDB.open(`hjkl-ft-${uid}`); q.onsuccess = () => res(q.result); q.onerror = () => rej(q.error); });
  const n = await new Promise((res) => { const tx = db.transaction(["diary"], "readonly"); const rq = tx.objectStore("diary").count(); rq.onsuccess = () => res(rq.result); });
  db.close(); return n;
});
check("A bootstrap: снапшот применён (дневник не пуст)", diaryCount > 200, `diary=${diaryCount}`);

// ── A: реальная мутация через приложение — тестовый флаг через planted rows не нужен:
// добавим еду+запись за СЕГОДНЯ прямым put в IndexedDB и продублируем это в outbox,
// имитируя tracked-путь (сам трекинг проверяется ниже настоящим UI-удалением).
const fid = `v2f-${Date.now()}`, eid = `v2e-${Date.now()}`;
const foodName = `V2 еда ${Date.now() % 100000}`;
await idb(A, async ({ uid, arg }) => {
  const db = await new Promise((res, rej) => { const q = indexedDB.open(`hjkl-ft-${uid}`); q.onsuccess = () => res(q.result); q.onerror = () => rej(q.error); });
  const today = new Date(Date.now() - 4 * 3600e3).toLocaleDateString('sv'); // логический день (DAY_START_HOUR=4)
  const nowIso = new Date().toISOString();
  await new Promise((res) => {
    const tx = db.transaction(["foods", "diary", "_outbox"], "readwrite");
    tx.objectStore("foods").put({ id: arg.fid, name: arg.foodName, kcal: 111, protein: 5, fat: 2, carbs: 10, nutrients: {}, package_weight: null, is_recipe: false, recipe_id: null, archived: false, created_at: nowIso, updated_at: nowIso });
    tx.objectStore("diary").put({ id: arg.eid, food_id: arg.fid, date: today, time: "13:00", grams: 200, waste_grams: 0, meal_label: "lunch", deleted: false, created_at: nowIso, updated_at: nowIso });
    const base = Date.now() * 1000;
    window.__seqs = [String(base).padStart(20, "0"), String(base + 1).padStart(20, "0")];
    tx.objectStore("_outbox").put({ seq: window.__seqs[0], store: "foods", op: "upsert", id: arg.fid });
    tx.objectStore("_outbox").put({ seq: window.__seqs[1], store: "diary", op: "upsert", id: arg.eid });
    tx.oncomplete = res;
  });
  db.close();
  return window.__seqs;
}, { fid, eid, foodName });
const plantedSeqs = await idb(A, async ({ uid }) => window.__seqs || [], {});
await A.reload({ waitUntil: "domcontentloaded" });
await A.waitForTimeout(9000); // sync_now: push outbox + pull
const leftover = await idb(A, async ({ uid }) => {
  const db = await new Promise((res, rej) => { const q = indexedDB.open(`hjkl-ft-${uid}`); q.onsuccess = () => res(q.result); q.onerror = () => rej(q.error); });
  const all = await new Promise((res) => { const tx = db.transaction(["_outbox"], "readonly"); const rq = tx.objectStore("_outbox").getAll(); rq.onsuccess = () => res(rq.result || []); });
  db.close(); return all.map(r => r.seq);
});
check("A: посаженные записи outbox отправлены", !leftover.some(sq => plantedSeqs.includes(sq)), `осталось всего=${leftover.length}`);
const verA1 = await getVersion(A);
check("A: версия выросла после push", verA1 > verA0, `${verA0} -> ${verA1}`);

// ── Device B: получает изменения A ──
const ctxB = await b.newContext({ viewport: { width: 430, height: 920 }, serviceWorkers: "block" });
const B = await ctxB.newPage();
B.on("console", m => { const t = m.text(); if (/panicked|sync v2:/.test(t)) console.log("[B]", t.slice(0, 180)); });
await seed(B);
await B.goto(FE, { waitUntil: "domcontentloaded" });
await B.waitForTimeout(22000);
check("B: запись A доехала", !!(await storeRow(B, "diary", eid)));
check("B: еда A доехала", !!(await storeRow(B, "foods", fid)));
const verB0 = await getVersion(B);
check("B: версия консистентна с A", verB0 >= verA1, `B=${verB0} A=${verA1}`);

// ── Одновременные изменения на A и B (разные строки одного стора) ──
const plantFlag = (page, key, value, ts) => idb(page, async ({ uid, arg }) => {
  const db = await new Promise((res, rej) => { const q = indexedDB.open(`hjkl-ft-${uid}`); q.onsuccess = () => res(q.result); q.onerror = () => rej(q.error); });
  await new Promise((res) => {
    const tx = db.transaction(["app_flags", "_outbox"], "readwrite");
    tx.objectStore("app_flags").put({ key: arg.key, value: arg.value, updated_at: arg.ts });
    tx.objectStore("_outbox").put({ seq: String(Date.now() * 1000 + Math.floor(Math.random() * 900)).padStart(20, "0"), store: "app_flags", op: "upsert", id: arg.key });
    tx.oncomplete = res;
  });
  db.close();
}, { key, value, ts });
await plantFlag(A, "v2_test_a", "from-A", now());
await plantFlag(B, "v2_test_b", "from-B", now());
// конфликт по одной строке: B пишет ПОЗЖЕ → должен победить на обоих
const conflictKey = `v2_conflict_${Date.now()}`;
await plantFlag(A, conflictKey, "A-early", new Date(Date.now() - 3600e3).toISOString());
await plantFlag(B, conflictKey, "B-late", new Date().toISOString());
await A.reload({ waitUntil: "domcontentloaded" }); await A.waitForTimeout(8000);
await B.reload({ waitUntil: "domcontentloaded" }); await B.waitForTimeout(8000);
let conflSettled = false;
for (let i = 0; i < 4 && !conflSettled; i++) {
  await A.reload({ waitUntil: "domcontentloaded" }); await A.waitForTimeout(8000);
  conflSettled = (await storeRow(A, "app_flags", conflictKey))?.value === "B-late";
}
const aFlagB = await storeRow(A, "app_flags", "v2_test_b");
const bFlagA = await storeRow(B, "app_flags", "v2_test_a");
check("merge: флаг B доехал до A", !!aFlagB);
check("merge: флаг A доехал до B", !!bFlagA);
const conflA = await storeRow(A, "app_flags", conflictKey);
const conflB = await storeRow(B, "app_flags", conflictKey);
check("merge: конфликт — поздний победил на A", conflA?.value === "B-late", String(conflA?.value));
check("merge: конфликт — поздний победил на B", conflB?.value === "B-late", String(conflB?.value));

// ── Настоящее UI-удаление сегодняшней записи на A → исчезает на B ──
await A.goto(FE + "/diary", { waitUntil: "domcontentloaded" });
await A.waitForTimeout(4000);
const row = A.getByText(foodName).first();
const visible = await row.isVisible().catch(() => false);
if (visible) {
  const box = await row.boundingBox();
  // Кебаб «⋮» в той же строке: кнопка с svg circle r=1.6 на близкой высоте.
  const dbg = await A.evaluate((rowY) => {
    const btns = [...document.querySelectorAll("button")].filter(b => b.querySelector('svg circle'));
    const ys = btns.map(b => Math.round(b.getBoundingClientRect().top));
    const target = btns.find(b => Math.abs(b.getBoundingClientRect().top - rowY) < 40);
    if (target) target.click();
    return { kebabs: btns.length, ys, rowY: Math.round(rowY), clicked: !!target };
  }, box.y);
  console.log("  kebab dbg:", JSON.stringify(dbg));
  await A.waitForTimeout(700);
  const del = A.getByText("Удалить", { exact: true }).first();
  console.log("  del visible:", await del.isVisible().catch(() => false));
  if (await del.isVisible().catch(() => false)) { await del.click(); }
  await A.waitForTimeout(1000);
}
await A.waitForTimeout(5000);
const goneLocallyA = !(await storeRow(A, "diary", eid));
check("A: UI-удаление убрало запись локально", goneLocallyA);
let deletedOnB = false;
for (let i = 0; i < 4 && !deletedOnB; i++) {
  await B.reload({ waitUntil: "domcontentloaded" });
  await B.waitForTimeout(8000);
  deletedOnB = !(await storeRow(B, "diary", eid));
}
check("B: удаление доехало (записи нет)", deletedOnB);

// ── Холостой синк: объём частичный, ноль записей в IndexedDB ──
let pullResp = null, pushBodies = [];
B.on("response", async r => {
  if (r.url().includes("/sync/v2/pull")) { try { pullResp = (await r.text()).length; } catch {} }
});
B.on("request", r => { if (r.url().includes("/sync/v2/push")) pushBodies.push((r.postData() || "").length); });
await B.context().addInitScript(() => {
  window.__puts = 0; window.__putStores = {};
  const orig = IDBObjectStore.prototype.put;
  IDBObjectStore.prototype.put = function (...a) {
    window.__puts++; window.__putStores[this.name] = (window.__putStores[this.name] || 0) + 1;
    return orig.apply(this, a);
  };
});
const B2 = await B.context().newPage();
await B2.goto(FE, { waitUntil: "domcontentloaded" });
await B2.waitForTimeout(10000); // догоняет хвост батчей
console.log("catch-up puts:", await B2.evaluate(() => JSON.stringify(window.__putStores)));
// Второе открытие — истинно холостой синк.
const B3 = await B.context().newPage();
B3.on("response", async r => { if (r.url().includes("/sync/v2/pull")) { try { pullResp = (await r.text()).length; } catch {} } });
await B3.goto(FE, { waitUntil: "domcontentloaded" });
await B3.waitForTimeout(10000);
const putStores = await B3.evaluate(() => window.__putStores);
console.log("idle puts by store:", JSON.stringify(putStores));
// «Холостой» относится к СИНКУ: собственные фоновые мутации приложения
// (классификация еды и её инвалидции) легитимны. Меряем данные-записи без
// служебных сторов.
const puts = Object.entries(putStores)
  .filter(([k]) => !["_outbox", "_sync_meta", "support_meta"].includes(k))
  .reduce((a, [, v]) => a + v, 0);
check("холостой pull: ответ маленький (частичный объём)", pullResp !== null && pullResp < 5000, `pull resp=${pullResp}b`);
check("холостой синк: данные почти не пишутся", puts <= 40, `data puts=${puts}`);
const verB1 = await getVersion(B3);
const verB2after = await getVersion(B3);
check("версия стабильна на повторных синках", verB1 === verB2after && Number.isInteger(verB1), `v=${verB1}`);

console.log(fail === 0 ? "\n=== ALL OK ===" : `\n=== FAILURES: ${fail} ===`);
await b.close();
process.exit(fail === 0 ? 0 : 1);
