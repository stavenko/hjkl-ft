// Verify the global press feedback: screenshot a button released vs held down.
import { chromium } from "playwright";
import { openSeeded } from "./harness.mjs";

const baseUrl = process.argv[2];
const b = await chromium.launch({ headless: true });
const { context, page } = await openSeeded(b, {
  baseUrl,
  landing: "/",
  seed: async (page, uid) => {
    await page.evaluate(async (uid) => {
      const open = (n) => new Promise((res, rej) => { const r = indexedDB.open(n); r.onsuccess = () => res(r.result); r.onerror = () => rej(r.error); });
      const db = await open(`hjkl-ft-${uid}`);
      const now = new Date(); const nowIso = now.toISOString();
      const ymd = (o) => { const d = new Date(); d.setDate(d.getDate() - o); return `${d.getFullYear()}-${String(d.getMonth()+1).padStart(2,"0")}-${String(d.getDate()).padStart(2,"0")}`; };
      const end = now.getTime() + 30*24*60*60*1000;
      const records = {
        app_flags: [
          { key: "push_onboarding_dismissed", value: "true" },
          { key: "paywall_skipped_date", value: ymd(0) },
          { key: "ft_subscription", value: JSON.stringify({ plan:"monthly", end, active:true, start:now.getTime(), status:"paid", no_renew:false, provider:"lava" }) },
        ],
        profile: [{ key:"profile", sex:"male", height_cm:180, birth_year:1990, goal:"lose", cycle_start:null, steps_planka:8000, updated_at:nowIso }],
      };
      const available = Array.from(db.objectStoreNames);
      for (const [store, rows] of Object.entries(records)) {
        if (!rows.length || !available.includes(store)) continue;
        await new Promise((res, rej) => { const tx = db.transaction([store],"readwrite"); const os = tx.objectStore(store); for (const r of rows) os.put(r); tx.oncomplete=()=>res(); tx.onerror=()=>rej(tx.error); });
      }
      db.close();
    }, uid);
  },
});

await page.waitForTimeout(1500);
// Target the story-tray circle badged "1" — a plain grey button, easy to see dim.
const btn = page.getByRole("button", { name: "1", exact: true }).first();
await btn.waitFor({ state: "visible", timeout: 10000 });
const box = await btn.boundingBox();
const cx = box.x + box.width / 2, cy = box.y + box.height / 2;

await page.screenshot({ path: "press-idle.png", clip: { x: box.x - 10, y: box.y - 10, width: box.width + 20, height: box.height + 20 } });
await page.mouse.move(cx, cy);
await page.mouse.down(); // hold → :active
await page.waitForTimeout(120);
await page.screenshot({ path: "press-active.png", clip: { x: box.x - 10, y: box.y - 10, width: box.width + 20, height: box.height + 20 } });
await page.mouse.up();
await context.close(); await b.close();
console.log("ok");
