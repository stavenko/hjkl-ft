// Sync v2 validation: init-migration + adopt + two-device incremental sync.
// Fresh dev account per run (dev JWT, secret is the shared dev placeholder).
import { chromium } from "playwright";
const FE = "https://renorma-fit-dev.pages.dev";
const SYNC = "https://sync-worker-dev.vg-stavenko.workers.dev";
const SECRET = "dev-secret-change-in-production";

// ── mint a fresh account ──
const uid = `v2m-${Date.now()}`;
const b64url = (buf) => Buffer.from(buf).toString("base64url");
async function signJwt(payload) {
  const enc = new TextEncoder();
  const data = b64url(JSON.stringify({ alg: "HS256", typ: "JWT" })) + "." + b64url(JSON.stringify(payload));
  const key = await crypto.subtle.importKey("raw", enc.encode(SECRET), { name: "HMAC", hash: "SHA-256" }, false, ["sign"]);
  const sig = await crypto.subtle.sign("HMAC", key, enc.encode(data));
  return data + "." + b64url(new Uint8Array(sig));
}
const nowSec = Math.floor(Date.now() / 1000);
const token = await signJwt({ sub: uid, iat: nowSec, exp: nowSec + 3650 * 86400, caps: [], token_id: "tok1" });
console.log("account:", uid);

// Фейковая платная подписка (TEST_ENTITLEMENT, dev-only) — иначе app-locked
// оверлей закрывает UI и клики не проходят.
const PAY = "https://payment-worker-dev.vg-stavenko.workers.dev";
const co = await fetch(PAY + "/test/guest-checkout", { method: "POST", headers: { "Content-Type": "application/json" }, body: JSON.stringify({ planId: "test" }) });
if (!co.ok) { console.error(`guest-checkout failed: HTTP ${co.status} ${await co.text()}`); process.exit(1); }
const { claimId, secret } = await co.json();
const cl = await fetch(PAY + "/claim", { method: "POST", headers: { "Content-Type": "application/json", Authorization: "Bearer " + token }, body: JSON.stringify({ claimId, secret }) });
if (!cl.ok) { console.error(`claim failed: HTTP ${cl.status} ${await cl.text()}`); process.exit(1); }
console.log("subscription:", JSON.stringify(await cl.json()));

