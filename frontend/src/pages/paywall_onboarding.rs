use leptos::*;

use crate::services::{i18n::t, subscription};

fn currency_symbol(code: &str) -> String {
    match code {
        "RUB" => "\u{20bd}".to_string(),
        "USD" => "$".to_string(),
        "EUR" => "\u{20ac}".to_string(),
        other => other.to_string(),
    }
}

fn days_left(end_ms: i64) -> i64 {
    let now = js_sys::Date::now() as i64;
    let day = 24 * 60 * 60 * 1000;
    (((end_ms - now) + day - 1) / day).max(0)
}

/// The trial / subscribe screen. Shown once per day (on launch and on foreground)
/// while there's no paid subscription. "Skip" is available only while the trial
/// is still running; once it lapses the user must subscribe.
#[component]
pub fn PaywallOnboarding(on_done: Callback<()>) -> impl IntoView {
    let status = create_rw_signal(None::<subscription::Status>);
    let plans = create_rw_signal(Vec::<subscription::Plan>::new());
    let busy = create_rw_signal(false);
    let error = create_rw_signal(None::<String>);

    // Paid users shouldn't be here (self-correct if the cached state was stale).
    spawn_local(async move {
        if let Ok(s) = subscription::status().await {
            if s.is_paid() {
                on_done.call(());
            } else {
                status.set(Some(s));
            }
        }
    });
    spawn_local(async move {
        if let Ok(ps) = subscription::plans().await {
            plans.set(ps);
        }
    });

    let buy = move |plan_id: String| {
        if busy.get_untracked() {
            return;
        }
        busy.set(true);
        error.set(None);
        spawn_local(async move {
            match subscription::checkout("lava", &plan_id).await {
                Ok(url) => {
                    if let Some(w) = web_sys::window() {
                        let _ = w.location().set_href(&url);
                    }
                }
                Err(e) => {
                    busy.set(false);
                    let msg = if e.contains("provider_not_configured") || e.contains("unknown_plan") {
                        t("paywall.not_configured")
                    } else {
                        t("paywall.checkout_error")
                    };
                    error.set(Some(msg.to_string()));
                }
            }
        });
    };

    let skip = move |_| {
        subscription::record_paywall_skip();
        on_done.call(());
    };

    // Trial is "active" while the (non-paid) subscription hasn't lapsed yet.
    let trial_active = move || status.get().map(|s| s.active && !s.is_paid()).unwrap_or(false);
    let trial_days = move || status.get().map(|s| days_left(s.end)).unwrap_or(0);

    view! {
        <div
            attr:data-testid="paywall-onboarding"
            style="display: flex; flex-direction: column; align-items: center; justify-content: center; min-height: 100vh; padding: 32px; background: var(--bulma-scheme-main); text-align: center;"
        >
            <div style="width: 100%; max-width: 380px;">
                <div style="font-size: 64px; margin-bottom: 20px;">"\u{23f3}"</div>

                <h1 class="is-size-3 has-text-weight-bold" style="margin: 0 0 8px 0;">
                    {move || if trial_active() {
                        t("paywall.trial_left").replace("{n}", &trial_days().to_string())
                    } else {
                        t("paywall.trial_expired").to_string()
                    }}
                </h1>

                // Price line (from the catalog).
                {move || plans.get().first().map(|p| {
                    let price = format!("{} {}", p.price.round() as i64, currency_symbol(&p.currency));
                    view! {
                        <p class="is-size-5 has-text-weight-semibold" style="margin: 0 0 24px 0;">
                            {t("paywall.price_line").replace("{price}", &price)}
                        </p>
                    }
                })}

                // Rules.
                <div style="text-align: left; max-width: 320px; margin: 0 auto 28px auto;">
                    <p class="is-size-6 has-text-grey-light" style="line-height: 1.5; margin: 0 0 8px 0;">{move || t("paywall.rule1")}</p>
                    <p class="is-size-6 has-text-grey-light" style="line-height: 1.5; margin: 0 0 8px 0;">{move || t("paywall.rule2")}</p>
                    <p class="is-size-6 has-text-grey-light" style="line-height: 1.5; margin: 0;">{move || t("paywall.rule3")}</p>
                </div>

                // Subscribe (immediate checkout via the provider).
                {move || plans.get().first().map(|p| {
                    let pid = p.id.clone();
                    view! {
                        <button
                            attr:data-testid="paywall-onboarding-btn-subscribe"
                            class="button is-link is-fullwidth is-medium has-text-weight-semibold"
                            style="border: none; border-radius: 12px;"
                            disabled=move || busy.get()
                            on:click=move |_| buy(pid.clone())
                        >
                            {move || t("paywall.subscribe")}
                        </button>
                    }
                })}

                {move || error.get().map(|e| view! {
                    <p class="is-size-7 has-text-danger" style="margin-top: 8px;">{e}</p>
                })}

                // Skip — only while the trial is still active.
                {move || trial_active().then(|| view! {
                    <button
                        attr:data-testid="paywall-onboarding-btn-skip"
                        class="has-text-grey-light is-size-6"
                        style="background: none; border: none; cursor: pointer; margin-top: 16px; padding: 8px;"
                        on:click=skip
                    >
                        {move || t("paywall.skip")}
                    </button>
                })}
            </div>
        </div>
    }
}
