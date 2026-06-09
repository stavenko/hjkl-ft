# auth-worker integration tests

Integration tests for the auth-worker Cloudflare Worker, using Miniflare to run
the compiled WASM worker locally and vitest as the test runner.

## Prerequisites

The worker must be built before running tests. The build produces
`build/worker/shim.mjs` and associated `.wasm` files.

```
cd cloudflare/auth-worker
cargo install worker-build
worker-build --release
```

## Running tests

```
cd cloudflare/auth-worker/tests
npm install
npm test
```
