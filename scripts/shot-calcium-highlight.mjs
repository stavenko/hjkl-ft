// Build the calcium GAUGE and INDICATOR "blinking sparkle" GIFs for Story 4 frame 6.
// Seed a dashboard (all indicators green except calcium; calcium gauge ~650/1000),
// then draw a pulsing radial-sparkle halo around each target element, capture a
// loop of frames, and encode a GIF (ffmpeg). Mirrors the existing dashboard-*.gif
// highlight style.
import { chromium } from "playwright";
import { execSync } from "node:child_process";
import { mkdirSync, rmSync } from "node:fs";

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
const uid = "cal-hl-" + Date.now();
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
    foods: [food("prot", { protein: 160 }), food("veg", { is_veg_fruit: true }), food("cal", { nutrients: { "Кальций": 1000 } })],
    diary: [],
  };
  for (let i = 0; i < N; i++) {
    rec.diary.push({ id: "dp" + i, food_id: "prot", date: ymd(i), time: null, grams: 100, waste_grams: 0, meal_label: "lunch", deleted: false, created_at: nowIso, updated_at: nowIso });
    rec.diary.push({ id: "dv" + i, food_id: "veg", date: ymd(i), time: null, grams: 850, waste_grams: 0, meal_label: "lunch", deleted: false, created_at: nowIso, updated_at: nowIso });
    const g = i === 0 ? 65 : (i <= 2 ? 40 : 110);
    rec.diary.push({ id: "dc" + i, food_id: "cal", date: ymd(i), time: null, grams: g, waste_grams: 0, meal_label: "lunch", deleted: false, created_at: nowIso, updated_at: nowIso });
  }
  const avail = Array.from(db.objectStoreNames);
  for (const [store, rows] of Object.entries(rec)) { if (!rows.length || !avail.includes(store)) continue; await new Promise((res, rej) => { const tx = db.transaction([store], "readwrite"); const os = tx.objectStore(store); for (const row of rows) os.put(row); tx.oncomplete = () => res(); tx.onerror = () => rej(tx.error); }); }
  db.close();
}, uid);
await page.goto(FE, { waitUntil: "domcontentloaded" });
await page.waitForSelector("#splash", { state: "detached", timeout: 15000 }).catch(() => {});
await page.waitForTimeout(2600); // let the gauge fill animation settle

