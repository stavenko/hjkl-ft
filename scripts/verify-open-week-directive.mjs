// Директива ОТКРЫТИЯ ТЕМЫ через чат, целиком по живому пути: админ пишет
// /open-week 7 в переписке → клиент сам применяет её (сервер данные пользователя
// не трогает) → тема красного мяса открыта, седьмая история появилась в ленте.
//
// Проверяется именно то, ради чего директива и заведена: гейт открыть тему НЕ
// может (у человека нет закрытых недель жиров), а куратор — может.
import { chromium } from "playwright";

const FE = process.argv[2] || "https://renorma-fit-dev.pages.dev";
const ADMIN = process.argv[3] || "https://renorma-admin-dev.pages.dev";
const SUP = "https://support-worker-dev.vg-stavenko.workers.dev";
const SYNC = "https://sync-worker-dev.vg-stavenko.workers.dev";
const PAY = "https://payment-worker-dev.vg-stavenko.workers.dev";
const SECRET = "dev-secret-change-in-production";
const APPROVE = "dev-admin-approve-secret";

const b64url = (b) => Buffer.from(b).toString("base64").replace(/=/g, "").replace(/\+/g, "-").replace(/\//g, "_");
async function jwt(sub) {
  const enc = new TextEncoder();
  const now = Math.floor(Date.now() / 1000);
  const data = `${b64url(JSON.stringify({ alg: "HS256", typ: "JWT" }))}.${b64url(JSON.stringify({ sub, iat: now, exp: now + 3600, caps: [], token_id: "t" }))}`;
  const key = await crypto.subtle.importKey("raw", enc.encode(SECRET), { name: "HMAC", hash: "SHA-256" }, false, ["sign"]);
  const sig = await crypto.subtle.sign("HMAC", key, enc.encode(data));
  return `${data}.${b64url(new Uint8Array(sig))}`;
}

const ts = Date.now();
const expertSub = "e2e-expert-" + ts;
const clientSub = "e2e-client-" + ts;
const expertTok = await jwt(expertSub);
const clientTok = await jwt(clientSub);

// approve expert
let r = await fetch(`${SUP}/admin/request`, { method: "POST", headers: { Authorization: `Bearer ${expertTok}`, "Content-Type": "application/json" }, body: "{}" });
const { code } = await r.json();
await fetch(`${SUP}/admin/approve`, { method: "POST", headers: { "X-Admin-Secret": APPROVE, "Content-Type": "application/json" }, body: JSON.stringify({ code }) });
// activate client's fake sub
const co = await fetch(`${PAY}/test/guest-checkout`, { method: "POST", headers: { "Content-Type": "application/json" }, body: JSON.stringify({ planId: "test" }) });
const { claimId, secret } = await co.json();
await fetch(`${PAY}/claim`, { method: "POST", headers: { "Content-Type": "application/json", Authorization: `Bearer ${clientTok}` }, body: JSON.stringify({ claimId, secret }) });
console.log("setup: expert approved + client sub active");

const readFlags = (page, uid) => page.evaluate(async (uid) => {
  const db = await new Promise((res, rej) => { const q = indexedDB.open(`hjkl-ft-${uid}`); q.onsuccess = () => res(q.result); q.onerror = () => rej(q.error); });
  const rows = await new Promise((res, rej) => { const tx = db.transaction(["app_flags"], "readonly"); const rq = tx.objectStore("app_flags").getAll(); rq.onsuccess = () => res(rq.result); rq.onerror = () => rej(rq.error); });
  db.close();
  const f = Object.fromEntries((rows || []).map((r) => [r.key, r.value]));
  return { unlocked: f.red_meat_week_unlocked ?? null, opened: f.red_meat_week_opened_at ?? null };
}, uid);

const b = await chromium.launch({ headless: true });

// ── Client: seed with planka 2000, open the Live chat ─────────────────────────
const cctx = await b.newContext({ viewport: { width: 430, height: 900 }, serviceWorkers: "block" });
const cpage = await cctx.newPage();
await cpage.goto(FE, { waitUntil: "domcontentloaded" });
await cpage.evaluate(async ({ uid, token }) => {
  const del = (n) => new Promise((r) => { const q = indexedDB.deleteDatabase(n); q.onsuccess = q.onerror = q.onblocked = () => r(); });
  await del("hjkl-ft"); await del(`hjkl-ft-${uid}`);
  localStorage.clear();
  localStorage.setItem("user_id", uid); localStorage.setItem("auth_token", token);
  localStorage.setItem("token_id", "t"); localStorage.setItem("auth_ctx", "browser");
  localStorage.setItem("pwa_dismissed", "true"); localStorage.setItem("profile_sex", "male");
}, { uid: clientSub, token: clientTok });
await cpage.goto(FE, { waitUntil: "domcontentloaded" });
for (let i = 0; i < 40; i++) {
  const ready = await cpage.evaluate(async (uid) => {
    const dbs = await indexedDB.databases();
    if (!dbs.some((d) => d.name === `hjkl-ft-${uid}`)) return false;
    return await new Promise((res) => { const q = indexedDB.open(`hjkl-ft-${uid}`); q.onsuccess = () => { const ok = q.result.objectStoreNames.contains("goals"); q.result.close(); res(ok); }; q.onerror = () => res(false); });
  }, clientSub).catch(() => false);
  if (ready) break;
  await cpage.waitForTimeout(500);
}
await cpage.evaluate(async (uid) => {
  const open = (n) => new Promise((res, rej) => { const q = indexedDB.open(n); q.onsuccess = () => res(q.result); q.onerror = () => rej(q.error); });
  const db = await open(`hjkl-ft-${uid}`);
  const now = new Date(), nowIso = now.toISOString(), end = now.getTime() + 30 * 864e5;
  const ymd = (o) => { const d = new Date(); d.setDate(d.getDate() - o); return `${d.getFullYear()}-${String(d.getMonth() + 1).padStart(2, "0")}-${String(d.getDate()).padStart(2, "0")}`; };
  const rec = {
    app_flags: [
      { key: "push_onboarding_dismissed", value: "true" },
      { key: "paywall_skipped_date", value: ymd(0) },
      { key: "ft_subscription", value: JSON.stringify({ plan: "monthly", end, active: true, start: now.getTime(), status: "paid", no_renew: false, provider: "lava" }) },
    ],
    profile: [{ key: "profile", sex: "male", height_cm: 180, birth_year: 1990, goal: "lose", cycle_start: null, steps_planka: 8000, updated_at: nowIso }],
    goals: [{ id: "cal", nutrient: "Calories", key: "calories", direction: "AtMost", amount: 2000, unit: "Kcal", period: "Day", created_at: nowIso, updated_at: nowIso }],
    weight_entries: [{ id: "w0", date: ymd(0), weight_kg: 80, no_water: false, no_food: false, no_wash: false, used_toilet: false, morning: true, created_at: nowIso, updated_at: nowIso }],
  };
  const avail = Array.from(db.objectStoreNames);
  for (const [store, rows] of Object.entries(rec)) {
    if (!avail.includes(store)) continue;
    await new Promise((res, rej) => { const tx = db.transaction([store], "readwrite"); const os = tx.objectStore(store); for (const row of rows) os.put(row); tx.oncomplete = () => res(); tx.onerror = () => rej(tx.error); });
  }
  db.close();
}, clientSub);
await cpage.goto(`${FE}/chat`, { waitUntil: "domcontentloaded" });
await cpage.waitForSelector("#splash", { state: "detached", timeout: 15000 }).catch(() => {});
await cpage.waitForTimeout(1500);
console.log("тема до директивы   :", JSON.stringify(await readFlags(cpage, clientSub)));

// Client opens a conversation (so the thread appears in the admin queue).
r = await fetch(`${SUP}/message`, { method: "POST", headers: { Authorization: `Bearer ${clientTok}`, "Content-Type": "application/json" }, body: JSON.stringify({ client_id: "hi-" + ts, text: "Здравствуйте" }) });
console.log("client opened thread:", r.status);

// ── Admin: /set-calorie-limit 2600 in the client's thread ─────────────────────
const actx = await b.newContext({ viewport: { width: 430, height: 900 } });
const apage = await actx.newPage();
await apage.goto(ADMIN, { waitUntil: "domcontentloaded" });
await apage.evaluate(({ u, t }) => { localStorage.setItem("user_id", u); localStorage.setItem("auth_token", t); }, { u: expertSub, t: expertTok });
await apage.goto(ADMIN, { waitUntil: "domcontentloaded" });
// Open the client's thread and issue the command through the REAL admin UI.
const conv = apage.getByTestId("conv").filter({ hasText: clientSub });
await conv.first().waitFor({ state: "visible", timeout: 20000 });
await conv.first().click();
const input = apage.getByTestId("reply-input");
await input.waitFor({ state: "visible", timeout: 10000 });
await input.fill("/open-week 7");
apage.on("requestfailed", (rq) => console.log("  [admin reqfail]", rq.url(), rq.failure()?.errorText));
await apage.getByTestId("reply-send").click();
await apage.waitForTimeout(3000);
const banners = await apage.$$eval(".banner", (els) => els.map((e) => e.textContent.trim()));
console.log("admin banners       :", JSON.stringify(banners));

// ── Client applies it (poll picks it up) ─────────────────────────────────────
let applied = null;
for (let i = 0; i < 60; i++) {
  applied = await readFlags(cpage, clientSub);
  if (applied.unlocked === "true") break;
  await cpage.waitForTimeout(500);
}
console.log("тема после директивы:", JSON.stringify(applied));
await cpage.screenshot({ path: "/tmp/open-week-chat.png", fullPage: true });

// The app also drops an inbox LETTER telling the user the curator changed it.
const letter = await cpage.evaluate(async (uid) => {
  const db = await new Promise((res, rej) => { const q = indexedDB.open(`hjkl-ft-${uid}`); q.onsuccess = () => res(q.result); q.onerror = () => rej(q.error); });
  const row = await new Promise((res, rej) => { const tx = db.transaction(["app_flags"], "readonly"); const rq = tx.objectStore("app_flags").get("letters_v1"); rq.onsuccess = () => res(rq.result); rq.onerror = () => rej(rq.error); });
  db.close();
  try { return (JSON.parse(row?.value || "[]")).map((l) => l.body).find((b) => b.includes("куратор")); } catch { return null; }
}, clientSub);
console.log("inbox letter        :", letter ? "«" + letter.split("\n")[0] + "»" : null);

// ── История появилась в ленте, и флаг доехал до сервера ─────────────────────
await cpage.goto(FE, { waitUntil: "domcontentloaded" });
await cpage.waitForTimeout(6000);
const badges = await cpage.locator('[data-testid="story-circle"]').allInnerTexts().catch(() => []);
console.log("истории в ленте     :", badges.map((t) => t.trim()).join("|"));

r = await fetch(`${SYNC}/sync/v2/pull`, {
  method: "POST",
  headers: { Authorization: `Bearer ${clientTok}`, "Content-Type": "application/json" },
  body: JSON.stringify({ since_version: 0 }),
});
let synced = null;
if (r.ok) {
  const journal = await r.json();
  for (const batch of journal.batches || []) {
    for (const ch of batch.changes || []) {
      if (ch.store === "app_flags" && ch.op === "upsert" && ch.row?.key === "red_meat_week_unlocked") {
        synced = ch.row.value;
      }
    }
  }
}
console.log("флаг в журнале синка:", synced);

const ok = applied?.unlocked === "true" && !!applied?.opened
  && badges.some((t) => t.trim() === "7");
console.log(ok ? "\nвсё сошлось" : "\nне сошлось");
await b.close();
process.exit(ok ? 0 : 1);
