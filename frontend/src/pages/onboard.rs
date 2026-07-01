use leptos::*;

use crate::services::auth;
use crate::services::i18n::t;
use crate::services::subscription;

/// The flow this page drives. Registration happens ONLY here (a new user only
/// ever arrives via a paid landing link with a `#claim=` fragment).
#[derive(Clone, PartialEq)]
enum Step {
    /// Register first: name + passkey.
    Register,
    /// Binding the paid subscription to the just-created account.
    Claiming,
    /// The webhook hasn't confirmed payment yet — auto-retry + manual retry.
    Pending,
    /// Terminal failure (claimed by another account, bad/void secret, …).
    Failed,
    /// Subscription bound — entering the app.
    Done,
}

/// Parse `#claim=claimId.secret` from the current URL fragment.
/// The secret travels ONLY in the fragment (never a query param), so it is not
/// sent to any server in the redirect and not written to access logs.
fn parse_claim_fragment() -> Option<(String, String)> {
    let hash = web_sys::window()?.location().hash().ok()?;
    // hash includes the leading '#'
    let rest = hash.strip_prefix("#claim=")?;
    let (claim_id, secret) = rest.split_once('.')?;
    if claim_id.is_empty() || secret.is_empty() {
        return None;
    }
    Some((claim_id.to_string(), secret.to_string()))
}

/// Strip the `#claim=...` secret from the URL bar once it's been consumed, so it
/// isn't left visible. Best-effort (history API).
fn clear_claim_fragment() {
    if let Some(history) = web_sys::window().and_then(|w| w.history().ok()) {
        let _ = history.replace_state_with_url(&wasm_bindgen::JsValue::NULL, "", Some("/onboard"));
    }
}

/// A claim error is retryable ONLY while the webhook hasn't confirmed payment
/// (`not_paid_yet`, which the server returns as HTTP 409). Everything else is
/// terminal — including `claim_void` (also HTTP 409, set after a manual refund),
/// `bad_secret`/`claimed_by_other` (403) and `claim_not_found` (404). Matching the
/// error CODE (not the bare HTTP 409 status) keeps a voided claim from looping
/// forever in the pending/retry state.
fn is_pending_err(e: &str) -> bool {
    e.contains("not_paid_yet")
}

