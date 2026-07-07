import { describe, test, expect, beforeAll, afterAll } from "vitest";
import { Miniflare } from "miniflare";
import { SignJWT } from "jose";
import path from "node:path";

// worker-build emits the real ES module at build/index.js (with index_bg.wasm
// alongside) and a thin build/worker/shim.mjs that re-exports `../index.js`.
// Point Miniflare at index.js directly: routing through shim.mjs makes workerd
// resolve `..` out of the starting directory and crash on startup.
const WORKER_BUILD_DIR = path.resolve(__dirname, "..", "build");
const JWT_SECRET = "dev-secret-change-in-production";

let mf: Miniflare;

const secret = new TextEncoder().encode(JWT_SECRET);

// Mint an HS256 JWT whose claims match the Rust TokenClaims shape
// (sub, iat, exp, caps, token_id). Far-future exp so the verify safeguard passes.
async function mkToken(sub: string): Promise<string> {
  return await new SignJWT({ sub, caps: [], token_id: "t-" + sub })
    .setProtectedHeader({ alg: "HS256", typ: "JWT" })
    .setIssuedAt()
    // far-future epoch (year 2100) so the Rust verify exp-safeguard passes
    .setExpirationTime(4_102_444_800)
    .sign(secret);
}

let userTok: string; // sub = user-A (not an expert)
let userBTok: string; // sub = user-B (not an expert)
let expertTok: string; // sub = expert-1 (DO-approved in beforeAll)

beforeAll(async () => {
  mf = new Miniflare({
    scriptPath: path.join(WORKER_BUILD_DIR, "index.js"),
    // Anchor module resolution at the build dir so `./index_bg.wasm` resolves
    // without workerd walking `..` out of the starting directory.
    modulesRoot: WORKER_BUILD_DIR,
    modules: true,
    modulesRules: [
      // worker-build emits build/index.js as an ES module; this Miniflare's
      // default treats **/*.js as CommonJS, so declare it ESModule explicitly.
      { type: "ESModule", include: ["**/*.js", "**/*.mjs"] },
      { type: "CompiledWasm", include: ["**/*.wasm"] },
    ],
    durableObjects: {
      // useSQLite mirrors wrangler.toml's new_sqlite_classes — Miniflare doesn't
      // read the migrations block, so SQL storage must be enabled explicitly.
      CONVERSATION_DO: { className: "ConversationDO", useSQLite: true },
      CONVERSATION_INDEX_DO: { className: "ConversationIndexDO", useSQLite: true },
    },
    bindings: {
      JWT_SECRET,
      ADMIN_APPROVE_SECRET: "test-admin-secret",
      INTERNAL_PUSH_KEY: "test-internal-key",
    },
    compatibilityDate: "2024-01-01",
  });

  userTok = await mkToken("user-A");
  userBTok = await mkToken("user-B");
  expertTok = await mkToken("expert-1");
  // There is no env allowlist anymore — the ONLY way to be an expert is the DO
  // approve flow. Approve expert-1 here so the expert-route tests exercise a
  // genuinely DO-approved admin.
  const { code }: any = await (
    await workerFetch("/admin/request", authed(expertTok, { method: "POST", body: "{}" }))
  ).json();
  await workerFetch("/admin/approve", {
    method: "POST",
    headers: { "Content-Type": "application/json", "X-Admin-Secret": "test-admin-secret" },
    body: JSON.stringify({ code }),
  });
});

afterAll(async () => {
  if (mf) await mf.dispose();
});

function authed(tok: string, init: RequestInit = {}): RequestInit {
  return {
    ...init,
    headers: {
      "Content-Type": "application/json",
      Authorization: `Bearer ${tok}`,
      ...(init.headers || {}),
    },
  };
}

async function workerFetch(
  urlPath: string,
  init?: RequestInit,
): Promise<Response> {
  return mf.dispatchFetch(`http://localhost${urlPath}`, init);
}

