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

fn currency_symbol(code: &str) -> String {
    match code {
        "RUB" => "\u{20bd}".to_string(),
        "USD" => "$".to_string(),
        "EUR" => "\u{20ac}".to_string(),
        other => other.to_string(),
    }
}

#[component]
pub fn PaywallPage() -> impl IntoView {
    let navigate = use_navigate();
    // Each <Show> captures its children (and thus `navigate`) by move, so give the
    // two Show branches their own owned clones.
    let nav_welcome = navigate.clone();
    let nav_main = navigate.clone();

    // `?status=success` — the buyer just returned from a successful lava checkout
    // (the webhook may still be in flight, so key the welcome screen on the param).
    let query = use_query_map();
    let just_paid = move || query.with(|q| q.get("status").map(|s| s == "success").unwrap_or(false));

    let status = create_rw_signal(None::<subscription::Status>);
    let plans = create_rw_signal(Vec::<subscription::Plan>::new());
    let error = create_rw_signal(None::<String>);
    // The plan id currently redirecting to checkout (so its button shows progress).
    let busy = create_rw_signal(None::<String>);

    // Load current subscription status (lazily creates a trial server-side) + plans.
    spawn_local(async move {
        if let Ok(s) = subscription::status().await {
            status.set(Some(s));
        }
    });
    spawn_local(async move {
        if let Ok(ps) = subscription::plans().await {
            plans.set(ps);
        }
    });

    // Start checkout for a plan → redirect the browser to the hosted page.
    let buy = move |plan_id: String| {
        if busy.get_untracked().is_some() {
            return;
        }
        busy.set(Some(plan_id.clone()));
        error.set(None);
        spawn_local(async move {
            match subscription::checkout("lava", &plan_id).await {
                Ok(url) => {
                    if let Some(w) = web_sys::window() {
                        let _ = w.location().set_href(&url);
                    }
                }
                Err(e) => {
                    busy.set(None);
                    let msg = if e.contains("provider_not_configured") || e.contains("unknown_plan") {
                        t("paywall.not_configured").to_string()
                    } else {
                        t("paywall.checkout_error").to_string()
                    };
                    error.set(Some(msg));
                }
            }
        });
    };

    view! {
        <div style=PAGE_BG>
            <div style="display: flex; align-items: center; padding: 12px 16px;">
                <button
                    style="appearance: none; -webkit-appearance: none; border: none; background: none; cursor: pointer; padding: 4px; font: inherit;"
                    class="is-size-5"
                    on:click={ let nav = navigate.clone(); move |_| nav("/", Default::default()) }
                >
                    <span class="has-text-link">{move || t("common.back")}</span>
                </button>
            </div>

            <h1 class="is-size-1 has-text-weight-bold" style="margin: 0 16px 16px 16px;">{move || t("story.ch1.next")}</h1>

            // Just returned from a successful checkout → welcome + where to manage.
            <Show when=just_paid>
                <div style="padding: 0 16px;">
                    <div style=format!("{} text-align: center;", CARD)>
                        <p class="is-size-3 has-text-weight-bold has-text-success" style="margin: 0 0 8px 0;">{move || t("paywall.welcome_title")}</p>
                        <p class="is-size-6" style="line-height: 1.55; margin: 0 0 16px 0;">{move || t("paywall.welcome_body")}</p>
                        <button
                            class="button is-link is-fullwidth is-medium"
                            on:click={ let nav = nav_welcome.clone(); move |_| nav("/settings/subscription", Default::default()) }
                        >
                            {move || t("paywall.welcome_manage")}
                        </button>
                        <button
                            class="button is-light is-fullwidth is-medium"
                            style="margin-top: 10px;"
                            on:click={ let nav = nav_welcome.clone(); move |_| nav("/", Default::default()) }
                        >
                            {move || t("paywall.back_to_story")}
                        </button>
                    </div>
                </div>
            </Show>

            <Show when=move || !just_paid()>
            <div style="padding: 0 16px 8px 16px;">
                <p class="is-size-6" style="line-height: 1.55; margin: 0 0 14px 0;">{move || t("story.next.p1")}</p>
                <p class="is-size-6" style="line-height: 1.55; margin: 0 0 14px 0;">{move || t("story.next.p2")}</p>
                <p class="is-size-6" style="line-height: 1.55; margin: 0 0 8px 0;">{move || t("story.next.p3")}</p>
            </div>

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

                    // Paid → success; otherwise the plan picker.
                    <Show when=move || status.get().map(|s| s.is_paid()).unwrap_or(false)>
                        <p class="is-size-6 has-text-weight-semibold has-text-success" style="margin-top: 14px;">
                            {move || t("paywall.success")}
                        </p>
                    </Show>

                    <Show when=move || !status.get().map(|s| s.is_paid()).unwrap_or(false)>
                        <p class="is-size-7 has-text-grey-light" style="text-transform: uppercase; letter-spacing: 0.02em; margin: 16px 0 8px 0;">
                            {move || t("paywall.choose_plan")}
                        </p>
                        <For
                            each=move || plans.get()
                            key=|p| p.id.clone()
                            children=move |p: subscription::Plan| {
                                let pid = p.id.clone();
                                let period = if p.period == "year" { t("paywall.per_year") } else { t("paywall.per_month") };
                                let price = format!("{} {} {}", p.price.round() as i64, currency_symbol(&p.currency), period);
                                let buy = buy.clone();
                                let pid2 = pid.clone();
                                view! {
                                    <div style="display: flex; align-items: center; justify-content: space-between; gap: 12px; padding: 12px 0; border-bottom: 0.5px solid var(--bulma-border-weak);">
                                        <div>
                                            <div class="is-size-6 has-text-weight-semibold">{p.title.clone()}</div>
                                            <div class="is-size-7 has-text-grey">{price}</div>
                                        </div>
                                        <button
                                            class="button is-link is-small"
                                            disabled=move || busy.get().is_some()
                                            on:click=move |_| buy(pid2.clone())
                                        >
                                            {move || if busy.get().as_deref() == Some(pid.as_str()) {
                                                t("paywall.paying")
                                            } else {
                                                t("paywall.pay_button")
                                            }}
                                        </button>
                                    </div>
                                }
                            }
                        />
                        {move || error.get().map(|e| view! {
                            <p class="has-text-danger is-size-7" style="margin-top: 12px;">{e}</p>
                        })}
                    </Show>
                </div>

                <button
                    class="button is-light is-fullwidth is-medium"
                    style="margin-top: 16px;"
                    on:click={ let nav = nav_main.clone(); move |_| nav("/", Default::default()) }
                >
                    {move || t("paywall.back_to_story")}
                </button>
            </div>
            </Show>

            <div style="height: 40px;"></div>
        </div>
    }
}
