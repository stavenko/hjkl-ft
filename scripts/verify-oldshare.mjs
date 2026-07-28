// Repro: a data_share in OLD format (no indicators/planka_days, as shared before
// the deploy) — does the NEW admin still render it, or show "no data"?
import { chromium } from "playwright";
const ADMIN = process.argv[2] || "https://renorma-admin-dev.pages.dev";
const SUP = "https://support-worker-dev.vg-stavenko.workers.dev";
const SECRET = "dev-secret-change-in-production";
const APPROVE = "dev-admin-approve-secret";
const b64url = (b) => Buffer.from(b).toString("base64").replace(/=/g, "").replace(/\+/g, "-").replace(/\//g, "_");
async function jwt(sub) {
  const enc = new TextEncoder(); const now = Math.floor(Date.now() / 1000);
  const data = `${b64url(JSON.stringify({ alg: "HS256", typ: "JWT" }))}.${b64url(JSON.stringify({ sub, iat: now, exp: now + 3600, caps: [], token_id: "t" }))}`;
  const key = await crypto.subtle.importKey("raw", enc.encode(SECRET), { name: "HMAC", hash: "SHA-256" }, false, ["sign"]);
  const sig = await crypto.subtle.sign("HMAC", key, enc.encode(data));
  return `${data}.${b64url(new Uint8Array(sig))}`;
}
const ts = Date.now();
const expertSub = "e2e-expert-" + ts, clientSub = "e2e-client-" + ts;
const expertTok = await jwt(expertSub), clientTok = await jwt(clientSub);
let r = await fetch(`${SUP}/admin/request`, { method: "POST", headers: { Authorization: `Bearer ${expertTok}`, "Content-Type": "application/json" }, body: "{}" });
const { code } = await r.json();
await fetch(`${SUP}/admin/approve`, { method: "POST", headers: { "X-Admin-Secret": APPROVE, "Content-Type": "application/json" }, body: JSON.stringify({ code }) });

// OLD-format single-food data_share (what a pre-deploy client would have sent).
const oldPayload = JSON.stringify({ food: { days: [
  { date: "2026-07-27", entries: [{ name: "Овсянка на молоке", grams: 250, kcal: 225, protein: 9, fat: 6, carbs: 32 }], totals: { kcal: 225, protein: 9, fat: 6, carbs: 32 } },
] } });
r = await fetch(`${SUP}/message`, { method: "POST", headers: { Authorization: `Bearer ${clientTok}`, "Content-Type": "application/json" },
  body: JSON.stringify({ client_id: "share-" + ts, text: "Поделился дневником", kind: "data_share", payload: oldPayload }) });
console.log("client posted OLD data_share:", r.status);

const b = await chromium.launch({ headless: true });
const apage = await (await b.newContext({ viewport: { width: 430, height: 900 } })).newPage();
apage.on("pageerror", (e) => console.log("  [admin pageerror]", e.message));
await apage.goto(ADMIN, { waitUntil: "domcontentloaded" });
await apage.evaluate(({ u, t }) => { localStorage.setItem("user_id", u); localStorage.setItem("auth_token", t); }, { u: expertSub, t: expertTok });
await apage.goto(ADMIN, { waitUntil: "domcontentloaded" });
const conv = apage.getByTestId("conv").filter({ hasText: clientSub });
await conv.first().waitFor({ state: "visible", timeout: 20000 });
await conv.first().click();
await apage.waitForTimeout(2500);
const btn = await apage.getByTestId("data-share-btn").count();
const err = await apage.locator("text=Не удалось прочитать данные").first().textContent().catch(() => null);
console.log("admin buttons:", btn, "| parse error:", err || "none");
let modalTxt = "";
if (btn > 0) {
  await apage.getByTestId("data-share-btn").first().click();
  await apage.getByTestId("data-share-modal").waitFor({ state: "visible", timeout: 8000 }).catch(() => {});
  await apage.waitForTimeout(500);
  modalTxt = (await apage.getByTestId("data-share-modal").textContent().catch(() => "")) || "";
}
await apage.screenshot({ path: "oldshare-admin.png", fullPage: true });
console.log("modal has 'Овсянка':", modalTxt.includes("Овсянка"), "| has 'Итого':", modalTxt.includes("Итого"));
await b.close();
const ok = btn === 1 && !err && modalTxt.includes("Овсянка");
console.log(ok ? "\n✅ old-format share renders fine" : "\n❌ FAIL — new admin cannot render an OLD data_share");
process.exit(ok ? 0 : 1);
