// E2E: the Settings «Обновить» button shows an INSTANT spinner + disables on click
// (before the ~1s reload), instead of feeling dead. Force the update to be
// available by faking a newer /version.json, then click and read the button state
// synchronously (Leptos updates the DOM in the click handler, before reload()).
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
const uid = "upd-" + Date.now();
const tok = await jwt(uid);
// activate a fake paid sub so the app boots past the lock screen
const co = await fetch(`${PAY}/test/guest-checkout`, { method: "POST", headers: { "Content-Type": "application/json" }, body: JSON.stringify({ planId: "test" }) });
const { claimId, secret } = await co.json();
await fetch(`${PAY}/claim`, { method: "POST", headers: { "Content-Type": "application/json", Authorization: `Bearer ${tok}` }, body: JSON.stringify({ claimId, secret }) });

const b = await chromium.launch({ headless: true });
const ctx = await b.newContext({ viewport: { width: 430, height: 900 }, serviceWorkers: "block" });
// Fake a NEWER deployed version so update::available() flips true → «Обновить» shows.
await ctx.route("**/version.json*", (route) => route.fulfill({ status: 200, contentType: "application/json", body: JSON.stringify({ v: "newer-test-9999" }) }));
const page = await ctx.newPage();

await page.goto(FE, { waitUntil: "domcontentloaded" });
await page.evaluate(({ uid, tok }) => {
  localStorage.clear();
  localStorage.setItem("user_id", uid); localStorage.setItem("auth_token", tok);
  localStorage.setItem("token_id", "t"); localStorage.setItem("auth_ctx", "browser");
  localStorage.setItem("pwa_dismissed", "true"); localStorage.setItem("profile_sex", "male");
}, { uid, tok });
await page.goto(FE, { waitUntil: "domcontentloaded" });
// minimal profile so we're past the persona overlay
for (let i = 0; i < 40; i++) {
  const ready = await page.evaluate(async (uid) => {
    const dbs = await indexedDB.databases();
    if (!dbs.some((d) => d.name === `hjkl-ft-${uid}`)) return false;
    return await new Promise((res) => { const q = indexedDB.open(`hjkl-ft-${uid}`); q.onsuccess = () => { const ok = q.result.objectStoreNames.contains("profile"); q.result.close(); res(ok); }; q.onerror = () => res(false); });
  }, uid).catch(() => false);
  if (ready) break;
  await page.waitForTimeout(500);
}
await page.evaluate(async (uid) => {
  const db = await new Promise((res, rej) => { const q = indexedDB.open(`hjkl-ft-${uid}`); q.onsuccess = () => res(q.result); q.onerror = () => rej(q.error); });
  const now = new Date().toISOString();
  await new Promise((res) => { const tx = db.transaction(["profile"], "readwrite"); tx.objectStore("profile").put({ key: "profile", sex: "male", height_cm: 180, birth_year: 1990, goal: "lose", cycle_start: null, steps_planka: null, updated_at: now }); tx.oncomplete = res; });
  db.close();
}, uid);
await page.goto(`${FE}/settings`, { waitUntil: "domcontentloaded" });
await page.waitForSelector("#splash", { state: "detached", timeout: 15000 }).catch(() => {});

const btn = page.getByTestId("settings-btn-update");
await btn.waitFor({ state: "visible", timeout: 20000 });
const before = await page.getByTestId("settings-btn-update").evaluate((el) => ({ cls: el.className, disabled: el.disabled }));
console.log("before click:", JSON.stringify(before));

// Click + read state SYNCHRONOUSLY (the Leptos handler updates the DOM before the
// 50ms-delayed reload fires) so the check isn't racing the navigation.
const after = await page.evaluate(() => {
  const el = document.querySelector('[data-testid="settings-btn-update"]');
  el.click();
  return { cls: el.className, disabled: el.disabled };
});
console.log("after click :", JSON.stringify(after));

await b.close();
const ok = !before.cls.includes("is-loading") && after.cls.includes("is-loading") && after.disabled === true;
console.log(ok ? "\n✅ PASS — «Обновить» shows a spinner + disables instantly on click" : "\n❌ FAIL");
process.exit(ok ? 0 : 1);
