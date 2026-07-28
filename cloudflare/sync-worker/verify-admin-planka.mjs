// E2E check of the admin planka mechanism against a deployed sync-worker.
// 1) as the USER (signed dev JWT): push a Calories goal (2600) → establishes SyncDO
// 2) as ADMIN (X-Admin-Secret): set-calorie-planka 3000
// 3) as the USER: /sync/dump → the goal must now read 3000 (client would adopt this)
// 4) as ADMIN: GET /admin/calorie-planka → 3000
const BASE = process.argv[2] || "https://sync-worker-dev.vg-stavenko.workers.dev";
const SECRET = process.env.JWT_SECRET || "dev-secret-change-in-production";
const ADMIN = process.env.ADMIN_SYNC_SECRET || "dev-admin-sync-secret";
const sub = "planka-verify-" + Math.floor(Date.now() / 1000);

const b64url = (buf) => Buffer.from(buf).toString("base64").replace(/=/g, "").replace(/\+/g, "-").replace(/\//g, "_");
async function signJwt(payload) {
  const enc = new TextEncoder();
  const header = { alg: "HS256", typ: "JWT" };
  const data = `${b64url(JSON.stringify(header))}.${b64url(JSON.stringify(payload))}`;
  const key = await crypto.subtle.importKey("raw", enc.encode(SECRET), { name: "HMAC", hash: "SHA-256" }, false, ["sign"]);
  const sig = await crypto.subtle.sign("HMAC", key, enc.encode(data));
  return `${data}.${b64url(new Uint8Array(sig))}`;
}

const now = Math.floor(Date.now() / 1000);
const token = await signJwt({ sub, iat: now, exp: now + 3600, caps: [], token_id: "t" });
const authHdr = { Authorization: `Bearer ${token}`, "Content-Type": "application/json" };
const iso = new Date().toISOString();

// 1) user pushes a starting planka of 2600
const goal = {
  id: "cal-goal-1", nutrient: "Calories", key: "calories",
  direction: "AtMost", amount: 2600, unit: "Kcal", period: "Day",
  created_at: iso, updated_at: iso,
};
let r = await fetch(`${BASE}/sync/push`, { method: "POST", headers: authHdr, body: JSON.stringify({ goals: [goal] }) });
console.log("1) user push  :", r.status, await r.text());

// 2) admin sets 3000
r = await fetch(`${BASE}/admin/set-calorie-planka`, {
  method: "POST",
  headers: { "X-Admin-Secret": ADMIN, "Content-Type": "application/json" },
  body: JSON.stringify({ user_id: sub, amount: 3000 }),
});
console.log("2) admin set  :", r.status, await r.text());

// 3) user dumps → planka must be 3000
r = await fetch(`${BASE}/sync/dump`, { method: "POST", headers: authHdr, body: "{}" });
const dump = await r.json();
const cal = (dump.goals || []).find((g) => g.nutrient === "Calories" && g.direction === "AtMost");
console.log("3) user dump  :", r.status, "Calories goal =", JSON.stringify(cal));

// 4) admin reads back
r = await fetch(`${BASE}/admin/calorie-planka?user_id=${encodeURIComponent(sub)}`, { headers: { "X-Admin-Secret": ADMIN } });
console.log("4) admin get  :", r.status, await r.text());

// 5) unauthorized admin call must be 401
r = await fetch(`${BASE}/admin/calorie-planka?user_id=${encodeURIComponent(sub)}`, { headers: { "X-Admin-Secret": "wrong" } });
console.log("5) bad secret :", r.status, "(expect 401)");

const ok = cal && cal.amount === 3000;
console.log(ok ? "\n✅ PASS — admin set propagated to the user's synced goal" : "\n❌ FAIL");
process.exit(ok ? 0 : 1);
