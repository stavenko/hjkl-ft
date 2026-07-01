use leptos::*;
use leptos_router::*;
use wasm_bindgen::JsValue;

use crate::services::i18n::{get_lang, t, Lang};
use crate::services::subscription;

const PAGE_BG: &str = "background: var(--bulma-background); min-height: 100vh; padding: 0; margin: -0.75rem;";
const CARD: &str = "background: var(--bulma-scheme-main); border-radius: 12px; overflow: hidden; padding: 16px;";

fn days_left(end_ms: i64) -> i64 {
    let now = js_sys::Date::now() as i64;
    let day = 24 * 60 * 60 * 1000;
    (((end_ms - now) + day - 1) / day).max(0)
}

/// Locale-aware unit for "N days" (RU declension / EN plural).
fn days_word(n: i64) -> &'static str {
    match get_lang() {
        Lang::En => {
            if n == 1 {
                "day"
            } else {
                "days"
            }
        }
        Lang::Ru => {
            let n100 = n % 100;
            let n10 = n % 10;
            if (11..=14).contains(&n100) {
                "дней"
            } else if n10 == 1 {
                "день"
            } else if (2..=4).contains(&n10) {
                "дня"
            } else {
                "дней"
            }
        }
    }
}

/// "N дней" / "N days" — a count of days (never a date).
fn days_phrase(n: i64) -> String {
    format!("{} {}", n, days_word(n))
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
    let busy = create_rw_signal(false);

    // Refund dialog state.
    let show_refund = create_rw_signal(false);
    let refund = create_rw_signal(None::<subscription::RefundPreview>);
    let refund_err = create_rw_signal(None::<String>);

    spawn_local(async move {
        if let Ok(s) = subscription::status().await {
            status.set(Some(s));
        }
    });

    // Cancel auto-renew. Stays active until the paid period ends — the confirm spells
    // out HOW MANY DAYS of access remain (a count, not a date).
    let cancel = move |_| {
        if busy.get_untracked() {
            return;
        }
        let dleft = status.get_untracked().map(|s| days_left(s.end)).unwrap_or(0);
        let msg = t("settings.sub_cancel_msg").replace("{n}", &days_phrase(dleft));
        let win = web_sys::window().unwrap();
        if !win.confirm_with_message(&msg).unwrap_or(false) {
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

    // Open the refund dialog and fetch the (server-computed) prorated amount.
    let open_refund = move |_| {
        refund_err.set(None);
        refund.set(None);
        show_refund.set(true);
        spawn_local(async move {
            match subscription::refund_preview().await {
                Ok(p) => refund.set(Some(p)),
                Err(e) => refund_err.set(Some(e)),
            }
        });
    };

    // Confirm: records the request for the operator AND revokes access immediately.
    let confirm_refund = move |_| {
        if busy.get_untracked() {
            return;
        }
        busy.set(true);
        refund_err.set(None);
        spawn_local(async move {
            match subscription::refund_request().await {
                Ok(s) => {
                    status.set(Some(s));
                    show_refund.set(false);
                }
                Err(_) => refund_err.set(Some(t("settings.sub_refund_error").to_string())),
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
                                Some("cancelled") => t("settings.sub_cancelled"),
                                Some("paid") if s.no_renew == Some(true) => t("settings.sub_cancelled"),
                                Some("paid") => t("settings.sub_active"),
                                _ if s.active => t("settings.sub_trial"),
                                _ => t("settings.sub_expired"),
                            };
                            let cls = if s.active { "has-text-success" } else { "has-text-danger" };
                            // Access-until: date + how many days remain (a count).
                            let expires = format!("{} \u{00b7} {}", fmt_date(s.end), days_phrase(days_left(s.end)));

                            view! {
                                <p class=format!("is-size-5 has-text-weight-bold {cls}") style="margin: 0 0 8px 0;">{label}</p>
                                {(s.start > 0).then(|| info_row(t("settings.sub_since").to_string(), fmt_date(s.start)))}
                                {info_row(t("settings.sub_until").to_string(), expires)}
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

                    // Request a refund — ONLY once the subscription is cancelled and
                    // still within the paid (now unused) period.
                    {move || {
                        let show = status.get().map(|s| s.active && s.no_renew == Some(true)).unwrap_or(false);
                        show.then(|| view! {
                            <button
                                attr:data-testid="sub-btn-refund"
                                class="button is-ghost is-fullwidth has-text-danger"
                                style="margin-top: 6px; text-decoration: underline;"
                                disabled=move || busy.get()
                                on:click=open_refund
                            >
                                {move || t("settings.sub_refund")}
                            </button>
                        })
                    }}
                </div>

                // Not subscribed → purchase happens on the website (landing), not in-app.
                {move || {
                    let unpaid = status.get().map(|s| !s.is_paid()).unwrap_or(false);
                    unpaid.then(|| view! {
                        <p class="is-size-6 has-text-grey" style="margin-top: 16px; line-height: 1.5;">
                            {move || t("settings.sub_buy_on_site")}
                        </p>
                        <a
                            class="button is-link is-fullwidth is-medium"
                            style="margin-top: 8px;"
                            href="https://renorma.app"
                            target="_blank"
                            rel="noopener"
                        >
                            {move || t("settings.sub_open_site")}
                        </a>
                    })
                }}
            </div>

            // ── Refund dialog ──
            {move || show_refund.get().then(|| view! {
                <div class="modal is-active">
                    <div class="modal-background" on:click=move |_| { if !busy.get_untracked() { show_refund.set(false); } }></div>
                    <div class="modal-card" style="max-width: 26rem; width: calc(100% - 2rem); margin: auto;">
                        <section class="modal-card-body" style="border-radius: 14px; padding: 22px;">
                            <h2 class="is-size-5 has-text-weight-bold" style="margin: 0 0 10px 0;">{move || t("settings.sub_refund_title")}</h2>
                            <p style="margin: 0 0 14px 0; line-height: 1.5;">{move || t("settings.sub_refund_warn")}</p>

                            // Computed amount (or a spinner while it loads).
                            {move || match refund.get() {
                                Some(p) => {
                                    let amount = format!("{} {}", p.amount, currency_symbol(&p.currency));
                                    view! {
                                        <div style="display: flex; align-items: baseline; justify-content: space-between; gap: 12px; padding: 12px 0; border-top: 0.5px solid var(--bulma-border-weak); border-bottom: 0.5px solid var(--bulma-border-weak);">
                                            <span class="is-size-6 has-text-grey">{move || t("settings.sub_refund_amount")}</span>
                                            <span class="is-size-4 has-text-weight-bold">{amount}</span>
                                        </div>
                                    }.into_view()
                                }
                                None => view! {
                                    <div style="display: flex; justify-content: center; padding: 16px 0;">
                                        {move || refund_err.get().map(|_| ().into_view())
                                            .unwrap_or_else(|| view! { <div class="ft-spinner"></div> }.into_view())}
                                    </div>
                                }.into_view(),
                            }}

                            <p class="is-size-7 has-text-grey" style="margin: 12px 0 0 0; line-height: 1.5;">
                                {move || t("settings.sub_refund_processing")}
                            </p>

                            {move || refund_err.get().map(|e| view! {
                                <p class="is-size-7 has-text-danger" style="margin: 10px 0 0 0;">{e}</p>
                            })}

                            <div style="display: flex; gap: 10px; margin-top: 20px;">
                                <button
                                    class="button is-light is-fullwidth"
                                    disabled=move || busy.get()
                                    on:click=move |_| show_refund.set(false)
                                >
                                    {move || t("common.back")}
                                </button>
                                <button
                                    attr:data-testid="sub-btn-refund-confirm"
                                    class="button is-danger is-fullwidth"
                                    disabled=move || busy.get() || refund.get().is_none()
                                    on:click=confirm_refund
                                >
                                    {move || t("settings.sub_refund_confirm")}
                                </button>
                            </div>
                        </section>
                    </div>
                </div>
            })}

            <div style="height: 40px;"></div>
        </div>
    }
}