#[component]
pub fn OnboardPage() -> impl IntoView {
    let claim = parse_claim_fragment();

    // No `#claim=` fragment → this isn't a paid entry. Send them to the login
    // dialog ("/"); registration lives only here, behind a paid claim.
    if claim.is_none() {
        if let Some(w) = web_sys::window() {
            let _ = w.location().set_href("/");
        }
    }

    let (claim_id, secret) = claim.unwrap_or_default();
    let claim_id = store_value(claim_id);
    let secret = store_value(secret);

    let step = create_rw_signal(Step::Register);
    let display_name = create_rw_signal(String::new());
    let loading = create_rw_signal(false);
    let error = create_rw_signal(None::<String>);

    // Attempt the claim (used after register and on each retry). Idempotent
    // server-side, so re-running after a half-done claim is safe.
    let do_claim = move || {
        step.set(Step::Claiming);
        error.set(None);
        spawn_local(async move {
            let cid = claim_id.get_value();
            let sec = secret.get_value();
            match subscription::claim(&cid, &sec).await {
                Ok(s) if s.is_paid() => {
                    clear_claim_fragment();
                    step.set(Step::Done);
                    // Enter the app.
                    if let Some(w) = web_sys::window() {
                        let _ = w.location().set_href("/");
                    }
                }
                Ok(_) => {
                    // Claimed but not active yet — treat as pending and retry.
                    step.set(Step::Pending);
                }
                Err(e) if is_pending_err(&e) => {
                    step.set(Step::Pending);
                }
                Err(e) => {
                    error.set(Some(e));
                    step.set(Step::Failed);
                }
            }
        });
    };

    // Register name + passkey, then claim.
    let on_register = move |_| {
        let name = display_name.get_untracked();
        if name.trim().is_empty() {
            return;
        }
        loading.set(true);
        error.set(None);
        spawn_local(async move {
            // F-1 pre-check: do NOT register if the claim can't be bound. Register runs
            // BEFORE claim (claim binds to a user_id), so a terminal claim state would
            // otherwise leave an orphan account with no subscription. We check the public
            // claim status first; terminal states (`claimed`/`void`/`none`) → stop here,
            // no account created. `paid`/`pending` are fine (pending → claim retries).
            // A transient network error on the check → proceed (claim() surfaces the real
            // error); a rare race after this check is accepted (no destructive rollback).
            let cid = claim_id.get_value();
            if let Ok(st) = subscription::claim_status(&cid).await {
                if st != "paid" && st != "pending" {
                    error.set(Some(t("onboard.link_unavailable").to_string()));
                    loading.set(false);
                    return;
                }
            }
            match auth::register(&name).await {
                Ok(_) => {
                    loading.set(false);
                    do_claim();
                }
                Err(e) => {
                    error.set(Some(e));
                    loading.set(false);
                }
            }
        });
    };

    // Auto-retry the claim while in Pending: poll every 2s for up to ~60s. The
    // loop self-cancels once the step leaves Pending (success / terminal error).
    create_effect(move |_| {
        if step.get() != Step::Pending {
            return;
        }
        spawn_local(async move {
            for _ in 0..30 {
                sleep_ms(2000).await;
                if step.get_untracked() != Step::Pending {
                    return;
                }
                let cid = claim_id.get_value();
                let sec = secret.get_value();
                match subscription::claim(&cid, &sec).await {
                    Ok(s) if s.is_paid() => {
                        clear_claim_fragment();
                        step.set(Step::Done);
                        if let Some(w) = web_sys::window() {
                            let _ = w.location().set_href("/");
                        }
                        return;
                    }
                    Ok(_) => { /* still not active — keep polling */ }
                    Err(e) if is_pending_err(&e) => { /* keep polling */ }
                    Err(e) => {
                        error.set(Some(e));
                        step.set(Step::Failed);
                        return;
                    }
                }
            }
        });
    });

    let error_view = move || {
        error.get().map(|e| view! {
            <div class="notification is-danger is-light mb-4" style="text-align: left;">
                {e}
                <div class="is-size-7 mt-2 has-text-grey-dark">
                    "Внутри Telegram регистрация не работает — откройте эту страницу в Safari или Chrome."
                </div>
            </div>
        })
    };

    view! {
        <div style="min-height: 100vh; display: flex; flex-direction: column; align-items: center; justify-content: center; padding: 2rem; text-align: center; background: var(--bulma-scheme-main);">
            <div style="max-width: 24rem; width: 100%;">
                {move || match step.get() {
                    Step::Register => view! {
                        <div>
                            <img src="/icon-192.png" alt="re:Norma" style="width: 80px; height: 80px; border-radius: 16px; margin-bottom: 1rem;" />
                            <h1 class="title is-4" style="margin-bottom: 0.5rem;">{move || t("onboard.title")}</h1>
                            <p class="has-text-grey mb-5" style="line-height: 1.6;">{move || t("onboard.subtitle")}</p>

                            {error_view}

                            <div style="display: flex; flex-direction: column; gap: 1rem;">
                                <input
                                    attr:data-testid="onboard-input-name"
                                    class="input is-medium"
                                    type="text"
                                    placeholder=t("auth.name_placeholder")
                                    prop:value=move || display_name.get()
                                    on:input=move |ev| display_name.set(event_target_value(&ev))
                                />
                                <button
                                    attr:data-testid="onboard-btn-register"
                                    class="button is-link is-medium is-fullwidth has-text-weight-semibold"
                                    disabled=move || loading.get() || display_name.get().trim().is_empty()
                                    on:click=on_register
                                >
                                    {move || if loading.get() { t("auth.creating") } else { t("auth.create_account") }}
                                </button>
                            </div>
                        </div>
                    }.into_view(),

                    Step::Claiming => view! {
                        <div attr:data-testid="onboard-claiming" style="display: flex; flex-direction: column; align-items: center; gap: 20px;">
                            <div class="ft-spinner"></div>
                            <p class="is-size-6 has-text-grey">{move || t("onboard.claiming")}</p>
                        </div>
                    }.into_view(),

                    Step::Pending => view! {
                        <div attr:data-testid="onboard-claiming" style="display: flex; flex-direction: column; align-items: center; gap: 20px;">
                            <div class="ft-spinner"></div>
                            <p class="is-size-5 has-text-weight-semibold">{move || t("onboard.pending_title")}</p>
                            <p class="is-size-6 has-text-grey">{move || t("onboard.pending_body")}</p>
                            <button
                                attr:data-testid="onboard-btn-retry"
                                class="button is-link is-light"
                                on:click=move |_| do_claim()
                            >
                                {move || t("onboard.retry")}
                            </button>
                        </div>
                    }.into_view(),

                    Step::Failed => view! {
                        <div attr:data-testid="onboard-error">
                            <div style="font-size: 56px; margin-bottom: 16px;">"\u{26a0}\u{fe0f}"</div>
                            <h1 class="title is-5" style="margin-bottom: 0.5rem;">{move || t("onboard.error_title")}</h1>
                            <p class="has-text-grey mb-2" style="line-height: 1.6;">{move || t("onboard.error_body")}</p>
                            {move || error.get().map(|e| view! {
                                <p class="is-size-7 has-text-danger mb-4">{e}</p>
                            })}
                            <button
                                attr:data-testid="onboard-btn-retry"
                                class="button is-light is-fullwidth mb-3"
                                on:click=move |_| do_claim()
                            >
                                {move || t("onboard.retry")}
                            </button>
                            <button
                                class="button is-ghost has-text-grey is-fullwidth"
                                style="text-decoration: underline;"
                                on:click=move |_| {
                                    if let Some(w) = web_sys::window() { let _ = w.location().set_href("/"); }
                                }
                            >
                                {move || t("auth.login_title")}
                            </button>
                        </div>
                    }.into_view(),

                    Step::Done => view! {
                        <div attr:data-testid="onboard-success" style="display: flex; flex-direction: column; align-items: center; gap: 20px;">
                            <div class="ft-spinner"></div>
                            <p class="is-size-6 has-text-grey">{move || t("onboard.success")}</p>
                        </div>
                    }.into_view(),
                }}
            </div>
        </div>
    }
}

async fn sleep_ms(ms: u32) {
    let promise = js_sys::Promise::new(&mut |resolve, _| {
        let window = web_sys::window().expect("no window");
        let _ = window.set_timeout_with_callback_and_timeout_and_arguments_0(&resolve, ms as i32);
    });
    let _ = wasm_bindgen_futures::JsFuture::from(promise).await;
}