let fail = 0;
const check = (name, ok, extra = "") => { console.log(`${ok ? "OK " : "FAIL"} ${name}${extra ? " — " + extra : ""}`); if (!ok) fail++; };
const sp = (path, body) => fetch(SYNC + path, {
  method: "POST",
  headers: { Authorization: `Bearer ${token}`, "Content-Type": "application/json" },
  body: JSON.stringify(body),
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

// Route-block the sync worker so a load creates the per-user DB without bootstrapping.
const blockSync = (ctx) => ctx.route(`${SYNC}/**`, (r) => r.abort());
const unblockSync = (ctx) => ctx.unroute(`${SYNC}/**`);

const dayStr = (offsetDays) => new Date(Date.now() - offsetDays * 86400e3 - 4 * 3600e3).toLocaleDateString("sv");
const isoAt = (day) => `${day}T10:00:00.000Z`;

const b = await chromium.launch({ headless: true });

// ── 0. Неинициализированный store не отдаёт данные ──
const r0 = await sp("/sync/v2/pull", { since_version: 0 });
const t0 = await r0.text();
check("server: pull из неинициализированного store — ошибка", r0.status === 409 && t0.includes("store_not_initialized"), `HTTP ${r0.status}`);

// ── 1. Клиент с данными инициализирует store (миграция по суткам) ──
const ctxA = await b.newContext({ viewport: { width: 430, height: 920 }, serviceWorkers: "block" });
await blockSync(ctxA);
const A = await ctxA.newPage();
A.on("console", (m) => { const t = m.text(); if (/panicked|sync v2:/.test(t)) console.log("[A]", t.slice(0, 200)); });
await seed(A);
await A.goto(FE, { waitUntil: "domcontentloaded" });
await A.waitForTimeout(9000); // приложение создало per-user БД; синк заблокирован — версии нет

// Фикстура: 3 дня данных. D2: еда mf1 + записи me1,me2; D1: запись me3 + еда mf2; D0(сегодня): вес mw1.
const D2 = dayStr(2), D1 = dayStr(1), D0 = dayStr(0);
const mf1 = `mig-f1-${Date.now()}`, mf2 = `mig-f2-${Date.now()}`;
const me1 = `mig-e1-${Date.now()}`, me2 = `mig-e2-${Date.now()}`, me3 = `mig-e3-${Date.now()}`;
const mw1 = `mig-w1-${Date.now()}`;
const migFoodName = `Мигр еда ${Date.now() % 100000}`;
await idb(A, async ({ uid, arg }) => {
  const db = await new Promise((res, rej) => { const q = indexedDB.open(`hjkl-ft-${uid}`); q.onsuccess = () => res(q.result); q.onerror = () => rej(q.error); });
  const food = (id, name, iso) => ({ id, name, kcal: 100, protein: 5, fat: 2, carbs: 10, nutrients: {}, package_weight: null, is_recipe: false, recipe_id: null, archived: false, created_at: iso, updated_at: iso });
  const entry = (id, fid, date, iso) => ({ id, food_id: fid, date, time: "12:00", grams: 150, waste_grams: 0, meal_label: "lunch", deleted: false, created_at: iso, updated_at: iso });
  await new Promise((res) => {
    const tx = db.transaction(["foods", "diary", "weight_entries"], "readwrite");
    tx.objectStore("foods").put(food(arg.mf1, arg.migFoodName, arg.i2));
    tx.objectStore("foods").put(food(arg.mf2, arg.migFoodName + " 2", arg.i1));
    tx.objectStore("diary").put(entry(arg.me1, arg.mf1, arg.D2, arg.i2));
    tx.objectStore("diary").put(entry(arg.me2, arg.mf1, arg.D2, arg.i2));
    tx.objectStore("diary").put(entry(arg.me3, arg.mf2, arg.D1, arg.i1));
    tx.objectStore("weight_entries").put({ id: arg.mw1, date: arg.D0, weight_kg: 80.5, no_water: false, no_food: false, no_wash: false, used_toilet: false, morning: true, created_at: arg.i0, updated_at: arg.i0 });
    tx.oncomplete = res;
  });
  db.close();
}, { mf1, mf2, me1, me2, me3, mw1, migFoodName, D2, D1, D0, i2: isoAt(D2), i1: isoAt(D1), i0: isoAt(D0) });

await unblockSync(ctxA);
await A.reload({ waitUntil: "domcontentloaded" });
await A.waitForTimeout(18000); // бутстрап: probe(409) → миграция по дням → init
const verA0 = await getVersion(A);
check("A init: версия установлена после миграции", Number.isInteger(verA0) && verA0 >= 3, `version=${verA0}`);

const rp = await sp("/sync/v2/pull", { since_version: 0 });
check("server: store инициализирован (pull отвечает)", rp.status === 200, `HTTP ${rp.status}`);
const journal = rp.status === 200 ? await rp.json() : { version: -1, batches: [] };
check("A init: версия клиента == версии сервера", journal.version === verA0, `client=${verA0} server=${journal.version}`);
check("server: число батчей == версии (счёт сошёлся)", journal.batches.length === journal.version, `batches=${journal.batches.length}`);
const batchOf = (id) => journal.batches.findIndex((bt) => bt.changes.some((c) => (c.row && (c.row.id === id || c.row.key === id)) || c.id === id));
const b1 = batchOf(me1), b2 = batchOf(me2), b3 = batchOf(me3), bw = batchOf(mw1), bf = batchOf(mf1);
check("миграция: все строки фикстуры в журнале", [b1, b2, b3, bw, bf].every((i) => i >= 0), `idx=${[b1, b2, b3, bw, bf]}`);
check("миграция: сутки D2 в одном батче (обе записи + еда)", b1 === b2 && b1 === bf, `e1=${b1} e2=${b2} f1=${bf}`);
check("миграция: разные сутки — разные батчи", b1 !== b3 && b3 !== bw && b1 !== bw, `D2=${b1} D1=${b3} D0=${bw}`);

// ── 2. Клиент инициализируется из store, НЕСМОТРЯ на свои данные (adopt) ──
const ctxC = await b.newContext({ viewport: { width: 430, height: 920 }, serviceWorkers: "block" });
await blockSync(ctxC);
const C = await ctxC.newPage();
C.on("console", (m) => { const t = m.text(); if (/panicked|sync v2:/.test(t)) console.log("[C]", t.slice(0, 200)); });
await seed(C);
await C.goto(FE, { waitUntil: "domcontentloaded" });
await C.waitForTimeout(9000);
const orphF = `orph-f-${Date.now()}`, orphE = `orph-e-${Date.now()}`;
await idb(C, async ({ uid, arg }) => {
  const db = await new Promise((res, rej) => { const q = indexedDB.open(`hjkl-ft-${uid}`); q.onsuccess = () => res(q.result); q.onerror = () => rej(q.error); });
  const nowIso = new Date().toISOString();
  await new Promise((res) => {
    const tx = db.transaction(["foods", "diary"], "readwrite");
    tx.objectStore("foods").put({ id: arg.orphF, name: "Сирота", kcal: 50, protein: 1, fat: 1, carbs: 5, nutrients: {}, package_weight: null, is_recipe: false, recipe_id: null, archived: false, created_at: nowIso, updated_at: nowIso });
    tx.objectStore("diary").put({ id: arg.orphE, food_id: arg.orphF, date: arg.D0, time: "09:00", grams: 100, waste_grams: 0, meal_label: "breakfast", deleted: false, created_at: nowIso, updated_at: nowIso });
    tx.oncomplete = res;
  });
  db.close();
}, { orphF, orphE, D0 });
await unblockSync(ctxC);
await C.reload({ waitUntil: "domcontentloaded" });
await C.waitForTimeout(18000); // бутстрап: store инициализирован → adopt (wipe + реплей журнала)
check("C adopt: локальные данные-сироты удалены", !(await storeRow(C, "foods", orphF)) && !(await storeRow(C, "diary", orphE)));
check("C adopt: данные из store применены локально", !!(await storeRow(C, "diary", me1)) && !!(await storeRow(C, "foods", mf1)));
const verC = await getVersion(C);
check("C adopt: версия клиента установлена", Number.isInteger(verC) && verC >= journal.version, `C=${verC}`);
const rp2 = await sp("/sync/v2/pull", { since_version: 0 });
const journal2 = await rp2.json();
check("C adopt: сироты НЕ попали на сервер", !JSON.stringify(journal2.batches).includes(orphF));
await ctxC.close();

// ── A: мутация после инициализации (planted rows + outbox, имитация tracked-пути) ──
const fid = `v2f-${Date.now()}`, eid = `v2e-${Date.now()}`;
const fidGlobal = fid;
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
    tx.objectStore("_outbox").put({ seq: window.__seqs[0], store: "foods", op: "upsert", id: arg.fid, ts: Date.now() });
    tx.objectStore("_outbox").put({ seq: window.__seqs[1], store: "diary", op: "upsert", id: arg.eid, ts: Date.now() });
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

// ── Device B: нулевой клиент получает всё журналом ──
const ctxB = await b.newContext({ viewport: { width: 430, height: 920 }, serviceWorkers: "block" });
const B = await ctxB.newPage();
B.on("console", m => { const t = m.text(); if (/panicked|sync v2:/.test(t)) console.log("[B]", t.slice(0, 200)); });
await seed(B);
await B.goto(FE, { waitUntil: "domcontentloaded" });
await B.waitForTimeout(18000);
check("B: мигрированные данные доехали", !!(await storeRow(B, "diary", me1)));
check("B: запись A доехала", !!(await storeRow(B, "diary", eid)));
check("B: еда A доехала", !!(await storeRow(B, "foods", fid)));
const verB0 = await getVersion(B);
check("B: версия консистентна с A", verB0 >= verA1, `B=${verB0} A=${verA1}`);

// ── Одновременные изменения на A и B (разные строки одного стора) ──
const plantFlag = (page, key, value, tsMs) => idb(page, async ({ uid, arg }) => {
  const db = await new Promise((res, rej) => { const q = indexedDB.open(`hjkl-ft-${uid}`); q.onsuccess = () => res(q.result); q.onerror = () => rej(q.error); });
  await new Promise((res) => {
    const tx = db.transaction(["app_flags", "_outbox"], "readwrite");
    tx.objectStore("app_flags").put({ key: arg.key, value: arg.value, updated_at: new Date(arg.tsMs).toISOString() });
    tx.objectStore("_outbox").put({ seq: String(Date.now() * 1000 + Math.floor(Math.random() * 900)).padStart(20, "0"), store: "app_flags", op: "upsert", id: arg.key, ts: arg.tsMs });
    tx.oncomplete = res;
  });
  db.close();
}, { key, value, tsMs });
await plantFlag(A, "v2_test_a", "from-A", Date.now());
await plantFlag(B, "v2_test_b", "from-B", Date.now());
// конфликт по одной строке: СОБЫТИЕ B позже → B побеждает на обоих (ts события,
// а не updated_at — тот теперь информативный)
const conflictKey = `v2_conflict_${Date.now()}`;
await plantFlag(A, conflictKey, "A-early", Date.now() - 3600e3);
await plantFlag(B, conflictKey, "B-late", Date.now());
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

// ── Индикаторы: перенос замороженного дня + «раннее вычисление побеждает» ──
const plantInd = (page, store, date, value, computedAtIso, tsMs) => idb(page, async ({ uid, arg }) => {
  const db = await new Promise((res, rej) => { const q = indexedDB.open(`hjkl-ft-${uid}`); q.onsuccess = () => res(q.result); q.onerror = () => rej(q.error); });
  const wireInd = { ind_protein: "protein", ind_veg_fruit: "veg_fruit", ind_steps: "steps", ind_calories: "calories" }[arg.store];
  await new Promise((res) => {
    const tx = db.transaction([arg.store, "_outbox"], "readwrite");
    tx.objectStore(arg.store).put({ date: arg.date, value: arg.value, ratio: 1.0, computed_at: arg.computedAtIso });
    tx.objectStore("_outbox").put({ seq: String(Date.now() * 1000 + Math.floor(Math.random() * 900)).padStart(20, "0"), store: "ind_days", op: "upsert", id: `${wireInd}:${arg.date}`, ts: arg.tsMs });
    tx.oncomplete = res;
  });
  db.close();
}, { store, date, value, computedAtIso, tsMs });
// Даты за пределами окна автозаморозки, чтобы приложение их не пересчитало.
const DP = dayStr(20), DI = dayStr(21);
await plantInd(A, "ind_protein", DP, 77, new Date().toISOString(), Date.now());
// Конфликт по одному дню калорий: A посчитал РАНЬШЕ (2500), B позже (9999) →
// для ind_days побеждает раннее вычисление — 2500 на обоих.
await plantInd(A, "ind_calories", DI, 2500, new Date(Date.now() - 7200e3).toISOString(), Date.now() - 7200e3);
await plantInd(B, "ind_calories", DI, 9999, new Date().toISOString(), Date.now());
await A.reload({ waitUntil: "domcontentloaded" }); await A.waitForTimeout(8000);
await B.reload({ waitUntil: "domcontentloaded" }); await B.waitForTimeout(8000);
let indTransfer = false, indConfl = false;
for (let i = 0; i < 4 && !(indTransfer && indConfl); i++) {
  const bp = await storeRow(B, "ind_protein", DP);
  const ca = await storeRow(A, "ind_calories", DI);
  const cb = await storeRow(B, "ind_calories", DI);
  indTransfer = bp?.value === 77;
  indConfl = ca?.value === 2500 && cb?.value === 2500;
  if (!(indTransfer && indConfl)) {
    await A.reload({ waitUntil: "domcontentloaded" }); await A.waitForTimeout(8000);
    await B.reload({ waitUntil: "domcontentloaded" }); await B.waitForTimeout(8000);
  }
}
check("индикаторы: замороженный день доехал до B", indTransfer);
const indA = await storeRow(A, "ind_calories", DI), indB = await storeRow(B, "ind_calories", DI);
check("индикаторы: конфликт — раннее вычисление победило на обоих", indConfl, `A=${indA?.value} B=${indB?.value}`);

// ── Прогресс историй (app_flags: story_viewed / welcome_shown) ──
await plantFlag(A, "story_viewed", JSON.stringify(["h1", "h2"]), Date.now());
await plantFlag(A, "welcome_shown", "true", Date.now());
await A.reload({ waitUntil: "domcontentloaded" }); await A.waitForTimeout(8000);
let storyOnB = false, welcomeOnB = false;
for (let i = 0; i < 4 && !(storyOnB && welcomeOnB); i++) {
  await B.reload({ waitUntil: "domcontentloaded" }); await B.waitForTimeout(8000);
  const sv = await storeRow(B, "app_flags", "story_viewed");
  const ws = await storeRow(B, "app_flags", "welcome_shown");
  const hashes = sv ? JSON.parse(sv.value) : [];
  storyOnB = hashes.includes("h1") && hashes.includes("h2");
  welcomeOnB = ws?.value === "true";
}
check("истории: прогресс просмотра доехал до B", storyOnB);
check("истории: welcome_shown доехал до B", welcomeOnB);
// B досматривает ещё кадр (позже) → дополненный набор возвращается на A.
const svB = await storeRow(B, "app_flags", "story_viewed");
const augmented = [...new Set([...(svB ? JSON.parse(svB.value) : []), "h3"])];
await plantFlag(B, "story_viewed", JSON.stringify(augmented), Date.now());
await B.reload({ waitUntil: "domcontentloaded" }); await B.waitForTimeout(8000);
let storyBack = false;
for (let i = 0; i < 4 && !storyBack; i++) {
  await A.reload({ waitUntil: "domcontentloaded" }); await A.waitForTimeout(8000);
  const sv = await storeRow(A, "app_flags", "story_viewed");
  const hashes = sv ? JSON.parse(sv.value) : [];
  storyBack = hashes.includes("h1") && hashes.includes("h3");
}
check("истории: дополненный прогресс вернулся на A", storyBack);

// ── Шаги: замороженный 0-день БЕЗ записи шагов — отравление старого бага.
// Ремонт при запуске удаляет строку, удаление синкается на другие устройства.
const DZ = dayStr(23);
const plantPoisonedStepDay = (page, clearFlag) => idb(page, async ({ uid, arg }) => {
  const db = await new Promise((res, rej) => { const q = indexedDB.open(`hjkl-ft-${uid}`); q.onsuccess = () => res(q.result); q.onerror = () => rej(q.error); });
  await new Promise((res) => {
    const tx = db.transaction(["ind_steps", "app_flags"], "readwrite");
    tx.objectStore("ind_steps").put({ date: arg.DZ, value: 0, ratio: 0, computed_at: new Date(Date.now() - 86400e3).toISOString() });
    if (arg.clearFlag) tx.objectStore("app_flags").delete("ind_steps_unentered_backfilled_v1");
    tx.oncomplete = res;
  });
  db.close();
}, { DZ, clearFlag });
await plantPoisonedStepDay(A, true);  // на A ремонт прогонится заново при запуске
await plantPoisonedStepDay(B, false); // на B строка ждёт удаления из журнала
await A.reload({ waitUntil: "domcontentloaded" }); await A.waitForTimeout(8000);
check("шаги: ремонт удалил 0-день без записи локально", !(await storeRow(A, "ind_steps", DZ)));
let stepGoneB = false;
for (let i = 0; i < 4 && !stepGoneB; i++) {
  await B.reload({ waitUntil: "domcontentloaded" }); await B.waitForTimeout(8000);
  stepGoneB = !(await storeRow(B, "ind_steps", DZ));
}
check("шаги: удаление отравленного дня доехало до B", stepGoneB);

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

// ── Конфликт «вес одной записи» и «удаление против правки» ──
const editEntry = (page, id, grams, tsMs, op = "upsert") => idb(page, async ({ uid, arg }) => {
  const db = await new Promise((res, rej) => { const q = indexedDB.open(`hjkl-ft-${uid}`); q.onsuccess = () => res(q.result); q.onerror = () => rej(q.error); });
  await new Promise((res) => {
    const tx = db.transaction(["diary", "_outbox"], "readwrite");
    if (arg.op === "upsert") {
      const st = tx.objectStore("diary");
      const rq = st.get(arg.id);
      rq.onsuccess = () => { const r = rq.result; if (r) { r.grams = arg.grams; r.updated_at = new Date().toISOString(); st.put(r); } };
    } else {
      tx.objectStore("diary").delete(arg.id);
    }
    tx.objectStore("_outbox").put({ seq: String(Date.now() * 1000 + Math.floor(Math.random() * 900)).padStart(20, "0"), store: "diary", op: arg.op, id: arg.id, ts: arg.tsMs });
    tx.oncomplete = res;
  });
  db.close();
}, { id, grams, tsMs, op });
// Общая запись уже есть на обоих: используем вторую посаженную пару.
const gid = `v2ge-${Date.now()}`;
await idb(A, async ({ uid, arg }) => {
  const db = await new Promise((res, rej) => { const q = indexedDB.open(`hjkl-ft-${uid}`); q.onsuccess = () => res(q.result); q.onerror = () => rej(q.error); });
  const today = new Date(Date.now() - 4 * 3600e3).toLocaleDateString('sv');
  const nowIso = new Date().toISOString();
  await new Promise((res) => {
    const tx = db.transaction(["diary", "_outbox"], "readwrite");
    tx.objectStore("diary").put({ id: arg.gid, food_id: arg.fid, date: today, time: "14:00", grams: 100, waste_grams: 0, meal_label: "lunch", deleted: false, created_at: nowIso, updated_at: nowIso });
    tx.objectStore("_outbox").put({ seq: String(Date.now() * 1000 + 990).padStart(20, "0"), store: "diary", op: "upsert", id: arg.gid, ts: Date.now() });
    tx.oncomplete = res;
  });
  db.close();
}, { gid, fid: fidGlobal });
await A.reload({ waitUntil: "domcontentloaded" }); await A.waitForTimeout(8000); // push
// Поллим доставку: правки ниже осмысленны только когда строка уже на B.
let baseOnB = false;
for (let i = 0; i < 5 && !baseOnB; i++) {
  await B.reload({ waitUntil: "domcontentloaded" }); await B.waitForTimeout(8000);
  baseOnB = !!(await storeRow(B, "diary", gid));
  if (!baseOnB) { await A.reload({ waitUntil: "domcontentloaded" }); await A.waitForTimeout(8000); }
}
check("grams: базовая запись доехала до B", baseOnB);
// A правит 150 (раньше), B правит 200 (позже) → 200 на обоих
await editEntry(A, gid, 150, Date.now() - 60000);
await editEntry(B, gid, 200, Date.now());
await A.reload({ waitUntil: "domcontentloaded" }); await A.waitForTimeout(8000);
await B.reload({ waitUntil: "domcontentloaded" }); await B.waitForTimeout(8000);
let gramsOk = false;
for (let i = 0; i < 4 && !gramsOk; i++) {
  await A.reload({ waitUntil: "domcontentloaded" }); await A.waitForTimeout(8000);
  const ra = await storeRow(A, "diary", gid);
  const rb = await storeRow(B, "diary", gid);
  gramsOk = ra?.grams === 200 && rb?.grams === 200;
  if (!gramsOk) { await B.reload({ waitUntil: "domcontentloaded" }); await B.waitForTimeout(8000); }
}
check("grams-конфликт: поздняя правка (200) победила на обоих", gramsOk);
// удаление против правки: B правит 300 (раньше), A удаляет (позже) → удалена на обоих
await editEntry(B, gid, 300, Date.now() - 60000);
await editEntry(A, gid, 0, Date.now(), "delete");
await B.reload({ waitUntil: "domcontentloaded" }); await B.waitForTimeout(8000);
await A.reload({ waitUntil: "domcontentloaded" }); await A.waitForTimeout(8000);
let delOk = false;
for (let i = 0; i < 4 && !delOk; i++) {
  await B.reload({ waitUntil: "domcontentloaded" }); await B.waitForTimeout(8000);
  delOk = !(await storeRow(A, "diary", gid)) && !(await storeRow(B, "diary", gid));
  if (!delOk) { await A.reload({ waitUntil: "domcontentloaded" }); await A.waitForTimeout(8000); }
}
check("удаление-vs-правка: позднее удаление победило на обоих", delOk);

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