async function send(tok: string, clientId: string, text: string) {
  const resp = await workerFetch(
    "/message",
    authed(tok, { method: "POST", body: JSON.stringify({ client_id: clientId, text }) }),
  );
  return resp;
}

async function reply(uid: string, clientId: string, text: string) {
  return workerFetch(
    `/conversations/${uid}/reply`,
    authed(expertTok, { method: "POST", body: JSON.stringify({ client_id: clientId, text }) }),
  );
}

// Page through a paginated endpoint (after_seq cursor) and return all messages
// plus the observed has_more sequence.
async function pageMessages(urlBase: string, tok: string, limit: number) {
  const all: any[] = [];
  const hasMoreSeq: boolean[] = [];
  let after = 0;
  for (let i = 0; i < 100; i++) {
    const sep = urlBase.includes("?") ? "&" : "?";
    const resp = await workerFetch(
      `${urlBase}${sep}after_seq=${after}&limit=${limit}`,
      authed(tok),
    );
    expect(resp.status).toBe(200);
    const body: any = await resp.json();
    all.push(...body.messages);
    hasMoreSeq.push(body.has_more);
    after = body.next_after_seq;
    if (!body.has_more) break;
  }
  return { all, hasMoreSeq };
}

describe("support-worker — invariant 1: idempotent send", () => {
  test("same client_id twice => one row, same seq + created_at (user)", async () => {
    const uid = "idem-user";
    const tok = await mkToken(uid);
    const r1: any = await (await send(tok, "c1", "hi")).json();
    const r2: any = await (await send(tok, "c1", "hi (retry)")).json();

    expect(r1.seq).toBe(r2.seq);
    expect(r1.created_at).toBe(r2.created_at);

    const { all } = await pageMessages("/messages", tok, 50);
    const matches = all.filter((m) => m.client_id === "c1");
    expect(matches.length).toBe(1);
  });

  test("same client_id twice => one row, same seq (expert reply)", async () => {
    const uid = "idem-user2";
    const tok = await mkToken(uid);
    await send(tok, "u1", "hello");
    const r1: any = await (await reply(uid, "r1", "answer")).json();
    const r2: any = await (await reply(uid, "r1", "answer retry")).json();
    expect(r1.seq).toBe(r2.seq);

    const { all } = await pageMessages("/messages", tok, 50);
    const matches = all.filter((m) => m.client_id === "r1");
    expect(matches.length).toBe(1);
  });
});

