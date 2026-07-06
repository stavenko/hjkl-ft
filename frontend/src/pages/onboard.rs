use leptos::*;

use crate::pages::pwa_prompt::PwaPrompt;
use crate::services::auth;
use crate::services::i18n::t;

/// Read the `?u=<user_id>` query param (the non-secret universal account id the Mini App /
/// dynamic manifest carries into this page). None → not a valid entry.
fn param_user_id() -> Option<String> {
    let search = web_sys::window()?.location().search().ok()?;
    let params = web_sys::UrlSearchParams::new_with_str(&search).ok()?;
    params.get("u").filter(|s| !s.is_empty())
}

/// Read the one-time code from the URL FRAGMENT (`#code=…`) — the Mini App minted it (it's the
/// trusted Telegram owner) and embedded it here so we auto-authorize without asking the user to
/// copy it. Fragment (not query) so it isn't sent to the server. None → ask for the code.
fn param_code() -> Option<String> {
    let hash = web_sys::window()?.location().hash().ok()?;
    let rest = hash.strip_prefix("#code=")?;
    (!rest.is_empty()).then(|| rest.to_string())
}

/// Point the page's `<link rel="manifest">` at this user so ANY install captures a manifest
/// whose `start_url=/?u=<user_id>` — the installed PWA then launches knowing its account.
fn set_manifest_user(user_id: &str) {
    if let Some(doc) = web_sys::window().and_then(|w| w.document()) {
        if let Ok(Some(link)) = doc.query_selector("link[rel=manifest]") {
            let _ = link.set_attribute("href", &format!("/manifest.json?u={user_id}"));
        }
    }
}

/// Onboarding always arrives with the one-time code in the link (the Mini App minted it), so
/// we auto-authorize. Steps: Verifying → (capable devices) Passkey → InstallPwa (the app's own
/// `PwaPrompt`, rendered here as an onboarding step) → into the app "/". A bad/expired code →
/// Failed. No ask-for-a-code screen here — that lives in the installed PWA (no code in the URL).
#[derive(Clone, Copy, PartialEq)]
enum Step {
    Verifying,
    Failed,
    Passkey,
    InstallPwa,
}

/// Enter the app — the app itself shows the existing PWA-install prompt when appropriate.
fn go_app() {
    if let Some(w) = web_sys::window() {
        let _ = w.location().set_href("/");
    }
}

/// Drop the one-time `#code=` fragment from the address bar after a successful verify, so a
/// forced reload (index.html reloads on SW activation and on `appinstalled`) can't try to
/// re-verify the already-consumed code. Keeps the `?u=` query.
fn strip_code_from_url() {
    if let Some(win) = web_sys::window() {
        if let Ok(history) = win.history() {
            let loc = win.location();
            let path = loc.pathname().unwrap_or_else(|_| "/onboard".to_string());
            let search = loc.search().unwrap_or_default();
            let new_url = format!("{path}{search}");
            let _ = history.replace_state_with_url(&wasm_bindgen::JsValue::NULL, "", Some(&new_url));
        }
    }
}

