//! App-scoped error log for background work (the classify/enrich queue).
//!
//! Errors are collected into a ROOT signal (created once in `main`) so any widget
//! can observe whether there are errors and list them. In-memory only: a reload
//! clears them; the background sweep re-runs and re-reports if the problem persists.

use std::cell::RefCell;

use leptos::*;

/// One recorded error: what failed (`context`, e.g. "Нутриенты: Творог 5%") and the
/// raw message.
#[derive(Clone, PartialEq, Eq)]
pub struct AppError {
    pub context: String,
    pub message: String,
}

impl AppError {
    /// One-line copyable representation.
    pub fn as_text(&self) -> String {
        format!("{}\n{}", self.context, self.message)
    }
}

thread_local! {
    static ERRS: RefCell<Option<RwSignal<Vec<AppError>>>> = const { RefCell::new(None) };
}

/// Create the error-log signal at the ROOT scope. Call once from `main()`.
pub fn init() {
    ERRS.with(|c| {
        if c.borrow().is_none() {
            *c.borrow_mut() = Some(create_rw_signal(Vec::new()));
        }
    });
}

/// The reactive error list (observers re-render when it changes).
pub fn signal() -> RwSignal<Vec<AppError>> {
    ERRS.with(|c| c.borrow().expect("errors::init() must run before errors::signal()"))
}

/// Record an error (deduplicated by context+message; keeps the most recent 50).
pub fn record(context: &str, message: &str) {
    let e = AppError { context: context.to_string(), message: message.to_string() };
    // Guard: if init() hasn't run yet, drop silently rather than panic.
    let has = ERRS.with(|c| c.borrow().is_some());
    if !has {
        return;
    }
    signal().update(|v| {
        if v.iter().any(|x| *x == e) {
            return;
        }
        v.push(e);
        if v.len() > 50 {
            v.remove(0);
        }
    });
}

/// Clear all recorded errors.
pub fn clear() {
    if ERRS.with(|c| c.borrow().is_some()) {
        signal().update(|v| v.clear());
    }
}
