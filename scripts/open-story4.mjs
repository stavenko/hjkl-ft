// Open a HEADED browser on dev, seeded so Story 4 (calcium week) is in the tray,
// and LEAVE IT OPEN so the user can click the «4» circle and swipe to frame 6.
import { chromium } from "playwright";
const FE = process.argv[2] || "https://renorma-fit-dev.pages.dev";
const uid = "s4-live-" + Date.now();
const b = await chromium.launch({ headless: false });
const ctx = await b.newContext({ viewport: { width: 430, height: 920 } });
const page = await ctx.newPage();
await page.goto(FE, { waitUntil: "domcontentloaded" });
await page.evaluate(async (uid) => {
  const del = (n) => new Promise((r) => { const q = indexedDB.deleteDatabase(n); q.onsuccess = q.onerror = q.onblocked = () => r(); });
  await del("hjkl-ft"); await del(`hjkl-ft-${uid}`);
  localStorage.clear();
  localStorage.setItem("user_id", uid); localStorage.setItem("auth_token", "x");
  localStorage.setItem("pwa_dismissed", "true"); localStorage.setItem("profile_sex", "male");
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
await page.evaluate(async (uid) => {
  const db = await new Promise((res, rej) => { const q = indexedDB.open(`hjkl-ft-${uid}`); q.onsuccess = () => res(q.result); q.onerror = () => rej(q.error); });
  const now = new Date().toISOString();
  const flags = [
    { key: "push_onboarding_dismissed", value: "true" },
    { key: "paywall_skipped_date", value: now.slice(0, 10) },
    { key: "ft_subscription", value: JSON.stringify({ plan: "monthly", end: Date.now() + 30 * 864e5, active: true, start: Date.now(), status: "paid", no_renew: false, provider: "lava" }) },
    { key: "welcome_shown", value: "true" },
    { key: "activity_week_unlocked", value: "true" },
    { key: "calcium_week_unlocked", value: "true" },
  ];
  await new Promise((res) => { const tx = db.transaction(["app_flags"], "readwrite"); const os = tx.objectStore("app_flags"); for (const f of flags) os.put(f); tx.oncomplete = res; });
  await new Promise((res) => { const tx = db.transaction(["profile"], "readwrite"); tx.objectStore("profile").put({ key: "profile", sex: "male", height_cm: 180, birth_year: 1990, goal: "lose", cycle_start: null, steps_planka: 8000, updated_at: now }); tx.oncomplete = res; });
  db.close();
}, uid);
await page.goto(FE, { waitUntil: "domcontentloaded" });
console.log("READY — тапни кружок «4» в ленте историй и листай до 6-го кадра. Окно останется открытым.");
// Keep the browser open until the window is closed.
await new Promise(() => {});
