use leptos::*;
use leptos_router::*;
use wasm_bindgen::JsValue;

use crate::services::{i18n::t, subscription};

const PAGE_BG: &str = "background: var(--bulma-background); min-height: 100vh; padding: 0; margin: -0.75rem;";
const CARD: &str = "background: var(--bulma-scheme-main); border-radius: 12px; overflow: hidden; padding: 16px;";

fn days_left(end_ms: i64) -> i64 {
    let now = js_sys::Date::now() as i64;
    let day = 24 * 60 * 60 * 1000;
    (((end_ms - now) + day - 1) / day).max(0)
}

/// Locale-independent DD.MM.YYYY (no locale API → deterministic).
fn fmt_date(ms: i64) -> String {
    if ms <= 0 {
        return String::new();
    }
    let d = js_sys::Date::new(&JsValue::from_f64(ms as f64));
    format!("{:02}.{:02}.{}", d.get_date(), d.get_month() + 1, d.get_full_year())
}

fn currency_symbol(code: &str) -> String {
    match code {
        "RUB" => "\u{20bd}".to_string(),
        "USD" => "$".to_string(),
        "EUR" => "\u{20ac}".to_string(),
        other => other.to_string(),
    }
}

/// A labelled "key: value" row.
fn info_row(label: String, value: String) -> impl IntoView {
    view! {
        <div style="display: flex; align-items: baseline; justify-content: space-between; gap: 12px; padding: 10px 0; border-bottom: 0.5px solid var(--bulma-border-weak);">
            <span class="is-size-7 has-text-grey">{label}</span>
            <span class="is-size-6 has-text-weight-semibold" style="text-align: right;">{value}</span>
        </div>
    }
}

#[component]
pub fn SubscriptionPage() -> impl IntoView {
    let navigate = use_navigate();

    let status = create_rw_signal(None::<subscription::Status>);
    let plans = create_rw_signal(Vec::<subscription::Plan>::new());
    let busy = create_rw_signal(false);

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

    // Cancel the subscription directly (real provider cancel). Stays active until
    // the paid period ends; just stops auto-renew.
    let cancel = move |_| {
        if busy.get_untracked() {
            return;
        }
        let win = web_sys::window().unwrap();
        if !win.confirm_with_message(&t("settings.sub_cancel_confirm")).unwrap_or(false) {
            return;
        }
        busy.set(true);
        spawn_local(async move {
            if let Ok(s) = subscription::cancel().await {
                status.set(Some(s));
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
                    on:click={ let nav = navigate.clone(); move |_| nav("/settings", Default::default()) }
                >
                    <span class="has-text-link">{move || t("common.back")}</span>
                </button>
            </div>

            <h1 class="is-size-1 has-text-weight-bold" style="margin: 0 16px 16px 16px;">{move || t("settings.subscription")}</h1>

            <div style="padding: 0 16px;">
                <div style=CARD>
                    {move || match status.get() {
                        None => view! { <p class="is-size-6 has-text-grey">{move || t("paywall.loading")}</p> }.into_view(),
                        Some(s) => {
                            let label = match s.status.as_deref() {
                                Some("paid") if s.no_renew == Some(true) => t("settings.sub_cancelled"),
                                Some("paid") => t("settings.sub_active"),
                                _ if s.active => t("settings.sub_trial"),
                                _ => t("settings.sub_expired"),
                            };
                            let cls = if s.active { "has-text-success" } else { "has-text-danger" };

                            // Cost: the plan matching the current sub, else the first (monthly) plan.
                            let ps = plans.get();
                            let cost = ps.iter().find(|p| p.id == s.plan).or_else(|| ps.first()).map(|p| {
                                let period = if p.period == "year" { t("paywall.per_year") } else { t("paywall.per_month") };
                                format!("{} {} {}", p.price.round() as i64, currency_symbol(&p.currency), period)
                            });

                            let expires = format!("{} \u{00b7} {} {}", fmt_date(s.end), days_left(s.end), t("paywall.days_left"));

                            view! {
                                <p class=format!("is-size-5 has-text-weight-bold {cls}") style="margin: 0 0 8px 0;">{label}</p>
                                {(s.start > 0).then(|| info_row(t("settings.sub_since").to_string(), fmt_date(s.start)))}
                                {info_row(t("settings.sub_until").to_string(), expires)}
                                {cost.map(|c| info_row(t("settings.sub_cost").to_string(), c))}
                            }.into_view()
                        }
                    }}

                    // Cancel — only while paid and still auto-renewing.
                    {move || {
                        let show = status.get().map(|s| s.is_paid() && s.no_renew != Some(true)).unwrap_or(false);
                        show.then(|| view! {
                            <button
                                attr:data-testid="sub-btn-cancel"
                                class="button is-danger is-light is-fullwidth"
                                style="margin-top: 16px;"
                                disabled=move || busy.get()
                                on:click=cancel
                            >
                                {move || t("settings.sub_cancel")}
                            </button>
                        })
                    }}
                </div>

                // Not subscribed → a way to subscribe.
                {move || {
                    let unpaid = status.get().map(|s| !s.is_paid()).unwrap_or(false);
                    unpaid.then(|| view! {
                        <button
                            class="button is-link is-fullwidth is-medium"
                            style="margin-top: 16px;"
                            on:click={ let nav = navigate.clone(); move |_| nav("/paywall", Default::default()) }
                        >
                            {move || t("story.sub.cta")}
                        </button>
                    })
                }}
            </div>

            <div style="height: 40px;"></div>
        </div>
    }
}
