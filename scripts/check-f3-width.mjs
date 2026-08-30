import { chromium } from "playwright";
// Адрес — как у всех остальных проверок: `FE=`. Аргументом он тоже принимается,
// но общий прогон его не передаёт, и проверка падала на `page.goto(undefined)`.
const FE = process.argv[2] || process.env.FE || "https://renorma-fit-dev.pages.dev";
const uid = "s4w-" + Date.now();
const b = await chromium.launch({ headless: true });
const ctx = await b.newContext({ viewport: { width: 430, height: 900 }, serviceWorkers: "block" });
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
    return await new Promise((res) => { const q = indexedDB.open(`hjkl-ft-${uid}`); q.onsuccess = () => { const ok = q.result.objectStoreNames.contains("app_flags") && q.result.objectStoreNames.contains("profile"); q.result.close(); res(ok); }; q.onerror = () => res(false); });
  }, uid).catch(() => false);
  if (ready) break;
  await page.waitForTimeout(500);
}
await page.evaluate(async (uid) => {
  const db = await new Promise((res, rej) => { const q = indexedDB.open(`hjkl-ft-${uid}`); q.onsuccess = () => res(q.result); q.onerror = () => rej(q.error); });
  const now = new Date().toISOString();
  const flags = [
    { key: "push_onboarding_dismissed", value: "true" },{ key: "paywall_skipped_date", value: now.slice(0,10) },
    { key: "ft_subscription", value: JSON.stringify({ plan: "monthly", end: Date.now()+30*864e5, active:true, start:Date.now(), status:"paid", no_renew:false, provider:"lava" }) },
    { key: "welcome_shown", value: "true" },{ key: "activity_week_unlocked", value: "true" },{ key: "calcium_week_unlocked", value: "true" },
  ];
  await new Promise((res)=>{const tx=db.transaction(["app_flags"],"readwrite");const os=tx.objectStore("app_flags");for(const f of flags)os.put(f);tx.oncomplete=res;});
  await new Promise((res)=>{const tx=db.transaction(["profile"],"readwrite");tx.objectStore("profile").put({key:"profile",sex:"male",height_cm:180,birth_year:1990,goal:"lose",cycle_start:null,steps_planka:8000,updated_at:now});tx.oncomplete=res;});
  db.close();
}, uid);
await page.goto(FE, { waitUntil: "domcontentloaded" });
await page.waitForSelector("#splash", { state: "detached", timeout: 15000 }).catch(()=>{});
await page.waitForSelector('[data-story-id="week4"]', { timeout: 15000 });
await page.waitForTimeout(700);
await page.click('[data-story-id="week4"]');
await page.waitForTimeout(900);
for (let i=0;i<2;i++){ await page.mouse.click(300,700); await page.waitForTimeout(550); }
await page.waitForTimeout(500);
const info = await page.evaluate(() => {
  const im = [...document.querySelectorAll('img')].find(i => i.src.includes('calcium-dairy'));
  if (!im) return "no img";
  const r = im.getBoundingClientRect();
  return { cssWidth: im.style.width, renderedPx: Math.round(r.width), viewport: window.innerWidth };
});
console.log(JSON.stringify(info));
const v = await page.evaluate(() => fetch('/version.json').then(r=>r.text()));
console.log("version", v);
await b.close();
