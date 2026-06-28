# support-worker integration tests

Integration tests for the support-worker Cloudflare Worker (human support chat,
Phase 1), using Miniflare to run the compiled WASM worker locally and vitest as
the test runner. They are HTTP-level and prove invariants 1–4 from the spec.

## Prerequisites

The worker must be built before running tests. The build produces
`build/worker/shim.mjs` and the associated `build/index_bg.wasm`.

```
cd cloudflare/support-worker
cargo install worker-build --version 0.8.5
worker-build --release
```

The Miniflare instance binds:

- two Durable Objects — `CONVERSATION_DO` (per-user log) and
  `CONVERSATION_INDEX_DO` (global expert queue);
- `JWT_SECRET` (HS256 secret, matches the dev value in `wrangler.toml`);
- `EXPERT_IDS` — comma-separated allowlist of user_ids permitted on the
  `/conversations*` endpoints (`expert-1,expert-2` in tests).

Tokens are minted in-test with `jose` (HS256) so the authed paths can be driven.

## Running tests

```
cd cloudflare/support-worker/tests
npm install
npm test
```