// Sparkle drawer (inside page.evaluate — not subject to the page CSP). Draws a
// radiating burst of HAND-DRAWN strokes (SVG tapered + slightly curved, like the
// existing welcome-persona.gif / dashboard-*.gif) around each rect, that shoot out,
// peak, then fade to a pause — a blink synced across the whole burst.
const drawSpark = ({ rects, frame, frames }) => {
  let ov = document.getElementById("__spk");
  if (!ov) { ov = document.createElement("div"); ov.id = "__spk"; ov.style.cssText = "position:absolute;left:0;top:0;width:0;height:0;pointer-events:none;z-index:99999;"; document.body.appendChild(ov); }
  // Snappy on/off blink like the original: strokes SHOWN on ~5 of 9 frames, GONE on
  // the rest (frame 1 + last 3). No smooth synchronized fade.
  const ON = frame >= 1 && frame <= 5;
  if (!ON) { ov.innerHTML = ""; return; }
  // Angles/positions are STABLE per stroke (seed by index); lengths + curve FLICKER
  // per frame (seed by index+frame) so the burst "boils" like a hand-drawn loop.
  const rS = (i, k) => { const x = Math.sin(i * 127.1 + k * 311.7) * 43758.5453; return x - Math.floor(x); };
  const rF = (i, k) => { const x = Math.sin(i * 127.1 + k * 311.7 + frame * 57.13) * 43758.5453; return x - Math.floor(x); };
  const env = 0.72 + 0.28 * Math.sin(Math.PI * (frame - 0.5) / 5); // mild fuller-in-middle
  const N = 22;
  let paths = "";
  const gap = 3;
  for (const rect of rects) {
    const cx = rect.x + rect.w / 2 + scrollX, cy = rect.y + rect.h / 2 + scrollY;
    // Wide elements (gauge bar) emit strokes from the ROUNDED-RECT perimeter, normal
    // to the nearest edge; square-ish ones (icon) emit radially from the centre.
    const wide = rect.w > rect.h * 1.8;
    const nn = wide ? Math.round(N * 1.4) : N; // a longer perimeter needs more strokes
    for (let i = 0; i < nn; i++) {
      let sx, sy, dx, dy;
      if (wide) {
        const hw = rect.w / 2 + gap, hh = rect.h / 2 + gap;
        const W = 2 * hw, H = 2 * hh, P = 2 * (W + H);
        let d = (((i + (rS(i, 7) - 0.5) * 0.7) / nn) * P % P + P) % P;
        let lx, ly, nx, ny;
        if (d < W) { lx = -hw + d; ly = -hh; nx = 0; ny = -1; }
        else if (d < W + H) { lx = hw; ly = -hh + (d - W); nx = 1; ny = 0; }
        else if (d < 2 * W + H) { lx = hw - (d - W - H); ly = hh; nx = 0; ny = 1; }
        else { lx = -hw; ly = hh - (d - 2 * W - H); nx = -1; ny = 0; }
        sx = cx + lx; sy = cy + ly;
        const ja = (rS(i, 1) - 0.5) * 0.5; // jitter the outward normal
        dx = nx * Math.cos(ja) - ny * Math.sin(ja);
        dy = nx * Math.sin(ja) + ny * Math.cos(ja);
      } else {
        const a = (i / nn) * 2 * Math.PI + (rS(i, 1) - 0.5) * 0.34;
        const r0 = Math.max(rect.w, rect.h) / 2 + gap + rS(i, 2) * 2;
        sx = cx + Math.cos(a) * r0; sy = cy + Math.sin(a) * r0;
        dx = Math.cos(a); dy = Math.sin(a);
      }
      const len = (8 + rF(i, 3) * 13) * env; // per-frame length flicker
      if (len < 2) continue;
      const ex = sx + dx * len, ey = sy + dy * len;
      const px = -dy, py = dx;               // perpendicular to the stroke
      const cur = (rF(i, 4) - 0.5) * 5;      // per-frame slight curve
      const mx = (sx + ex) / 2 + px * cur, my = (sy + ey) / 2 + py * cur;
      const w = (1.5 + rS(i, 5) * 1.0);      // taper width (mid)
      const t1x = mx + px * w, t1y = my + py * w, t2x = mx - px * w, t2y = my - py * w;
      paths += `<path d="M${sx.toFixed(1)} ${sy.toFixed(1)} Q${t1x.toFixed(1)} ${t1y.toFixed(1)} ${ex.toFixed(1)} ${ey.toFixed(1)} Q${t2x.toFixed(1)} ${t2y.toFixed(1)} ${sx.toFixed(1)} ${sy.toFixed(1)} Z" fill="#6b7482"/>`;
    }
  }
  ov.innerHTML = `<svg width="${document.documentElement.scrollWidth}" height="${document.documentElement.scrollHeight}" style="position:absolute;left:0;top:0;overflow:visible;">${paths}</svg>`;
};

// One GIF cropped to the WHOLE progress widget, with the sparkle burst on the given
// selectors (e.g. the calcium indicator) — so all indicators stay visible.
async function makeWidgetGif(sparkSels, name) {
  const box = (sel) => page.$eval(sel, (el) => { const r = el.getBoundingClientRect(); return { x: r.x, y: r.y, w: r.width, h: r.height }; });
  const widget = await box('[data-testid="progress-widget"]');
  const rects = [];
  for (const s of sparkSels) rects.push(await box(s));
  const M = 30; // room for the rays overflowing the card + a little background
  const clip = { x: Math.max(0, widget.x - M), y: Math.max(0, widget.y - M), width: widget.w + 2 * M, height: widget.h + 2 * M };
  const dir = `/tmp/spk-${name}`;
  rmSync(dir, { recursive: true, force: true }); mkdirSync(dir, { recursive: true });
  const FRAMES = 9; // match the originals: 9 frames @ ~11fps (0.82s loop)
  for (let f = 0; f < FRAMES; f++) {
    await page.evaluate(drawSpark, { rects, frame: f, frames: FRAMES });
    await page.screenshot({ path: `${dir}/f${String(f).padStart(2, "0")}.png`, clip });
  }
  await page.evaluate(() => { const o = document.getElementById("__spk"); if (o) o.remove(); });
  execSync(`ffmpeg -y -framerate 11 -pattern_type glob -i '${dir}/f*.png' -vf palettegen /tmp/pal-${name}.png`, { stdio: "ignore" });
  execSync(`ffmpeg -y -framerate 11 -pattern_type glob -i '${dir}/f*.png' -i /tmp/pal-${name}.png -lavfi paletteuse -loop 0 ${name}.gif`, { stdio: "ignore" });
  console.log(`${name}.gif built`);
}