#[component]
pub fn OnboardPage() -> impl IntoView {
    let user_id = param_user_id();
    if user_id.is_none() {
        if let Some(w) = web_sys::window() {
            let _ = w.location().set_href("/");
        }
    }
    let user_id = store_value(user_id.unwrap_or_default());
    set_manifest_user(&user_id.get_value());

    // The one-time code lives in the URL, and index.html force-reloads this page on SW
    // activation and on PWA install — each reload would otherwise re-verify an ALREADY-CONSUMED
    // code and falsely show «stale». A code proves auth exactly ONCE; after that the session
    // (get_user_id) is the source of truth. So: already signed in → we're in, never «stale».
    let authed_at_mount = auth::get_user_id().is_some();

    let step = create_rw_signal(if authed_at_mount || param_code().is_some() {
        Step::Verifying
    } else {
        Step::Failed
    });

    let can_passkey = create_rw_signal(false);
    // Detect passkey capability, then decide. Onboarding NEVER auto-redirects into the app — it
    // always lands on «create key» (capable devices) or the PWA-install screen; the user leaves
    // ONLY via that screen. Already signed in (a prior load verified the code, then index.html
    // force-reloaded on SW-activation / appinstalled) → continue the flow, DON'T re-verify the
    // consumed code, DON'T show «stale». A verify failure with NO session → genuinely stale link.
    create_effect(move |_| {
        spawn_local(async move {
            let cp = !auth::passkey_unavailable().await;
            can_passkey.set(cp);
            let onboarding_step = move || step.set(if cp { Step::Passkey } else { Step::InstallPwa });

            // Reloaded / already signed in → continue onboarding, no re-verify.
            if authed_at_mount {
                onboarding_step();
                return;
            }

            if let Some(code) = param_code() {
                match auth::code_verify(&user_id.get_value(), &code).await {
                    Ok(()) => {
                        strip_code_from_url();
                        onboarding_step();
                    }
                    // A concurrent load may have consumed the code and signed us in first — if
                    // we're signed in now, that's success, not a stale link.
                    Err(_) => {
                        if auth::get_user_id().is_some() {
                            onboarding_step();
                        } else {
                            step.set(Step::Failed);
                        }
                    }
                }
            } else if auth::get_user_id().is_some() {
                onboarding_step();
            } else {
                step.set(Step::Failed);
            }
        });
    });

    // «Create key» screen state (the original register screen, restored).
    let display_name = create_rw_signal(String::new());
    let loading = create_rw_signal(false);
    let error = create_rw_signal(None::<String>);
    let on_register = move |_| {
        let name = display_name.get_untracked();
        if name.trim().is_empty() {
            return;
        }
        loading.set(true);
        error.set(None);
        spawn_local(async move {
            match auth::add_passkey_named(name.trim()).await {
                Ok(()) => step.set(Step::InstallPwa),
                Err(e) => {
                    error.set(Some(e));
                    loading.set(false);
                }
            }
        });
    };
    let on_login = move |_| {
        loading.set(true);
        error.set(None);
        spawn_local(async move {
            match auth::authenticate().await {
                Ok(_) => step.set(Step::InstallPwa),
                Err(e) => {
                    error.set(Some(e));
                    loading.set(false);
                }
            }
        });
    };
    let error_view = move || {
        error.get().map(|e| view! {
            <div class="notification is-danger is-light mb-4" style="text-align: left;">{e}</div>
        })
    };

    move || match step.get() {
        // ── Screen 3 (its own full screen): install the PWA — the app's own PwaPrompt ──
        Step::InstallPwa => view! {
            <PwaPrompt on_dismiss=Callback::new(|_| go_app()) />
        }.into_view(),

        // ── Verifying / Failed / Passkey share the onboarding chrome ──
        _ => view! {
        <div style="min-height: 100vh; display: flex; flex-direction: column; align-items: center; justify-content: center; padding: 2rem; text-align: center; background: var(--bulma-scheme-main);">
            <div style="max-width: 24rem; width: 100%;">
                <img src="/icon-192.png" alt="re:Norma" style="width: 80px; height: 80px; border-radius: 16px; margin-bottom: 1rem;" />

                {move || match step.get() {
                    // ── Auto-authorizing with the code from the link ──
                    Step::Verifying => view! {
                        <div attr:data-testid="onboard-verifying" style="display: flex; flex-direction: column; align-items: center; gap: 18px;">
                            <div class="ft-spinner"></div>
                            <p class="is-size-6 has-text-grey">"Входим…"</p>
                        </div>
                    }.into_view(),

                    // ── Stale/failed link — send them back to the bot ──
                    Step::Failed => view! {
                        <div attr:data-testid="onboard-failed">
                            <h1 class="title is-4" style="margin-bottom: 0.5rem;">"Ссылка устарела"</h1>
                            <p class="has-text-grey mb-4" style="line-height: 1.6;">"Код входа не подошёл или истёк. Откройте нашего Telegram-бота и снова нажмите «Получить доступ»."</p>
                        </div>
                    }.into_view(),

                    // ── Screen 2 (its own screen): create the passkey — original screen ──
                    Step::Passkey => view! {
                        <div>
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

                            <button
                                attr:data-testid="onboard-btn-login"
                                class="button is-ghost has-text-grey is-fullwidth mt-4"
                                style="text-decoration: underline;"
                                disabled=move || loading.get()
                                on:click=on_login
                            >
                                {move || t("onboard.have_account")}
                            </button>
                        </div>
                    }.into_view(),

                    // InstallPwa is handled by the outer match (its own full screen).
                    Step::InstallPwa => view! { <div></div> }.into_view(),
                }}
            </div>
        </div>
        }.into_view(),
    }
}
