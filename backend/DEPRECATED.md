# DEPRECATED

This backend is **no longer used** and is kept for reference only.

The app moved to an **offline-first** architecture:

- All CRUD (foods, diary, recipes, goals) lives **client-side in IndexedDB**
  (see `frontend/src/services/local.rs`).
- **AI** (text lookup + label vision) and **sync** run on the **Cloudflare
  workers** (`cloudflare/ai-worker`, `cloudflare/main-flow`).

Do not extend this crate or depend on it. It is intentionally out of sync with
`common/api-types` and is not part of any build or deploy path.
