//! "Sticky" resource values.
//!
//! A dashboard widget backed by `create_resource` is RE-CREATED every time its
//! page is mounted (i.e. on each navigation back to the dashboard). Its first
//! `.get()` is `None` (loading), so `unwrap_or_default()` yields an EMPTY value
//! and the widget paints its "no data / add first entry" placeholder — which then
//! snaps to the real data one frame later when the IndexedDB read resolves. That
//! flash is what the user sees switching panels.
//!
//! [`sticky`] keeps the last successfully-loaded value in a process-lifetime
//! cache (a module `thread_local`, which survives component unmount — WASM is
//! single-threaded, so it's effectively a global). It returns:
//!   - the fresh value when the resource has resolved (and updates the cache),
//!   - else the last-known value (instant, so the FIRST paint after navigation
//!     already has data — no flash),
//!   - else `None`, only before the very first successful load — callers render
//!     nothing (stay empty) until then, instead of a placeholder.

use std::cell::RefCell;
use std::thread::LocalKey;

/// Fold a resource's current `Option<T>` through a persistent last-value cache.
/// See the module docs. `None` is returned only until the first successful load.
pub fn sticky<T: Clone>(cache: &'static LocalKey<RefCell<Option<T>>>, current: Option<T>) -> Option<T> {
    match current {
        Some(v) => {
            cache.with(|c| *c.borrow_mut() = Some(v.clone()));
            Some(v)
        }
        None => cache.with(|c| c.borrow().clone()),
    }
}
