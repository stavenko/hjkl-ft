# CloudFlare Workers Architecture

## Workers

### 1. Auth Worker (`auth-worker`)
- PassKey (WebAuthn) registration & authentication
- Recovery key (password) for account recovery
- Device linking (add new PassKey from another device)
- Session token issuance (short-lived JWT/opaque tokens)
- Global Durable Object for user storage (credentials, recovery keys)
- NO user PII: no email, no phone

### 2. Billing Worker (`billing-worker`)
- Subscription management
- Transaction records from external acquirer
- Internal order tracking
- Subscription status checks (used by other workers)

### 3. Sync Worker (`sync-worker`)
- Version-based data sync between devices
- Requires active subscription
- Each frontend change = new version
- Write succeeds only if backend version matches
- On conflict: client pulls changes, merges, retries
- InProgress recipes NOT synced, only Completed

### 4. AI Token Worker (`ai-token-worker`)
- Issues one-time temporary tokens for AI requests
- Requires active subscription
- Token = single use, time-limited

### 5. AI Proxy Worker (`ai-proxy-worker`)
- Validates temporary token
- Proxies request to CloudFlare Workers AI
- Uses open-source models (Llama 3.1, Mistral, etc.)

## Auth Flow

```
Browser PassKey → Auth Worker (DO) → Session Token (JWT)
Session Token → Any Worker → Validates token, checks subscription
```

## Technology

- Rust + WASM via `workers-rs`
- `passkey-server` crate (wasm feature) for WebAuthn
- Durable Objects for persistent storage
- Miniflare for local testing (JS/TS test harness over WASM)
- Rust unit tests for business logic
