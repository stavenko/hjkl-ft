//! # DEPRECATED
//!
//! This backend is no longer used. The app moved to an offline-first
//! architecture: all CRUD lives client-side in IndexedDB, and AI + sync go
//! through the Cloudflare workers. Kept for reference only — do not extend or
//! depend on it. It is intentionally out of sync with `api-types`.

pub mod api;
pub mod config;
pub mod providers;
pub mod use_cases;