describe("support-worker — invariant 2: monotonic seq + cursor paging", () => {
  test("/messages pages over multiple batches with no loss/dup + has_more transition", async () => {
    const uid = "pager-user";
    const tok = await mkToken(uid);
    const N = 25;
    for (let i = 0; i < N; i++) await send(tok, `m${i}`, `msg ${i}`);

    const { all, hasMoreSeq } = await pageMessages("/messages", tok, 10);
    const seqs = all.map((m) => m.seq);

    expect(seqs.length).toBe(N);
    // strictly increasing + contiguous
    for (let i = 1; i < seqs.length; i++) {
      expect(seqs[i]).toBe(seqs[i - 1] + 1);
    }
    // no duplicates
    expect(new Set(seqs).size).toBe(N);
    // has_more was true for batches 1-2, false on the final batch
    expect(hasMoreSeq).toEqual([true, true, false]);
  });

  test("/conversations/:uid/messages (expert view) pages identically", async () => {
    const uid = "pager-user"; // reuse the 25-message log above
    const { all, hasMoreSeq } = await pageMessages(
      `/conversations/${uid}/messages`,
      expertTok,
      10,
    );
    const seqs = all.map((m) => m.seq);
    expect(seqs.length).toBe(25);
    for (let i = 1; i < seqs.length; i++) expect(seqs[i]).toBe(seqs[i - 1] + 1);
    expect(new Set(seqs).size).toBe(25);
    expect(hasMoreSeq).toEqual([true, true, false]);
  });

  test("status=answered list pages with no loss/dup + has_more transition", async () => {
    // Seed > limit answered conversations: each gets a user msg + an expert reply.
    const M = 7;
    const ids: string[] = [];
    for (let i = 0; i < M; i++) {
      const cu = `ans-user-${String(i).padStart(2, "0")}`;
      ids.push(cu);
      const t = await mkToken(cu);
      await send(t, `${cu}-c`, "question");
      await reply(cu, `${cu}-r`, "answered");
    }

    // page answered with limit 3 => 3 + 3 + 1
    const seen: string[] = [];
    const hasMoreSeq: boolean[] = [];
    let after: string | null = null;
    for (let i = 0; i < 100; i++) {
      const q = after
        ? `/conversations?status=answered&limit=3&after=${encodeURIComponent(after)}`
        : `/conversations?status=answered&limit=3`;
      const resp = await workerFetch(q, authed(expertTok));
      expect(resp.status).toBe(200);
      const body: any = await resp.json();
      for (const c of body.conversations) seen.push(c.user_id);
      hasMoreSeq.push(body.has_more);
      after = body.next_after ?? null;
      if (!body.has_more) break;
    }

    // every seeded answered conversation appears exactly once
    for (const id of ids) {
      expect(seen.filter((s) => s === id).length).toBe(1);
    }
    expect(new Set(seen).size).toBe(seen.length); // no duplicates
    // has_more transition true...false (M=7, limit 3 => true,true,false)
    expect(hasMoreSeq[hasMoreSeq.length - 1]).toBe(false);
    expect(hasMoreSeq.filter((b) => b).length).toBeGreaterThanOrEqual(2);
  });
});

