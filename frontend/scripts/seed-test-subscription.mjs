#!/usr/bin/env node
// Seed a test account with a VALID signed dev JWT and activate a FAKE paid
// subscription, so a Playwright / manual browser session can reach the AI
// worker — which 402-gates on an active subscription (see ai-worker gate).
//
// DEV ONLY. It relies on two dev-only facts:
//   1. every dev worker shares JWT_SECRET = "dev-secret-change-in-production",
//   2. payment-worker-dev exposes the TEST_ENTITLEMENT path `/test/guest-checkout`
//      (absent in prod, where `/test/*` returns 404 — so this cannot mint money).
//
// The fake-subscription mechanism itself lives in the workers: this script only
// drives it (mint a paid guest claim → bind it to the account). Nothing here is
// bundled into the app.
//
// Usage:
//   node scripts/seed-test-subscription.mjs [sub] [--payment <url>]
//     sub         account id / JWT `sub` (default "testuser")
//     --payment   payment-worker base URL (default payment-worker-dev)
//     JWT_SECRET  env override for the signing secret (default dev secret)
//
// Prints the signed JWT, the resulting subscription status, and a browser
// snippet to authorize the session (paste into DevTools or page.evaluate).

const args = process.argv.slice(2);
const sub = args.find((a) => !a.startsWith("--")) || "testuser";
const SECRET = process.env.JWT_SECRET || "dev-secret-change-in-production";
const base = (() => {
  const i = args.indexOf("--payment");
  return i >= 0 ? args[i + 1] : "https://payment-worker-dev.vg-stavenko.workers.dev";
})();

const b64url = (buf) => Buffer.from(buf).toString("base64url");

async function signJwt(payload) {
  const enc = new TextEncoder();
  const header = { alg: "HS256", typ: "JWT" };
  const data = b64url(JSON.stringify(header)) + "." + b64url(JSON.stringify(payload));
  const key = await crypto.subtle.importKey(
    "raw",
    enc.encode(SECRET),
    { name: "HMAC", hash: "SHA-256" },
    false,
    ["sign"],
  );
  const sig = await crypto.subtle.sign("HMAC", key, enc.encode(data));
  return data + "." + b64url(new Uint8Array(sig));
}

const nowSec = Math.floor(Date.now() / 1000);
// Far-future exp; the workers keep it only as a safeguard (see token.rs).
const token = await signJwt({ sub, iat: nowSec, exp: nowSec + 10 * 365 * 86400, caps: [], token_id: "tok1" });

// 1. mint a PAID guest claim (TEST_ENTITLEMENT — no real money, no lava).
const co = await fetch(base + "/test/guest-checkout", {
  method: "POST",
  headers: { "Content-Type": "application/json" },
  body: JSON.stringify({ planId: "test" }),
});
if (!co.ok) {
  console.error(`guest-checkout failed: HTTP ${co.status} ${await co.text()}`);
  process.exit(1);
}
const { claimId, secret } = await co.json();

// 2. bind the paid claim to THIS account (activates its SubscriptionDO).
const cl = await fetch(base + "/claim", {
  method: "POST",
  headers: { "Content-Type": "application/json", Authorization: "Bearer " + token },
  body: JSON.stringify({ claimId, secret }),
});
if (!cl.ok) {
  console.error(`claim failed: HTTP ${cl.status} ${await cl.text()}`);
  process.exit(1);
}
const status = await cl.json();

console.log("sub:         ", sub);
console.log("payment:     ", base);
console.log("subscription:", JSON.stringify(status));
console.log("token:       ", token);
console.log("\n// Browser: paste into DevTools / page.evaluate to authorize this session:");
console.log(`localStorage.setItem('user_id', ${JSON.stringify(sub)});`);
console.log(`localStorage.setItem('auth_token', ${JSON.stringify(token)});`);
console.log(`localStorage.setItem('token_id', 'tok1');`);
console.log(`localStorage.setItem('auth_ctx', 'browser');`);