// ── Alternative: a clean pulsing GLOW RING (no anime rays) ────────────────────
const drawGlow = ({ items, phase }) => {
  let ov = document.getElementById("__spk");
  if (!ov) { ov = document.createElement("div"); ov.id = "__spk"; ov.style.cssText = "position:absolute;left:0;top:0;width:0;height:0;pointer-events:none;z-index:99999;"; document.body.appendChild(ov); }
  ov.innerHTML = "";
  const t = 0.5 - 0.5 * Math.cos(2 * Math.PI * phase); // breathe 0..1..0
  for (const it of items) {
    const size = it.d * (1 + 0.16 * t);
    const op = 0.4 + 0.55 * t;
    const el = document.createElement("div");
    el.style.cssText = "position:absolute;left:" + (it.cx - size / 2) + "px;top:" + (it.cy - size / 2) + "px;width:" + size + "px;height:" + size + "px;border-radius:50%;border:2.5px solid " + it.color + ";box-shadow:0 0 " + (7 + 16 * t) + "px 1px " + it.glow + ";opacity:" + op + ";";
    ov.appendChild(el);
  }
};
async function makeGlowGif(name) {
  const box = (sel) => page.$eval(sel, (el) => { const r = el.getBoundingClientRect(); return { x: r.x, y: r.y, w: r.width, h: r.height }; });
  const widget = await box('[data-testid="progress-widget"]');
  const icon = await box('[data-ind="Кальций"] > div'); // the 38px icon circle
  const items = [{ cx: icon.x + icon.w / 2 + 0, cy: icon.y + icon.h / 2 + 0, d: Math.max(icon.w, icon.h) + 10, color: "#e8850d", glow: "rgba(232,133,13,0.55)" }];
  // convert to page coords (add scroll) inside evaluate via closure-free values
  const so = await page.evaluate(() => ({ x: scrollX, y: scrollY }));
  items.forEach((it) => { it.cx += so.x; it.cy += so.y; });
  const M = 30;
  const clip = { x: Math.max(0, widget.x - M), y: Math.max(0, widget.y - M), width: widget.w + 2 * M, height: widget.h + 2 * M };
  const dir = `/tmp/spk-${name}`; rmSync(dir, { recursive: true, force: true }); mkdirSync(dir, { recursive: true });
  const FRAMES = 14;
  for (let f = 0; f < FRAMES; f++) { await page.evaluate(drawGlow, { items, phase: f / FRAMES }); await page.screenshot({ path: `${dir}/f${String(f).padStart(2, "0")}.png`, clip }); }
  await page.evaluate(() => { const o = document.getElementById("__spk"); if (o) o.remove(); });
  execSync(`ffmpeg -y -framerate 14 -pattern_type glob -i '${dir}/f*.png' -vf palettegen /tmp/pal-${name}.png`, { stdio: "ignore" });
  execSync(`ffmpeg -y -framerate 14 -pattern_type glob -i '${dir}/f*.png' -i /tmp/pal-${name}.png -lavfi paletteuse -loop 0 ${name}.gif`, { stdio: "ignore" });
  console.log(`${name}.gif built`);
}

await makeWidgetGif(['[data-ind="Кальций"] > div', '[data-gauge="Кальций"]'], "calcium-highlight");
await b.close();
console.log("done");