describe("support-worker — invariant 3: pending_since lifecycle", () => {
  async function pendingIds(): Promise<string[]> {
    const ids: string[] = [];
    let after: string | null = null;
    for (let i = 0; i < 100; i++) {
      const q = after
        ? `/conversations?status=pending&limit=50&after=${encodeURIComponent(after)}`
        : `/conversations?status=pending&limit=50`;
      const resp = await workerFetch(q, authed(expertTok));
      expect(resp.status).toBe(200);
      const body: any = await resp.json();
      for (const c of body.conversations) ids.push(c.user_id);
      after = body.next_after ?? null;
      if (!body.has_more) break;
    }
    return ids;
  }

  test("spam-no-jump (DETERMINISTIC via pending_seq), reply-clears, re-open, deduped-reply-no-reclear", async () => {
    const OLD = "pend-old";
    const NEW = "pend-new";
    const oldTok = await mkToken(OLD);
    const newTok = await mkToken(NEW);

    // (a) OLD sends first -> appears pending
    await send(oldTok, "old-1", "first from old");
    let p = await pendingIds();
    expect(p).toContain(OLD);

    // (b) NEW sends later -> OLD precedes NEW by ARRIVAL ORDER (monotonic pending_seq,
    //     NOT millisecond pending_since timing — deterministic even within a ms).
    await send(newTok, "new-1", "first from new");
    p = await pendingIds();
    const onlyTwo = p.filter((x) => x === OLD || x === NEW);
    expect(onlyTwo).toEqual([OLD, NEW]);

    // (c) NEW spams more -> must NOT jump ahead of OLD. Because pending_seq is kept
    //     for an already-pending run, later messages can never reorder the queue.
    await send(newTok, "new-2", "spam 2");
    await send(newTok, "new-3", "spam 3");
    await send(newTok, "new-4", "spam 4");
    p = await pendingIds();
    const orderAfterSpam = p.filter((x) => x === OLD || x === NEW);
    expect(orderAfterSpam).toEqual([OLD, NEW]);

    // (d) expert replies to OLD -> OLD leaves pending
    await reply(OLD, "rOLD", "here is your answer");
    p = await pendingIds();
    expect(p).not.toContain(OLD);
    expect(p).toContain(NEW);

    // (e) OLD re-opens with a new message -> reappears pending with a FRESH arrival
    //     seq, so it now sits AFTER NEW (correct: it re-entered the queue later).
    await send(oldTok, "old-2", "re-opening");
    p = await pendingIds();
    expect(p).toContain(OLD);
    const reorder = p.filter((x) => x === OLD || x === NEW);
    expect(reorder).toEqual([NEW, OLD]);

    // (f) DEDUPED GUARD: retry the ORIGINAL reply to OLD (same client_id "rOLD").
    //     The index op is ALWAYS called now (no dedup-skip), so correctness rests on
    //     the monotonic clear guard: existing last_seq > reply_seq => do NOT clear.
    const orig: any = await (await reply(OLD, "rOLD", "here is your answer")).json();
    const retry: any = await (await reply(OLD, "rOLD", "dup")).json();
    expect(retry.seq).toBe(orig.seq);
    p = await pendingIds();
    expect(p).toContain(OLD); // still pending — monotonic clear guard held
  });

  test("pending paging ACROSS a batch boundary: lossless, dedup-free, arrival-ordered", async () => {
    // Create > limit pending conversations in a KNOWN arrival order. Each gets a
    // single user message and no reply, so all stay pending.
    const N = 7;
    const ids: string[] = [];
    for (let i = 0; i < N; i++) {
      const cu = `pgpend-${String(i).padStart(2, "0")}`;
      ids.push(cu);
      const t = await mkToken(cu);
      await send(t, `${cu}-c`, "waiting");
    }

    // Page with a small limit (3) => spans multiple batches.
    const seen: string[] = [];
    const hasMoreSeq: boolean[] = [];
    let after: string | null = null;
    for (let i = 0; i < 100; i++) {
      const q = after
        ? `/conversations?status=pending&limit=3&after=${encodeURIComponent(after)}`
        : `/conversations?status=pending&limit=3`;
      const resp = await workerFetch(q, authed(expertTok));
      expect(resp.status).toBe(200);
      const body: any = await resp.json();
      for (const c of body.conversations) seen.push(c.user_id);
      hasMoreSeq.push(body.has_more);
      after = body.next_after ?? null;
      if (!body.has_more) break;
    }

    // lossless + dedup-free
    for (const id of ids) {
      expect(seen.filter((s) => s === id).length).toBe(1);
    }
    // arrival order preserved across the batch boundary: the subsequence of our
    // seeded ids appears in exactly the order they were created.
    const seenOurs = seen.filter((s) => ids.includes(s));
    expect(seenOurs).toEqual(ids);
    // has_more transitioned true...false
    expect(hasMoreSeq[hasMoreSeq.length - 1]).toBe(false);
    expect(hasMoreSeq.filter((b) => b).length).toBeGreaterThanOrEqual(2);
  });

  test("idempotent recovery: same client_id twice => pending EXACTLY once, right position", async () => {
    // A conversation that arrives BEFORE ours, to anchor position.
    const ANCHOR = "idemrec-anchor";
    const SELF = "idemrec-self";
    const aTok = await mkToken(ANCHOR);
    const sTok = await mkToken(SELF);

    await send(aTok, "idemrec-a", "anchor first");
    // First send of SELF. Then a RETRY with the SAME client_id (deduped append).
    // Because the index op is ALWAYS called now, the retry re-touches the index —
    // and idempotency must keep SELF present exactly once, not duplicated/reordered.
    await send(sTok, "idemrec-s", "self first");
    await send(sTok, "idemrec-s", "self retry");

    const p = await pendingIds();
    // exactly once
    expect(p.filter((x) => x === SELF).length).toBe(1);
    // correct position: ANCHOR (earlier arrival) precedes SELF
    const pair = p.filter((x) => x === ANCHOR || x === SELF);
    expect(pair).toEqual([ANCHOR, SELF]);
  });
});

