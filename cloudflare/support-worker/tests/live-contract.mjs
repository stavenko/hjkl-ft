import { SignJWT } from "jose";
const BASE = "https://support-worker.vg-stavenko.workers.dev";
const secret = new TextEncoder().encode("dev-secret-change-in-production");
const mk = (sub) => new SignJWT({ sub, caps: [], token_id: "t-" + sub })
  .setProtectedHeader({ alg: "HS256" }).setIssuedAt().setExpirationTime(4102444800).sign(secret);
const U = "live-u-" + Date.now();         // fresh user id → fresh conversation
let ok = true; const chk = (c, m) => { if (!c) { ok = false; console.log("  ✗", m); } else console.log("  ✓", m); };
const call = async (tok, path, init = {}) => {
  const r = await fetch(BASE + path, { ...init, headers: { Authorization: "Bearer " + tok, "Content-Type": "application/json", ...(init.headers||{}) } });
  let body = null; try { body = await r.json(); } catch {}
  return { status: r.status, body };
};
const user = await mk(U), expert = await mk("expert-1");

console.log("1) user send + idempotency");
const s1 = await call(user, "/message", { method: "POST", body: JSON.stringify({ client_id: "c1", text: "привет" }) });
chk(s1.status === 200 && s1.body.seq === 1, "first send seq=1 ("+s1.status+")");
const s1b = await call(user, "/message", { method: "POST", body: JSON.stringify({ client_id: "c1", text: "привет" }) });
chk(s1b.status === 200 && s1b.body.seq === 1, "same client_id => same seq (idempotent)");

console.log("2) cursor read");
const g0 = await call(user, "/messages?after_seq=0&limit=50");
chk(g0.body.messages.length === 1 && g0.body.messages[0].text === "привет", "user sees own message");

console.log("3) expert sees pending");
const p1 = await call(expert, "/conversations?status=pending&limit=200");
chk(p1.status === 200 && p1.body.conversations.some(c => c.user_id === U && c.pending_since), "conversation is in pending queue");

console.log("4) expert replies");
const rep = await call(expert, `/conversations/${U}/reply`, { method: "POST", body: JSON.stringify({ client_id: "r1", text: "отвечаю" }) });
chk(rep.status === 200 && rep.body.seq === 2, "reply seq=2 ("+rep.status+")");
const p2 = await call(expert, "/conversations?status=pending&limit=200");
chk(!p2.body.conversations.some(c => c.user_id === U), "conversation cleared from pending after reply");

console.log("5) user pulls reply via cursor");
const g1 = await call(user, "/messages?after_seq=1&limit=50");
chk(g1.body.messages.length === 1 && g1.body.messages[0].sender === "expert" && g1.body.messages[0].text === "отвечаю", "user gets expert reply via cursor");

console.log("6) allowlist");
const forbidden = await call(user, "/conversations?status=pending");
chk(forbidden.status === 403, "non-expert => 403 on /conversations ("+forbidden.status+")");
const noauth = await fetch(BASE + "/messages?after_seq=0").then(r => r.status);
chk(noauth === 401, "no token => 401 ("+noauth+")");

console.log(ok ? "\nLIVE CONTRACT: ALL PASS" : "\nLIVE CONTRACT: FAILURES");
process.exit(ok ? 0 : 1);
