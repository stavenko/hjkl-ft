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
      // Parity scenario: veg/fruit + calcium ON TARGET for the 7 COMPLETED days,
      // but TODAY is incomplete (only a little eaten so far). The widget (7
      // completed days, today excluded) shows GREEN; the OLD share (compute(),
      // window includes today) wrongly showed ORANGE. The fix must show GREEN.
      const N = 8; // today (i=0) + 7 completed days (i=1..7)
      const rec = {
        app_flags: [
          { key: "push_onboarding_dismissed", value: "true" },
          { key: "paywall_skipped_date", value: ymd(0) },
          { key: "ft_subscription", value: JSON.stringify({ plan: "monthly", end, active: true, start: now.getTime(), status: "paid", no_renew: false, provider: "lava" }) },
          { key: "activity_week_unlocked", value: "true" },
          { key: "calcium_week_unlocked", value: "true" },
        ],
        profile: [{ key: "profile", sex: "male", height_cm: 180, birth_year: 1990, goal: "lose", cycle_start: null, steps_planka: 8000, updated_at: nowIso }],
        goals: [
          { id: "cal", nutrient: "Calories", key: "calories", direction: "AtMost", amount: 2600, unit: "Kcal", period: "Day", created_at: nowIso, updated_at: nowIso },
          { id: "ca", nutrient: "Кальций", key: "calcium", direction: "AtLeast", amount: 1000, unit: "Mg", period: "Day", created_at: nowIso, updated_at: nowIso },
        ],
        weight_entries: Array.from({ length: 9 }, (_, i) => ({ id: "w" + i, date: ymd(i), weight_kg: 80 - i * 0.1, no_water: false, no_food: false, no_wash: false, used_toilet: false, morning: true, created_at: nowIso, updated_at: nowIso })),
        // One veg/fruit food carrying calcium (130 mg / 100 g). Completed days eat
        // 850 g → veg 850 ≥ 800 target AND calcium 1105 ≥ 1000 → both green. Today
        // eats only 100 g → under both, but today must NOT count.
        foods: [{ id: "vf", name: "Овощи с кальцием", kcal: 30, protein: 2, fat: 0, carbs: 5, nutrients: { "Кальций": 130 }, package_weight: null, is_recipe: false, recipe_id: null, archived: false, is_restaurant: false, is_snack: false, is_veg_fruit: true, created_at: nowIso, updated_at: nowIso }],
        diary: Array.from({ length: N }, (_, i) => ({ id: "d" + i, food_id: "vf", date: ymd(i), time: null, grams: i === 0 ? 100 : 850, waste_grams: 0, meal_label: "lunch", deleted: false, created_at: nowIso, updated_at: nowIso })),
        step_entries: Array.from({ length: N }, (_, i) => ({ id: "s" + i, date: ymd(i), steps: 9000, created_at: nowIso, updated_at: nowIso })),
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
  body: JSON.stringify({ client_id: "req-" + ts, text: "Куратор запрашивает дневник питания", kind: "data_request", payload: JSON.stringify({ dataset: "food" }) }),
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
const inds = payload?.food?.indicators || [];
const veg = inds.find((i) => i.label === "Овощи и фрукты");
const calcium = inds.find((i) => i.label === "Кальций");
console.log("indicators in share:", inds.map((i) => `${i.label}:${i.state}`).join(", "));

// ── Render it in the admin modal ─────────────────────────────────────────────
const actx = await b.newContext({ viewport: { width: 430, height: 900 } });
const apage = await actx.newPage();
await apage.goto(ADMIN, { waitUntil: "domcontentloaded" });
await apage.evaluate(({ u, t }) => { localStorage.setItem("user_id", u); localStorage.setItem("auth_token", t); }, { u: expertSub, t: expertTok });
await apage.goto(ADMIN, { waitUntil: "domcontentloaded" });
const conv = apage.getByTestId("conv").filter({ hasText: clientSub });
await conv.first().waitFor({ state: "visible", timeout: 20000 });
await conv.first().click();
await apage.getByTestId("data-share-btn").first().waitFor({ state: "visible", timeout: 15000 });
await apage.getByTestId("data-share-btn").first().click();
await apage.getByTestId("data-share-modal").waitFor({ state: "visible", timeout: 10000 });
await apage.waitForTimeout(600);
await apage.screenshot({ path: "food-share-modal.png", fullPage: true });
const modalTxt = (await apage.getByTestId("data-share-modal").textContent().catch(() => "")) || "";
const hasToday = modalTxt.includes("Сегодня");
console.log("admin modal has «Сегодня»:", hasToday);

await cctx.close(); await actx.close(); await b.close();

// Completed days are on target → the share must show GREEN (today excluded),
// matching the client widget — not orange from counting the incomplete today.
console.log(`veg_fruit=${veg?.state}  calcium=${calcium?.state} (both must be green)`);
const ok = veg?.state === "green" && calcium?.state === "green" && hasToday;
console.log(ok ? "\n✅ PASS — indicators match the widget (green) + today row marked «Сегодня»" : "\n❌ FAIL");
process.exit(ok ? 0 : 1);
