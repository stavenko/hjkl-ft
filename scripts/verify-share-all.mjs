// Full E2E: expert requests the food diary; the REAL client app builds the share
// (curator_share, now incl. indicators + calorie-planka adherence); verify the
// payload, then render it in the admin modal.
import { chromium } from "playwright";

const FE = process.argv[2] || "https://renorma-fit-dev.pages.dev";
const ADMIN = process.argv[3] || "https://renorma-admin-dev.pages.dev";
const SUP = "https://support-worker-dev.vg-stavenko.workers.dev";
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

// approve the expert
let r = await fetch(`${SUP}/admin/request`, { method: "POST", headers: { Authorization: `Bearer ${expertTok}`, "Content-Type": "application/json" }, body: "{}" });
const { code } = await r.json();
await fetch(`${SUP}/admin/approve`, { method: "POST", headers: { "X-Admin-Secret": APPROVE, "Content-Type": "application/json" }, body: JSON.stringify({ code }) });
console.log("expert approved");

// Activate a fake paid subscription for the client (dev TEST_ENTITLEMENT), else
// the app shows the "no subscription" lock overlay and blocks the chat.
const PAY = "https://payment-worker-dev.vg-stavenko.workers.dev";
const co = await fetch(`${PAY}/test/guest-checkout`, { method: "POST", headers: { "Content-Type": "application/json" }, body: JSON.stringify({ planId: "test" }) });
const { claimId, secret } = await co.json();
r = await fetch(`${PAY}/claim`, { method: "POST", headers: { "Content-Type": "application/json", Authorization: `Bearer ${clientTok}` }, body: JSON.stringify({ claimId, secret }) });
console.log("client sub activated:", r.status);

// The client seed (runs in the browser): profile + sub + planka goal + weight +
// three logged days (two within the 2600 planka, one over → calories indicator orange).
function seedFn({ uid }) {
  return new Promise(async (resolve, reject) => {
    try {
      const open = (n) => new Promise((res, rej) => { const q = indexedDB.open(n); q.onsuccess = () => res(q.result); q.onerror = () => rej(q.error); });
      const db = await open(`hjkl-ft-${uid}`);
      const now = new Date(), nowIso = now.toISOString();
      const ymd = (o) => { const d = new Date(); d.setDate(d.getDate() - o); return `${d.getFullYear()}-${String(d.getMonth() + 1).padStart(2, "0")}-${String(d.getDate()).padStart(2, "0")}`; };
      const end = now.getTime() + 30 * 24 * 60 * 60 * 1000;
      const day = [
        { d: 0, id: "f0", kcal: 2400, name: "День 0" },
        { d: 1, id: "f1", kcal: 2300, name: "День 1" },
        { d: 2, id: "f2", kcal: 2900, name: "День 2" },
      ];
      const rec = {
        app_flags: [
          { key: "push_onboarding_dismissed", value: "true" },
          { key: "paywall_skipped_date", value: ymd(0) },
          { key: "ft_subscription", value: JSON.stringify({ plan: "monthly", end, active: true, start: now.getTime(), status: "paid", no_renew: false, provider: "lava" }) },
        ],
        profile: [{ key: "profile", sex: "male", height_cm: 180, birth_year: 1990, goal: "lose", cycle_start: null, steps_planka: 8000, updated_at: nowIso }],
        goals: [{ id: "cal", nutrient: "Calories", key: "calories", direction: "AtMost", amount: 2600, unit: "Kcal", period: "Day", created_at: nowIso, updated_at: nowIso }],
        weight_entries: Array.from({ length: 9 }, (_, i) => ({ id: "w" + i, date: ymd(i), weight_kg: 80 - i * 0.1, no_water: false, no_food: false, no_wash: false, used_toilet: false, morning: true, created_at: nowIso, updated_at: nowIso })),
        foods: day.map((x) => ({ id: x.id, name: x.name, kcal: x.kcal, protein: 120, fat: 70, carbs: 250, nutrients: {}, package_weight: null, is_recipe: false, recipe_id: null, archived: false, is_restaurant: false, is_snack: false, created_at: nowIso, updated_at: nowIso })),
        diary: day.map((x) => ({ id: "d" + x.d, food_id: x.id, date: ymd(x.d), time: null, grams: 100, waste_grams: 0, meal_label: "lunch", deleted: false, created_at: nowIso, updated_at: nowIso })),
      };
      const avail = Array.from(db.objectStoreNames);
      for (const [store, rows] of Object.entries(rec)) {
        if (!rows.length || !avail.includes(store)) continue;
        await new Promise((res, rej) => { const tx = db.transaction([store], "readwrite"); const os = tx.objectStore(store); for (const row of rows) os.put(row); tx.oncomplete = () => res(); tx.onerror = () => rej(tx.error); });
      }
      db.close();
      resolve(true);
    } catch (e) { reject(String(e)); }
  });
}

const b = await chromium.launch({ headless: true });

