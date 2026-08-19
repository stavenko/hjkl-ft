// Reproduce the "photo detection creates duplicate foods" bug on DEV.
// Runs the «По фото еды» flow TWICE with the SAME real food photo and reports how
// many Food rows landed in the base — expecting one new Food per run (the dup bug).
import { chromium } from "playwright";
import { readFileSync } from "fs";
const FE = "https://renorma-fit-dev.pages.dev";
const uid = readFileSync("/tmp/repro_uid.txt", "utf8").trim();
const token = readFileSync("/tmp/repro_token.txt", "utf8").trim();
const PHOTO = "/Users/vasilijstavenko/projects/hjkl-ft/frontend/story-img/calcium-dairy.jpg";

const b = await chromium.launch({ headless: true });
const ctx = await b.newContext({ viewport: { width: 430, height: 920 }, serviceWorkers: "block" });
const page = await ctx.newPage();
page.on("console", (m) => { const t = m.text(); if (/error|fail|402|denied/i.test(t)) console.log("  [page]", t); });

await page.goto(FE, { waitUntil: "domcontentloaded" });
await page.evaluate(async ({ uid, token }) => {
  const del = (n) => new Promise((r) => { const q = indexedDB.deleteDatabase(n); q.onsuccess = q.onerror = q.onblocked = () => r(); });
  await del("hjkl-ft"); await del(`hjkl-ft-${uid}`);
  localStorage.clear();
  localStorage.setItem("user_id", uid);
  localStorage.setItem("auth_token", token);
  localStorage.setItem("token_id", "tok1");
  localStorage.setItem("auth_ctx", "browser");
  localStorage.setItem("pwa_dismissed", "true");
  localStorage.setItem("paywall_skipped_date", new Date().toISOString().slice(0, 10));
}, { uid, token });
await page.goto(FE, { waitUntil: "domcontentloaded" });
await page.waitForSelector("#splash", { state: "detached", timeout: 20000 }).catch(() => {});
await page.waitForTimeout(2000);

const countFoods = async () => page.evaluate(async (uid) => {
  const db = await new Promise((res, rej) => { const q = indexedDB.open(`hjkl-ft-${uid}`); q.onsuccess = () => res(q.result); q.onerror = () => rej(q.error); });
  if (!db.objectStoreNames.contains("foods")) { db.close(); return []; }
  const all = await new Promise((res) => { const tx = db.transaction(["foods"], "readonly"); const rq = tx.objectStore("foods").getAll(); rq.onsuccess = () => res(rq.result || []); rq.onerror = () => res([]); });
  db.close();
  return all.filter(f => !f.archived).map(f => ({ id: f.id, name: f.name }));
}, uid);

const runOnce = async (label) => {
  console.log(`\n=== run ${label} ===`);
  await page.goto(FE + "/diary/add", { waitUntil: "domcontentloaded" });
  await page.waitForTimeout(1800);
  await page.getByText("Добавить новый продукт").click();
  await page.waitForTimeout(800);
  await page.getByText("По фото еды").click();
  await page.waitForTimeout(600);
  await page.setInputFiles("#fitems-photo-input", PHOTO);
  await page.waitForTimeout(800);
  await page.getByRole("button", { name: /Определить еду/ }).click();
  console.log("  submitted, waiting for detection (poller+Qwen)…");
  // Wait until the "Добавить все продукты" button appears (list resolved).
  const addAll = page.getByRole("button", { name: /Добавить все продукты/ });
  const deadline = Date.now() + 300000;
  let ok = false;
  while (Date.now() < deadline) {
    if (await addAll.isVisible().catch(() => false)) { ok = true; break; }
    const status = await page.evaluate(() => {
      const err = [...document.querySelectorAll('.has-text-danger')].map(e => e.textContent.trim()).filter(Boolean);
      const btn = [...document.querySelectorAll('button')].map(b => b.textContent.trim()).find(t => /Определить|ток|с$|Распозна|queue|очеред/i.test(t));
      return { err, btn };
    });
    console.log(`   …${Math.round((300000 - (deadline - Date.now())) / 1000)}s btn="${status.btn || ''}" err=${JSON.stringify(status.err).slice(0,160)}`);
    if (status.err && status.err.length) { console.log("  DETECTION ERROR surfaced → aborting this run"); break; }
    await page.waitForTimeout(10000);
  }
  if (!ok) { console.log("  add-all button never appeared"); return; }
  // detected names
  const names = await page.evaluate(() => [...document.querySelectorAll('input')].map(i => i.value).filter(Boolean));
  console.log("  detected row values:", JSON.stringify(names).slice(0, 300));
  await page.waitForTimeout(500);
  await addAll.click();
  await page.waitForURL(/\/diary(\?|$)/, { timeout: 20000 }).catch(() => {});
  await page.waitForTimeout(1500);
  const foods = await countFoods();
  console.log(`  foods in base after run ${label}: ${foods.length}`);
  console.log("   ", JSON.stringify(foods));
};

console.log("foods at start:", (await countFoods()).length);
await runOnce("1");
await runOnce("2");

const final = await countFoods();
const byName = {};
for (const f of final) byName[f.name] = (byName[f.name] || 0) + 1;
console.log("\n=== RESULT ===");
console.log("total foods in base:", final.length);
console.log("counts by name:", JSON.stringify(byName, null, 1));
await b.close();