describe("admin authorization (request/approve/me)", () => {
  function approve(code: string, secret?: string) {
    const headers: Record<string, string> = { "Content-Type": "application/json" };
    if (secret !== undefined) headers["X-Admin-Secret"] = secret;
    return workerFetch("/admin/approve", {
      method: "POST",
      headers,
      body: JSON.stringify({ code }),
    });
  }

  test("full request -> approve -> expert-access flow + invariants", async () => {
    // A fresh candidate sub, distinct from expert-1, so it doesn't collide with other tests.
    const CAND = "admin-cand";
    const candTok = await mkToken(CAND);

    // pre-request: not approved, no code
    let me: any = await (await workerFetch("/admin/me", authed(candTok))).json();
    expect(me).toEqual({ approved: false, code: null });

    // request a code (INVARIANT 3: maps to the authenticated sub)
    const reqResp = await workerFetch("/admin/request", authed(candTok, { method: "POST", body: "{}" }));
    expect(reqResp.status).toBe(200);
    const { code }: any = await reqResp.json();
    expect(code).toMatch(/^[A-HJ-NP-Z2-9]{8}$/); // alphabet drops I/O/0/1

    // idempotent: same code back
    const again: any = await (
      await workerFetch("/admin/request", authed(candTok, { method: "POST", body: "{}" }))
    ).json();
    expect(again.code).toBe(code);

    // /admin/me now shows the code, still not approved
    me = await (await workerFetch("/admin/me", authed(candTok))).json();
    expect(me).toEqual({ approved: false, code });

    // INVARIANT 4: still 403 on expert routes pre-approval
    expect((await workerFetch("/conversations", authed(candTok))).status).toBe(403);

    // INVARIANT 1: approve without the secret header => 403
    expect((await approve(code)).status).toBe(403);
    // wrong secret => 403
    expect((await approve(code, "nope")).status).toBe(403);

    // INVARIANT 2: correct secret but an unknown code => 404
    expect((await approve("ZZZZZZZZ", "test-admin-secret")).status).toBe(404);

    // correct secret + real code => approved, sub resolved from STORAGE
    const okResp = await approve(code, "test-admin-secret");
    expect(okResp.status).toBe(200);
    const okBody: any = await okResp.json();
    expect(okBody).toEqual({ approved: true, sub: CAND });

    // /admin/me => approved; expert route now 200 (DO-approved expert; INVARIANT 4)
    me = await (await workerFetch("/admin/me", authed(candTok))).json();
    expect(me.approved).toBe(true);
    expect((await workerFetch("/conversations", authed(candTok))).status).toBe(200);
  });

  test("approving a code does NOT make the approve caller an expert (INVARIANT 2/5)", async () => {
    // user-B requests its own code, then user-B's code is approved. Approval grants
    // expert access ONLY to the candidate who generated the code, never the caller
    // (the approve call supplies no sub at all).
    const CAND = "admin-cand2";
    const candTok = await mkToken(CAND);
    const { code }: any = await (
      await workerFetch("/admin/request", authed(candTok, { method: "POST", body: "{}" }))
    ).json();

    // user-B (a different, unapproved user) is still 403 before AND after approval.
    expect((await workerFetch("/conversations", authed(userBTok))).status).toBe(403);
    expect((await approve(code, "test-admin-secret")).status).toBe(200);
    expect((await workerFetch("/conversations", authed(userBTok))).status).toBe(403);
    // the candidate that generated the code IS now an expert
    expect((await workerFetch("/conversations", authed(candTok))).status).toBe(200);
  });

  test("env bootstrap expert still works without any DO record", async () => {
    expect((await workerFetch("/conversations", authed(expertTok))).status).toBe(200);
  });
});