// ── Client: seed + open the Live chat ─────────────────────────────────────────
const cctx = await b.newContext({ viewport: { width: 430, height: 900 }, serviceWorkers: "block" });
const cpage = await cctx.newPage();
await cpage.goto(FE, { waitUntil: "domcontentloaded" });
await cpage.evaluate(async ({ uid, token }) => {
  const del = (n) => new Promise((r) => { const q = indexedDB.deleteDatabase(n); q.onsuccess = q.onerror = q.onblocked = () => r(); });
  await del("hjkl-ft"); await del(`hjkl-ft-${uid}`);
  localStorage.clear();
  localStorage.setItem("user_id", uid);
  localStorage.setItem("auth_token", token);
  localStorage.setItem("token_id", "t");
  localStorage.setItem("auth_ctx", "browser");
  localStorage.setItem("pwa_dismissed", "true");
  localStorage.setItem("profile_sex", "male");
}, { uid: clientSub, token: clientTok });
await cpage.goto(FE, { waitUntil: "domcontentloaded" });
// wait for the per-user DB to exist, then seed
for (let i = 0; i < 40; i++) {
  const ready = await cpage.evaluate(async (uid) => {
    const dbs = await indexedDB.databases();
    if (!dbs.some((d) => d.name === `hjkl-ft-${uid}`)) return false;
    return await new Promise((res) => { const q = indexedDB.open(`hjkl-ft-${uid}`); q.onsuccess = () => { const ok = q.result.objectStoreNames.contains("app_flags"); q.result.close(); res(ok); }; q.onerror = () => res(false); });
  }, clientSub).catch(() => false);
  if (ready) break;
  await cpage.waitForTimeout(500);
}
await cpage.evaluate(seedFn, { uid: clientSub });
await cpage.goto(`${FE}/chat`, { waitUntil: "domcontentloaded" });
await cpage.waitForSelector("#splash", { state: "detached", timeout: 15000 }).catch(() => {});
await cpage.waitForTimeout(1500);

// ── Expert posts the food-diary data_request ──────────────────────────────────
r = await fetch(`${SUP}/conversations/${clientSub}/reply`, {
  method: "POST",
  headers: { Authorization: `Bearer ${expertTok}`, "Content-Type": "application/json" },
  body: JSON.stringify({ client_id: "req-" + ts, text: "Куратор запрашивает дневник питания", kind: "data_request", payload: JSON.stringify({ dataset: "all" }) }),
});
console.log("expert data_request:", r.status);

// ── Client shares ─────────────────────────────────────────────────────────────
await cpage.getByTestId("live-request-share").first().waitFor({ state: "visible", timeout: 20000 });
await cpage.getByTestId("live-request-share").first().click();
await cpage.getByTestId("live-request-done").first().waitFor({ state: "visible", timeout: 20000 });
console.log("client shared ✓");

// ── Inspect the shared payload (as the expert) ───────────────────────────────
r = await fetch(`${SUP}/conversations/${clientSub}/messages?after_seq=0&limit=50`, { headers: { Authorization: `Bearer ${expertTok}` } });
const page = await r.json();
const share = (page.messages || []).find((m) => m.kind === "data_share");
const payload = share ? JSON.parse(share.payload) : null;
console.log("data_share top-level keys:", payload ? Object.keys(payload).join(",") : null);
console.log("payload bytes:", share ? share.payload.length : 0);

// ── Admin side: does the data_share render (buttons) or fail (error bubble)? ──
const actx = await b.newContext({ viewport: { width: 430, height: 900 } });
const apage = await actx.newPage();
await apage.goto(ADMIN, { waitUntil: "domcontentloaded" });
await apage.evaluate(({ u, t }) => { localStorage.setItem("user_id", u); localStorage.setItem("auth_token", t); }, { u: expertSub, t: expertTok });
await apage.goto(ADMIN, { waitUntil: "domcontentloaded" });
const conv = apage.getByTestId("conv").filter({ hasText: clientSub });
await conv.first().waitFor({ state: "visible", timeout: 20000 });
await conv.first().click();
await apage.waitForTimeout(2500);
const btnCount = await apage.getByTestId("data-share-btn").count();
const errText = await apage.locator("text=Не удалось прочитать данные").first().textContent().catch(() => null);
console.log("admin data-share buttons:", btnCount, "| parse error:", errText || "none");
await apage.screenshot({ path: "share-all-admin.png", fullPage: true });

// Open the FOOD dataset button and verify its modal has content.
let modalOk = false;
if (btnCount > 0) {
  const foodBtn = apage.getByTestId("data-share-btn").filter({ hasText: "питания" });
  if (await foodBtn.count()) {
    await foodBtn.first().click();
    await apage.getByTestId("data-share-modal").waitFor({ state: "visible", timeout: 10000 }).catch(() => {});
    await apage.waitForTimeout(500);
    const modalTxt = await apage.getByTestId("data-share-modal").textContent().catch(() => "");
    modalOk = !!modalTxt && modalTxt.includes("Планка по калориям");
    await apage.screenshot({ path: "share-all-modal.png", fullPage: true });
  }
}

await cctx.close(); await actx.close(); await b.close();

// Датасетов теперь ПЯТЬ: тело, вес, шаги, еда и данные об устройстве (system).
const ok = btnCount === 5 && !errText && modalOk;
console.log(ok ? "\n✅ PASS — /request-all renders (5 datasets, food modal has planka)" : "\n❌ FAIL — bundle share broken");
process.exit(ok ? 0 : 1);
process.exit(ok ? 0 : 1);
