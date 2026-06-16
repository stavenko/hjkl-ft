use leptos::*;
use leptos_router::*;

use crate::services::{i18n::t, subscription};

const PAGE_BG: &str = "background: var(--bulma-background); min-height: 100vh; padding: 0; margin: -0.75rem;";
const CARD: &str = "background: var(--bulma-scheme-main); border-radius: 12px; overflow: hidden; padding: 16px;";

fn days_left(end_ms: i64) -> i64 {
    let now = js_sys::Date::now() as i64;
    let day = 24 * 60 * 60 * 1000;
    ((end_ms - now) + day - 1) / day
}

#[component]
pub fn PaywallPage() -> impl IntoView {
    let navigate = use_navigate();

    let status = create_rw_signal(None::<subscription::Status>);
    let code = create_rw_signal(String::new());
    let error = create_rw_signal(None::<String>);
    let busy = create_rw_signal(false);

    // Load current subscription status on mount (lazily creates a trial server-side).
    spawn_local(async move {
        if let Ok(s) = subscription::status().await {
            status.set(Some(s));
        }
    });

    let on_pay = move |_| {
        let code_val = code.get_untracked().trim().to_string();
        if code_val.is_empty() || busy.get_untracked() {
            return;
        }
        busy.set(true);
        error.set(None);
        spawn_local(async move {
            match subscription::pay(&code_val).await {
                Ok(s) => {
                    status.set(Some(s));
                    code.set(String::new());
                }
                Err(e) => {
                    if e.contains("HTTP 400") || e.contains("invalid_code") {
                        error.set(Some(t("paywall.invalid_code").to_string()));
                    } else {
                        error.set(Some(e));
                    }
                }
            }
            busy.set(false);
        });
    };

    view! {
        <div style=PAGE_BG>
            <div style="display: flex; align-items: center; padding: 12px 16px;">
                <button
                    style="appearance: none; -webkit-appearance: none; border: none; background: none; cursor: pointer; padding: 4px; font: inherit;"
                    class="is-size-5"
                    on:click={
                        let nav = navigate.clone();
                        move |_| nav("/", Default::default())
                    }
                >
                    <span class="has-text-link">{move || t("common.back")}</span>
                </button>
            </div>

            <h1 class="is-size-1 has-text-weight-bold" style="margin: 0 16px 16px 16px;">{move || t("story.ch1.next")}</h1>

            <div style="padding: 0 16px 8px 16px;">
                <p class="is-size-6" style="line-height: 1.55; margin: 0 0 14px 0;">{move || t("story.next.p1")}</p>
                <p class="is-size-6" style="line-height: 1.55; margin: 0 0 14px 0;">{move || t("story.next.p2")}</p>
                <p class="is-size-6" style="line-height: 1.55; margin: 0 0 8px 0;">{move || t("story.next.p3")}</p>
            </div>

            // ---- Subscription status + paywall ----
            <div style="padding: 16px 16px 0 16px;">
                <div style=CARD>
                    // Status line
                    {move || match status.get() {
                        None => view! { <p class="is-size-6 has-text-grey">{move || t("paywall.loading")}</p> }.into_view(),
                        Some(s) => {
                            let line = if s.is_paid() {
                                format!("{} \u{2014} {} {}", t("paywall.status_paid"), days_left(s.end), t("paywall.days_left"))
                            } else if s.active {
                                format!("{} \u{2014} {} {}", t("paywall.status_trial"), days_left(s.end), t("paywall.days_left"))
                            } else {
                                t("paywall.status_expired").to_string()
                            };
                            let cls = if s.active { "has-text-success" } else { "has-text-danger" };
                            view! { <p class=format!("is-size-6 has-text-weight-semibold {cls}")>{line}</p> }.into_view()
                        }
                    }}

                    // Already paid → no need for the form
                    <Show when=move || !status.get().map(|s| s.is_paid()).unwrap_or(false)>
                        <div style="margin-top: 14px;">
                            <input
                                attr:data-testid="paywall-input-code"
                                type="text"
                                class="input"
                                placeholder=t("paywall.code_placeholder")
                                prop:value=move || code.get()
                                on:input=move |ev| code.set(event_target_value(&ev))
                            />
                            {move || error.get().map(|e| view! {
                                <p class="has-text-danger is-size-7" style="margin-top: 8px;">{e}</p>
                            })}
                            <button
                                attr:data-testid="paywall-btn-pay"
                                class="button is-link is-fullwidth is-medium"
                                style="margin-top: 12px;"
                                disabled=move || busy.get()
                                on:click=on_pay
                            >
                                {move || if busy.get() { t("paywall.paying") } else { t("paywall.pay_button") }}
                            </button>
                        </div>
                    </Show>

                    <Show when=move || status.get().map(|s| s.is_paid()).unwrap_or(false)>
                        <p class="is-size-6 has-text-weight-semibold has-text-success" style="margin-top: 14px;">
                            {move || t("paywall.success")}
                        </p>
                    </Show>
                </div>

                <button
                    class="button is-light is-fullwidth is-medium"
                    style="margin-top: 16px;"
                    on:click={
                        let nav = navigate.clone();
                        move |_| nav("/", Default::default())
                    }
                >
                    {move || t("paywall.back_to_story")}
                </button>
            </div>

            <div style="height: 40px;"></div>
        </div>
    }
}
