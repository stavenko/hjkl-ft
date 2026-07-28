// E2E: approved expert sets a client's planka via support-worker, which delegates
// to sync-worker; verify it landed in the client's synced goal. Also: a non-expert
// is rejected.
const SUP = process.argv[2] || "https://support-worker-dev.vg-stavenko.workers.dev";
const SYNC = process.argv[3] || "https://sync-worker-dev.vg-stavenko.workers.dev";
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
const stamp = Math.floor(Date.now() / 1000);
const expertSub = "expert-verify-" + stamp;
const targetSub = "client-verify-" + stamp;

const expertTok = await jwt(expertSub);
const targetTok = await jwt(targetSub);

// 1) expert requests an access code, operator approves it (adds sub to admins)
let r = await fetch(`${SUP}/admin/request`, { method: "POST", headers: { Authorization: `Bearer ${expertTok}`, "Content-Type": "application/json" }, body: "{}" });
const { code } = await r.json();
console.log("1) request code:", r.status, code);
r = await fetch(`${SUP}/admin/approve`, { method: "POST", headers: { "X-Admin-Secret": APPROVE, "Content-Type": "application/json" }, body: JSON.stringify({ code }) });
console.log("2) approve     :", r.status, await r.text());

// 3) a NON-expert must be rejected
r = await fetch(`${SUP}/conversations/${targetSub}/set-planka`, { method: "POST", headers: { Authorization: `Bearer ${targetTok}`, "Content-Type": "application/json" }, body: JSON.stringify({ amount: 9999 }) });
console.log("3) non-expert  :", r.status, "(expect 401/403)");
const rejected = r.status === 401 || r.status === 403;

// 4) expert sets the client's planka to 2450
r = await fetch(`${SUP}/conversations/${targetSub}/set-planka`, { method: "POST", headers: { Authorization: `Bearer ${expertTok}`, "Content-Type": "application/json" }, body: JSON.stringify({ amount: 2450 }) });
console.log("4) expert set  :", r.status, await r.text());

// 5) verify via sync-worker dump (as the client)
r = await fetch(`${SYNC}/sync/dump`, { method: "POST", headers: { Authorization: `Bearer ${targetTok}`, "Content-Type": "application/json" }, body: "{}" });
const dump = await r.json();
const cal = (dump.goals || []).find((g) => g.nutrient === "Calories" && g.direction === "AtMost");
console.log("5) client dump :", r.status, "Calories goal =", JSON.stringify(cal));

// 6) expert reads it back via support-worker
r = await fetch(`${SUP}/conversations/${targetSub}/planka`, { headers: { Authorization: `Bearer ${expertTok}` } });
console.log("6) expert get  :", r.status, await r.text());

const ok = rejected && cal && cal.amount === 2450;
console.log(ok ? "\n✅ PASS — expert→support→sync sets the client planka; non-expert blocked" : "\n❌ FAIL");
process.exit(ok ? 0 : 1);
