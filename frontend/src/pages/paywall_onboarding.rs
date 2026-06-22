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

/// Startup onboarding step: presents the subscription right after push onboarding.
/// Skippable — tapping "Later" continues on the trial. Paid users are skipped.
#[component]
pub fn PaywallOnboarding(on_done: Callback<()>) -> impl IntoView {
    let plans = create_rw_signal(Vec::<subscription::Plan>::new());
    let busy = create_rw_signal(false);
    let error = create_rw_signal(None::<String>);

    // Already paid → don't dwell on the paywall.
    spawn_local(async move {
        if let Ok(s) = subscription::status().await {
            if s.is_paid() {
                subscription::dismiss_paywall_onboarding();
                on_done.call(());
            }
        }
    });
    spawn_local(async move {
        if let Ok(ps) = subscription::plans().await {
            plans.set(ps);
        }
    });

    // Start checkout → redirect to the hosted page (and don't show this step again).
    let buy = move |plan_id: String| {
        if busy.get_untracked() {
            return;
        }
        busy.set(true);
        error.set(None);
        spawn_local(async move {
            match subscription::checkout("lava", &plan_id).await {
                Ok(url) => {
                    subscription::dismiss_paywall_onboarding();
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

    let later = move |_| {
        subscription::dismiss_paywall_onboarding();
        on_done.call(());
    };

    view! {
        <div
            attr:data-testid="paywall-onboarding"
            style="display: flex; flex-direction: column; align-items: center; justify-content: center; min-height: 100vh; padding: 32px; background: var(--bulma-scheme-main); text-align: center;"
        >
            <div style="width: 100%; max-width: 360px;">
                <div style="font-size: 64px; margin-bottom: 24px;">"💳"</div>

                <h1 class="is-size-3 has-text-weight-bold" style="margin: 0 0 16px 0;">
                    {move || t("paywall.onb_title")}
                </h1>

                <p class="is-size-6 has-text-grey-light" style="line-height: 1.5; margin: 0 auto 10px auto; max-width: 320px;">
                    {move || t("story.sub.p1")}
                </p>
                <p class="is-size-6 has-text-grey-light" style="line-height: 1.5; margin: 0 auto 10px auto; max-width: 320px;">
                    {move || t("story.sub.p2")}
                </p>
                <p class="is-size-6 has-text-grey-light" style="line-height: 1.5; margin: 0 auto 24px auto; max-width: 320px;">
                    {move || t("story.sub.p3")}
                </p>

                <For
                    each=move || plans.get()
                    key=|p| p.id.clone()
                    children=move |p: subscription::Plan| {
                        let period = if p.period == "year" { t("paywall.per_year") } else { t("paywall.per_month") };
                        let price = format!("{} {} {}", p.price.round() as i64, currency_symbol(&p.currency), period);
                        let buy = buy.clone();
                        let pid = p.id.clone();
                        view! {
                            <button
                                attr:data-testid="paywall-onboarding-btn-buy"
                                class="button is-link is-fullwidth is-medium"
                                style="border: none; border-radius: 12px; margin-bottom: 10px;"
                                disabled=move || busy.get()
                                on:click=move |_| buy(pid.clone())
                            >
                                <span class="has-text-weight-semibold">{move || t("story.sub.cta")}</span>
                                <span style="opacity: 0.85; margin-left: 8px;">{price}</span>
                            </button>
                        }
                    }
                />

                {move || error.get().map(|e| view! {
                    <p class="is-size-7 has-text-danger" style="margin-top: 8px;">{e}</p>
                })}

                <button
                    attr:data-testid="paywall-onboarding-btn-later"
                    class="has-text-grey-light is-size-6"
                    style="background: none; border: none; cursor: pointer; margin-top: 16px; padding: 8px;"
                    on:click=later
                >
                    {move || t("paywall.later")}
                </button>
            </div>
        </div>
    }
}
