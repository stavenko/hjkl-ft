// Playwright E2E of the admin planka chat-command on dev.
// Setup (fetch): approve an expert; a client opens a support thread.
// Browser: inject the expert JWT → Queue → open the client's thread → type
// "/set-calorie-limit 2600" → send → assert the confirmation banner.
// Verify (fetch): the client's synced Calories goal is now 2600.
import { chromium } from "playwright";

const ADMIN = process.argv[2] || "https://renorma-admin-dev.pages.dev";
const SUP = "https://support-worker-dev.vg-stavenko.workers.dev";
const SYNC = "https://sync-worker-dev.vg-stavenko.workers.dev";
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

// 1) approve the expert
let r = await fetch(`${SUP}/admin/request`, { method: "POST", headers: { Authorization: `Bearer ${expertTok}`, "Content-Type": "application/json" }, body: "{}" });
const { code } = await r.json();
r = await fetch(`${SUP}/admin/approve`, { method: "POST", headers: { "X-Admin-Secret": APPROVE, "Content-Type": "application/json" }, body: JSON.stringify({ code }) });
console.log("approve expert:", r.status);

// 2) client opens a support thread (so it shows in the queue)
r = await fetch(`${SUP}/message`, { method: "POST", headers: { Authorization: `Bearer ${clientTok}`, "Content-Type": "application/json" }, body: JSON.stringify({ client_id: "c-" + ts, text: "У меня слишком низкая планка, помогите" }) });
console.log("client message:", r.status);

// 3) drive the admin console
const b = await chromium.launch({ headless: true });
const ctx = await b.newContext({ viewport: { width: 430, height: 900 } });
const page = await ctx.newPage();
page.on("console", (m) => { if (m.type() === "error") console.log("  [browser error]", m.text()); });

await page.goto(ADMIN, { waitUntil: "domcontentloaded" });
await page.evaluate(({ u, t }) => { localStorage.setItem("user_id", u); localStorage.setItem("auth_token", t); }, { u: expertSub, t: expertTok });
await page.goto(ADMIN, { waitUntil: "domcontentloaded" });

// Land on the Queue, open THIS run's conversation (the queue is oldest-first and
// accumulates across runs, so target the row by our unique client id — not .first()).
const conv = page.getByTestId("conv").filter({ hasText: clientSub });
await conv.first().waitFor({ state: "visible", timeout: 20000 });
await page.screenshot({ path: "admin-queue.png", fullPage: true });
await conv.first().click();

// Type the command and send.
const input = page.getByTestId("reply-input");
await input.waitFor({ state: "visible", timeout: 10000 });
await input.fill("/set-calorie-limit 2600");
await page.screenshot({ path: "admin-command.png", fullPage: true });
await page.getByTestId("reply-send").click();

// Confirmation banner.
await page.getByText("Планка установлена").waitFor({ state: "visible", timeout: 15000 });
await page.screenshot({ path: "admin-confirmed.png", fullPage: true });
console.log("banner: confirmation shown ✓");

await ctx.close();
await b.close();

// 4) verify server-side
r = await fetch(`${SYNC}/sync/dump`, { method: "POST", headers: { Authorization: `Bearer ${clientTok}`, "Content-Type": "application/json" }, body: "{}" });
const dump = await r.json();
const cal = (dump.goals || []).find((g) => g.nutrient === "Calories" && g.direction === "AtMost");
console.log("client synced planka =", JSON.stringify(cal));
const ok = cal && cal.amount === 2600;
console.log(ok ? "\n✅ PASS — /set-calorie-limit in the admin UI set the client's planka" : "\n❌ FAIL");
process.exit(ok ? 0 : 1);
