use worker::*;

/// Resolve a secret, preferring the Cloudflare Secrets Store binding (prod) and
/// falling back to a per-worker secret / [vars] value (dev/test). In dev there is
/// NO Store binding → `env.secret_store` errs → we fall back to the [vars] value,
/// so nothing dev-side changes. In prod the Store binding returns the global value.
/// Returns Err with a clear MISCONFIGURED message when the Store binding is
/// present but unresolvable, or when the secret is configured nowhere.
///
/// Copied verbatim from payment-worker/support-worker token.rs (only this helper is
/// needed here: telegram-worker generates no secrets and validates no JWTs).
pub async fn secret_or_var(env: &Env, name: &str) -> std::result::Result<String, String> {
    match env.secret_store(name) {
        Ok(store) => match store.get().await {
            Ok(Some(v)) if !v.is_empty() => Ok(v),
            Ok(_) => Err(format!(
                "MISCONFIGURED: Secrets Store binding '{name}' is empty/unset"
            )),
            Err(e) => Err(format!(
                "MISCONFIGURED: Secrets Store binding '{name}' get() failed: {e:?}"
            )),
        },
        Err(_) => env
            .secret(name)
            .map(|s| s.to_string())
            .ok()
            .or_else(|| env.var(name).map(|v| v.to_string()).ok())
            .ok_or_else(|| {
                format!("MISCONFIGURED: '{name}' not set (no Secrets Store binding and no var/secret)")
            }),
    }
}
