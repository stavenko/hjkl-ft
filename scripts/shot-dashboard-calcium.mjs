// Seed a dashboard where protein/veg/steps indicators are GREEN and calcium is
// ORANGE, with the calcium gauge partially filled (today ~650/1000). Screenshot
// the whole dashboard + the progress-widget region.
import { chromium } from "playwright";
const FE = process.argv[2] || "https://renorma-fit-dev.pages.dev";
const PAY = "https://payment-worker-dev.vg-stavenko.workers.dev";
const SECRET = "dev-secret-change-in-production";
const b64url = (b) => Buffer.from(b).toString("base64").replace(/=/g, "").replace(/\+/g, "-").replace(/\//g, "_");
async function jwt(sub) {
  const enc = new TextEncoder(); const now = Math.floor(Date.now() / 1000);
  const data = `${b64url(JSON.stringify({ alg: "HS256", typ: "JWT" }))}.${b64url(JSON.stringify({ sub, iat: now, exp: now + 3600, caps: [], token_id: "t" }))}`;
  const key = await crypto.subtle.importKey("raw", enc.encode(SECRET), { name: "HMAC", hash: "SHA-256" }, false, ["sign"]);
  const sig = await crypto.subtle.sign("HMAC", key, enc.encode(data));
  return `${data}.${b64url(new Uint8Array(sig))}`;
}
const uid = "dash-cal-" + Date.now();
const tok = await jwt(uid);
const co = await fetch(`${PAY}/test/guest-checkout`, { method: "POST", headers: { "Content-Type": "application/json" }, body: JSON.stringify({ planId: "test" }) });
const { claimId, secret } = await co.json();
await fetch(`${PAY}/claim`, { method: "POST", headers: { "Content-Type": "application/json", Authorization: `Bearer ${tok}` }, body: JSON.stringify({ claimId, secret }) });

const b = await chromium.launch({ headless: true });
const ctx = await b.newContext({ viewport: { width: 440, height: 1100 }, deviceScaleFactor: 2, serviceWorkers: "block" });
const page = await ctx.newPage();
await page.goto(FE, { waitUntil: "domcontentloaded" });
await page.evaluate(({ uid, tok }) => {
  localStorage.clear();
  localStorage.setItem("user_id", uid); localStorage.setItem("auth_token", tok);
  localStorage.setItem("token_id", "t"); localStorage.setItem("auth_ctx", "browser");
  localStorage.setItem("pwa_dismissed", "true"); localStorage.setItem("profile_sex", "male");
}, { uid, tok });
await page.goto(FE, { waitUntil: "domcontentloaded" });
for (let i = 0; i < 40; i++) {
  const ready = await page.evaluate(async (uid) => {
    const dbs = await indexedDB.databases();
    if (!dbs.some((d) => d.name === `hjkl-ft-${uid}`)) return false;
    return await new Promise((res) => { const q = indexedDB.open(`hjkl-ft-${uid}`); q.onsuccess = () => { const ok = q.result.objectStoreNames.contains("diary"); q.result.close(); res(ok); }; q.onerror = () => res(false); });
  }, uid).catch(() => false);
  if (ready) break;
  await page.waitForTimeout(500);
}
await page.evaluate(async (uid) => {
  const db = await new Promise((res, rej) => { const q = indexedDB.open(`hjkl-ft-${uid}`); q.onsuccess = () => res(q.result); q.onerror = () => rej(q.error); });
  const now = new Date(), nowIso = now.toISOString(), end = now.getTime() + 30 * 864e5;
  const ymd = (o) => { const d = new Date(); d.setDate(d.getDate() - o); return `${d.getFullYear()}-${String(d.getMonth() + 1).padStart(2, "0")}-${String(d.getDate()).padStart(2, "0")}`; };
  const N = 8;
  const food = (id, extra) => ({ id, name: id, kcal: 50, protein: 0, fat: 0, carbs: 0, nutrients: {}, package_weight: null, is_recipe: false, recipe_id: null, archived: false, is_restaurant: false, is_snack: false, is_veg_fruit: false, created_at: nowIso, updated_at: nowIso, ...extra });
  const rec = {
    app_flags: [
      { key: "push_onboarding_dismissed", value: "true" },
      { key: "paywall_skipped_date", value: ymd(0) },
      { key: "ft_subscription", value: JSON.stringify({ plan: "monthly", end, active: true, start: now.getTime(), status: "paid", no_renew: false, provider: "lava" }) },
      { key: "welcome_shown", value: "true" },
      { key: "activity_week_unlocked", value: "true" },
      { key: "calcium_week_unlocked", value: "true" },
    ],
    profile: [{ key: "profile", sex: "male", height_cm: 180, birth_year: 1990, goal: "lose", cycle_start: null, steps_planka: 8000, updated_at: nowIso }],
    goals: [
      { id: "cal", nutrient: "Calories", key: "calories", direction: "AtMost", amount: 2600, unit: "Kcal", period: "Day", created_at: nowIso, updated_at: nowIso },
      { id: "ca", nutrient: "Кальций", key: "calcium", direction: "AtLeast", amount: 1000, unit: "Mg", period: "Day", created_at: nowIso, updated_at: nowIso },
    ],
    weight_entries: Array.from({ length: N }, (_, i) => ({ id: "w" + i, date: ymd(i), weight_kg: 80, no_water: false, no_food: false, no_wash: false, used_toilet: false, morning: true, created_at: nowIso, updated_at: nowIso })),
    step_entries: Array.from({ length: N }, (_, i) => ({ id: "s" + i, date: ymd(i), steps: 9000, created_at: nowIso, updated_at: nowIso })),
    // Three foods to control each metric independently.
    foods: [
      food("prot", { protein: 160 }),      // 100 g → 160 g protein/day → green
      food("veg", { is_veg_fruit: true }),  // 850 g → veg green
      food("cal", { nutrients: { "Кальций": 1000 } }), // 1000 mg / 100 g
    ],
    diary: [],
  };
  for (let i = 0; i < N; i++) {
    rec.diary.push({ id: "dp" + i, food_id: "prot", date: ymd(i), time: null, grams: 100, waste_grams: 0, meal_label: "lunch", deleted: false, created_at: nowIso, updated_at: nowIso });
    rec.diary.push({ id: "dv" + i, food_id: "veg", date: ymd(i), time: null, grams: 850, waste_grams: 0, meal_label: "lunch", deleted: false, created_at: nowIso, updated_at: nowIso });
    // Calcium: today ~650 (gauge partial); completed days: 2 misses (i=1,2 →400), rest 1100 → orange indicator.
    const g = i === 0 ? 65 : (i <= 2 ? 40 : 110);
    rec.diary.push({ id: "dc" + i, food_id: "cal", date: ymd(i), time: null, grams: g, waste_grams: 0, meal_label: "lunch", deleted: false, created_at: nowIso, updated_at: nowIso });
  }
  const avail = Array.from(db.objectStoreNames);
  for (const [store, rows] of Object.entries(rec)) { if (!rows.length || !avail.includes(store)) continue; await new Promise((res, rej) => { const tx = db.transaction([store], "readwrite"); const os = tx.objectStore(store); for (const row of rows) os.put(row); tx.oncomplete = () => res(); tx.onerror = () => rej(tx.error); }); }
  db.close();
}, uid);
await page.goto(FE, { waitUntil: "domcontentloaded" });
await page.waitForSelector("#splash", { state: "detached", timeout: 15000 }).catch(() => {});
await page.waitForTimeout(2500);
await page.screenshot({ path: "dashboard-calcium.png", fullPage: true });
console.log("dashboard shot ok");
await b.close();
