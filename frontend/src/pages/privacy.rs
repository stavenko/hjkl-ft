use leptos::*;
use leptos_router::*;

use crate::services::{auth, i18n::t};
use crate::pages::pair_page::PairPageLoggedIn;

fn format_timestamp(ms: i64) -> String {
    let date = js_sys::Date::new(&wasm_bindgen::JsValue::from_f64(ms as f64));
    let d = date.get_date();
    let m = date.get_month() + 1;
    let y = date.get_full_year();
    let h = date.get_hours();
    let min = date.get_minutes();
    format!("{d:02}.{m:02}.{y} {h:02}:{min:02}")
}

#[component]
pub fn PrivacyPage() -> impl IntoView {
    let show_pair = create_rw_signal(false);
    let navigate = use_navigate();

    // Enroll a passkey on THIS device (natural next step after a backup-phrase login).
    let passkey_busy = create_rw_signal(false);
    let passkey_done = create_rw_signal(false);
    let passkey_err = create_rw_signal(None::<String>);
    let on_add_passkey = move |_| {
        passkey_busy.set(true);
        passkey_err.set(None);
        spawn_local(async move {
            match auth::add_passkey().await {
                Ok(()) => passkey_done.set(true),
                Err(e) => passkey_err.set(Some(e)),
            }
            passkey_busy.set(false);
        });
    };

    view! {
        {move || if show_pair.get() {
            view! {
                <PairPageLoggedIn on_close=Callback::new(move |_| show_pair.set(false)) />
            }.into_view()
        } else {
            view! {
                <div style="background: var(--bulma-background); min-height: 100vh; padding: 0; margin: -0.75rem;">

                    // Nav bar
                    <div style="display: flex; align-items: center; padding: 12px 16px; background: var(--bulma-background);">
                        <button
                            attr:data-testid="privacy-btn-back"
                            class="has-text-link is-size-5"
                            style="background: none; border: none; cursor: pointer; padding: 0; display: flex; align-items: center; gap: 4px;"
                            on:click={
                                let nav = navigate.clone();
                                move |_| {
                                    let nav = nav.clone();
                                    nav("/settings", Default::default());
                                }
                            }
                        >
                            {move || t("privacy.back")}
                        </button>
                        <h1 class="is-size-5 has-text-weight-semibold" style="margin: 0 auto;">{move || t("privacy.title")}</h1>
                        // Invisible spacer for centering
                        <span class="is-size-5" style="visibility: hidden;">{move || t("privacy.back")}</span>
                    </div>

                    // Sessions section
                    <div style="padding: 0 16px;">
                        <p
                            attr:data-testid="privacy-sessions-header"
                            class="is-size-7 has-text-grey-light"
                            style="text-transform: uppercase; letter-spacing: 0.02em; padding: 24px 0 8px 0; margin: 0;"
                        >
                            {move || t("privacy.sessions")}
                        </p>
                        <div style="background: var(--bulma-scheme-main); border-radius: 12px; overflow: hidden;">
                        {
                            let tokens_res = create_resource(
                                || (),
                                |_| async { auth::fetch_tokens().await },
                            );
                            let my_fp = auth::current_fingerprint();
                            let my_token_id = auth::current_token_id();
                            move || {
                                let my_fp = my_fp.clone();
                                let my_token_id = my_token_id.clone();
                                tokens_res.get().map(|result| {
                                    match result {
                                    Err(e) => {
                                        view! {
                                            <div style="padding: 12px 16px;">
                                                <p class="is-size-6 has-text-danger">{e}</p>
                                            </div>
                                        }.into_view()
                                    }
                                    Ok(tokens) if tokens.is_empty() => {
                                        view! {
                                            <div style="padding: 12px 16px;">
                                                <p class="is-size-6 has-text-grey-light">"—"</p>
                                            </div>
                                        }.into_view()
                                    }
                                    Ok(tokens) => {
                                        let len = tokens.len();
                                        tokens.into_iter().enumerate().map(|(i, tok)| {
                                            let fp = tok.get("fingerprint").and_then(|v| v.as_str()).unwrap_or("").to_string();
                                            let token_id = tok.get("token_id").and_then(|v| v.as_str()).unwrap_or("").to_string();
                                            let created = tok.get("created_at")
                                                .and_then(|v| v.as_i64())
                                                .map(|ts| format_timestamp(ts))
                                                .unwrap_or_default();
                                            let is_current = if let Some(ref my_tid) = my_token_id {
                                                !my_tid.is_empty() && token_id == *my_tid
                                            } else {
                                                fp == my_fp
                                            };
                                            let short_fp = if fp.len() > 8 { &fp[..8] } else { &fp };
                                            let short_fp = short_fp.to_string();
                                            let border = if i < len - 1 {
                                                "border-bottom: 0.5px solid var(--bulma-border-weak); margin-left: 16px;"
                                            } else { "" };
                                            view! {
                                                <div
                                                    attr:data-testid="privacy-session-item"
                                                    style=format!("padding: 12px 16px; {border}")
                                                >
                                                    <div style="display: flex; align-items: center; gap: 8px;">
                                                        <span class="is-size-6" style="font-weight: 400;">{short_fp.clone()}</span>
                                                        {if is_current {
                                                            view! {
                                                                <span
                                                                    attr:data-testid="privacy-session-current"
                                                                    style="color: var(--bulma-success); background: var(--bulma-success-light); padding: 1px 8px; border-radius: 10px;"
                                                                    class="is-size-7"
                                                                >
                                                                    {move || t("privacy.this_device")}
                                                                </span>
                                                            }.into_view()
                                                        } else {
                                                            view! {}.into_view()
                                                        }}
                                                    </div>
                                                    <div class="is-size-7 has-text-grey-light" style="margin-top: 2px;">
                                                        {created}
                                                    </div>
                                                </div>
                                            }
                                        }).collect::<Vec<_>>().into_view()
                                    }
                                    }
                                })
                            }
                        }
                        </div>
                    </div>

                    // Add device
                    <div style="padding: 0 16px; margin-top: 24px;">
                        <div style="background: var(--bulma-scheme-main); border-radius: 12px; overflow: hidden;">
                            <div
                                attr:data-testid="privacy-btn-add-device"
                                style="padding: 12px 16px; display: flex; align-items: center; justify-content: space-between; cursor: pointer;"
                                on:click=move |_| show_pair.set(true)
                            >
                                <span class="is-size-6 has-text-link">{move || t("privacy.add_device")}</span>
                                <span style="color: var(--bulma-text-weak); font-size: 18px;">"›"</span>
                            </div>
                        </div>
                    </div>

                    // Passkey on THIS device (e.g. after signing in with the backup phrase).
                    <div style="padding: 0 16px; margin-top: 12px;">
                        <div style="background: var(--bulma-scheme-main); border-radius: 12px; overflow: hidden;">
                            <div
                                attr:data-testid="privacy-btn-add-passkey"
                                style="padding: 12px 16px; display: flex; align-items: center; justify-content: space-between; cursor: pointer;"
                                on:click=on_add_passkey
                            >
                                <span class="is-size-6 has-text-link">
                                    {move || if passkey_busy.get() {
                                        t("privacy.add_passkey_busy")
                                    } else if passkey_done.get() {
                                        t("privacy.add_passkey_done")
                                    } else {
                                        t("privacy.add_passkey")
                                    }}
                                </span>
                                {move || (!passkey_done.get()).then(|| view! {
                                    <span style="color: var(--bulma-text-weak); font-size: 18px;">"›"</span>
                                })}
                            </div>
                        </div>
                        {move || passkey_err.get().map(|e| view! {
                            <p class="is-size-7 has-text-danger" style="margin: 6px 4px 0;">{e}</p>
                        })}
                    </div>

                </div>
            }.into_view()
        }}
    }
}
