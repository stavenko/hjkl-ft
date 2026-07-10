use leptos::*;

use crate::services::i18n::t;
use crate::services::summary;

const CARD: &str = "background: var(--bulma-scheme-main-bis); border-radius: 12px; padding: 14px 16px; margin: 1rem 0 0.5rem 0;";

/// Weekly report shown under the food list on PAST diary days.
///
/// The former daily «Оценка» (AI assessment of the previous day) was removed in
/// favour of background per-food classification (see the `classify` service). Only
/// the weekly report — a short free-text summary computed once the week has ended —
/// remains here.
#[component]
pub fn SummaryBlock(#[prop(into)] date: Signal<String>) -> impl IntoView {
    // Weekly report state. `week_start` is the Monday of the selected day's week.
    let show_week = create_rw_signal(false);
    let week = create_resource(
        move || (date.get(), show_week.get()),
        |(d, show)| async move {
            if !show {
                return None;
            }
            let ws = summary::week_start_of(&d);
            summary::ensure_week(&ws).await.map(|s| s.text)
        },
    );

    view! {
        <div>
            {move || {
                let ws = summary::week_start_of(&date.get());
                if summary::week_ready(&ws) {
                    if show_week.get() {
                        view! {
                            {move || match week.get() {
                                Some(Some(text)) => view! {
                                    <div style=CARD>
                                        <p class="is-size-7 has-text-weight-semibold has-text-grey" style="margin: 0 0 6px 0; text-transform: uppercase; letter-spacing: 0.02em;">
                                            {move || t("summary.week_title")}
                                        </p>
                                        <p class="is-size-6" style="white-space: pre-wrap; line-height: 1.45;">{text}</p>
                                    </div>
                                }.into_view(),
                                Some(None) => ().into_view(),
                                None => view! {
                                    <div style=CARD>
                                        <p class="is-size-7 has-text-grey">{move || t("summary.generating")}</p>
                                    </div>
                                }.into_view(),
                            }}
                        }.into_view()
                    } else {
                        view! {
                            <button
                                class="button is-light is-fullwidth is-rounded"
                                style="margin-top: 0.25rem;"
                                on:click=move |_| show_week.set(true)
                            >{move || t("summary.week_button")}</button>
                        }.into_view()
                    }
                } else {
                    // Week not ended yet: nothing shown until the week is ready.
                    ().into_view()
                }
            }}
        </div>
    }
}
