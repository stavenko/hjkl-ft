// E2E: story-tray sorts unread stories (with unviewed frames) to the LEFT; a story
// becomes read after it's fully viewed and slides to the right, keeping natural
// order within each group. Seed all stories visible, read week2 fully, check order.
import { chromium } from "playwright";
const FE = process.argv[2] || "https://renorma-fit-dev.pages.dev";

const uid = "story-sort-" + Math.floor(Date.now() / 1000);
const b = await chromium.launch({ headless: true });
const ctx = await b.newContext({ viewport: { width: 430, height: 900 }, serviceWorkers: "block" });
const page = await ctx.newPage();

await page.goto(FE, { waitUntil: "domcontentloaded" });
await page.evaluate(async (uid) => {
  const del = (n) => new Promise((r) => { const q = indexedDB.deleteDatabase(n); q.onsuccess = q.onerror = q.onblocked = () => r(); });
  await del("hjkl-ft"); await del(`hjkl-ft-${uid}`);
  localStorage.clear();
  localStorage.setItem("user_id", uid);
  localStorage.setItem("auth_token", "x"); // no server calls needed for the tray
  localStorage.setItem("pwa_dismissed", "true");
  localStorage.setItem("profile_sex", "male");
}, uid);
await page.goto(FE, { waitUntil: "domcontentloaded" });
for (let i = 0; i < 40; i++) {
  const ready = await page.evaluate(async (uid) => {
    const dbs = await indexedDB.databases();
    if (!dbs.some((d) => d.name === `hjkl-ft-${uid}`)) return false;
    return await new Promise((res) => { const q = indexedDB.open(`hjkl-ft-${uid}`); q.onsuccess = () => { const ok = q.result.objectStoreNames.contains("app_flags"); q.result.close(); res(ok); }; q.onerror = () => res(false); });
  }, uid).catch(() => false);
  if (ready) break;
  await page.waitForTimeout(500);
}
// Seed: reach the dashboard + make every story visible (planka + activity + calcium).
await page.evaluate(async (uid) => {
  const open = (n) => new Promise((res, rej) => { const q = indexedDB.open(n); q.onsuccess = () => res(q.result); q.onerror = () => rej(q.error); });
  const db = await open(`hjkl-ft-${uid}`);
  const now = new Date(), nowIso = now.toISOString(), end = now.getTime() + 30 * 864e5;
  const ymd = (o) => { const d = new Date(); d.setDate(d.getDate() - o); return `${d.getFullYear()}-${String(d.getMonth() + 1).padStart(2, "0")}-${String(d.getDate()).padStart(2, "0")}`; };
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
    goals: [{ id: "cal", nutrient: "Calories", key: "calories", direction: "AtMost", amount: 2600, unit: "Kcal", period: "Day", created_at: nowIso, updated_at: nowIso }],
    weight_entries: [{ id: "w0", date: ymd(0), weight_kg: 80, no_water: false, no_food: false, no_wash: false, used_toilet: false, morning: true, created_at: nowIso, updated_at: nowIso }],
  };
  const avail = Array.from(db.objectStoreNames);
  for (const [store, rows] of Object.entries(rec)) { if (!avail.includes(store)) continue; await new Promise((res, rej) => { const tx = db.transaction([store], "readwrite"); const os = tx.objectStore(store); for (const row of rows) os.put(row); tx.oncomplete = () => res(); tx.onerror = () => rej(tx.error); }); }
  db.close();
}, uid);
await page.goto(FE, { waitUntil: "domcontentloaded" });
await page.waitForSelector("#splash", { state: "detached", timeout: 15000 }).catch(() => {});
await page.waitForTimeout(1500);

const order = () => page.$$eval('[data-testid="story-circle"]', (els) => els.map((e) => e.getAttribute("data-story-id")));
const before = await order();
console.log("order before:", before.join(","));

// Fully read week2: open it, tap through all frames (each tap advances; the tap
// past the last frame closes the viewer).
await page.click('[data-story-id="week2"]');
await page.waitForTimeout(600);
for (let i = 0; i < 9; i++) {
  // Tap the right 70% advance zone (bottom area to avoid the close ×).
  await page.mouse.click(300, 700);
  await page.waitForTimeout(180);
}
await page.waitForTimeout(800);
const after = await order();
console.log("order after :", after.join(","));
await page.screenshot({ path: "story-sort.png" });

await ctx.close(); await b.close();
// week2 must move behind all still-unread stories; unread ones keep natural order.
const idx = (arr, id) => arr.indexOf(id);
const week2Last = idx(after, "week2") === after.length - 1;
const unreadKeepOrder = ["welcome", "week1", "week3", "week4"].every((id) => idx(after, id) >= 0)
  && idx(after, "welcome") < idx(after, "week1") && idx(after, "week1") < idx(after, "week3") && idx(after, "week3") < idx(after, "week4");
const ok = before.join(",") === "welcome,week1,week2,week3,week4" && week2Last && unreadKeepOrder;
console.log(ok ? "\n✅ PASS — read story slid to the end; unread stay left in natural order" : "\n❌ FAIL");
process.exit(ok ? 0 : 1);