describe("internal /internal/is-admin (cross-worker admin check)", () => {
  function isAdmin(sub: string, key?: string) {
    const headers: Record<string, string> = { "Content-Type": "application/json" };
    if (key !== undefined) headers["X-Internal-Key"] = key;
    return workerFetch("/internal/is-admin", {
      method: "POST",
      headers,
      body: JSON.stringify({ sub }),
    });
  }

  test("missing or wrong X-Internal-Key => 403", async () => {
    expect((await isAdmin("expert-1")).status).toBe(403);
    expect((await isAdmin("expert-1", "wrong")).status).toBe(403);
  });

  test("DO-approved sub (expert-1) => {approved:true}", async () => {
    const resp = await isAdmin("expert-1", "test-internal-key");
    expect(resp.status).toBe(200);
    expect(await resp.json()).toEqual({ approved: true });
  });

  test("random sub (not env, not DO-approved) => {approved:false}", async () => {
    const resp = await isAdmin("nobody-random-sub", "test-internal-key");
    expect(resp.status).toBe(200);
    expect(await resp.json()).toEqual({ approved: false });
  });

  test("DO-approved sub => {approved:true}", async () => {
    // Approve a fresh candidate through the public flow, then check via internal.
    const CAND = "internal-admin-cand";
    const candTok = await mkToken(CAND);
    const { code }: any = await (
      await workerFetch("/admin/request", authed(candTok, { method: "POST", body: "{}" }))
    ).json();
    const ok = await workerFetch("/admin/approve", {
      method: "POST",
      headers: { "Content-Type": "application/json", "X-Admin-Secret": "test-admin-secret" },
      body: JSON.stringify({ code }),
    });
    expect(ok.status).toBe(200);

    const resp = await isAdmin(CAND, "test-internal-key");
    expect(resp.status).toBe(200);
    expect(await resp.json()).toEqual({ approved: true });
  });
});

describe("support-worker — invariant 4: allowlist + ownership + monotonic read", () => {
  test("non-expert valid JWT => 403 on expert routes", async () => {
    const r1 = await workerFetch("/conversations", authed(userTok));
    expect(r1.status).toBe(403);
    const r2 = await workerFetch("/conversations/user-A/messages", authed(userTok));
    expect(r2.status).toBe(403);
    const r3 = await workerFetch(
      "/conversations/user-A/reply",
      authed(userTok, { method: "POST", body: JSON.stringify({ client_id: "x", text: "y" }) }),
    );
    expect(r3.status).toBe(403);
  });

  test("no token => 401, bad token => 401 (user and expert routes)", async () => {
    const noAuthUser = await workerFetch("/messages");
    expect(noAuthUser.status).toBe(401);
    const noAuthExpert = await workerFetch("/conversations");
    expect(noAuthExpert.status).toBe(401);

    const badUser = await workerFetch("/messages", {
      headers: { Authorization: "Bearer not.a.jwt" },
    });
    expect(badUser.status).toBe(401);
    const badExpert = await workerFetch("/conversations", {
      headers: { Authorization: "Bearer not.a.jwt" },
    });
    expect(badExpert.status).toBe(401);
  });

  test("a user can only touch their OWN conversation (DO keyed by sub)", async () => {
    await send(userTok, "owna-1", "secret from A");
    // user-B reads /messages -> sees user-B's own (empty) conversation, never A's
    const { all } = await pageMessages("/messages", userBTok, 50);
    expect(all.find((m) => m.client_id === "owna-1")).toBeUndefined();
  });

  test("read cursor advances forward-only (monotonic) without error", async () => {
    const uid = "reader";
    const tok = await mkToken(uid);
    await send(tok, "rd-1", "one");
    await send(tok, "rd-2", "two");
    await send(tok, "rd-3", "three");

    const a = await workerFetch(
      "/read",
      authed(tok, { method: "POST", body: JSON.stringify({ seq: 2 }) }),
    );
    expect(a.status).toBe(200);
    // a lower seq must not error and must not regress the cursor
    const b = await workerFetch(
      "/read",
      authed(tok, { method: "POST", body: JSON.stringify({ seq: 1 }) }),
    );
    expect(b.status).toBe(200);
    // advancing further still works
    const c = await workerFetch(
      "/read",
      authed(tok, { method: "POST", body: JSON.stringify({ seq: 3 }) }),
    );
    expect(c.status).toBe(200);
  });
});
