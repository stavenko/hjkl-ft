use leptos::*;
use leptos_router::*;
use api_types::StepEntry;

use crate::components::steps_widget::chart_svg_steps;
use crate::components::mini_chart::short_date;
use crate::services::i18n::t;

#[component]
pub fn StepsChartModal(
    entries: Signal<Vec<StepEntry>>,
    on_close: Callback<()>,
) -> impl IntoView {
    let navigate = use_navigate();

    // "Today" in local time — the record button resets at local midnight.
    let today = chrono::Local::now().format("%Y-%m-%d").to_string();
    let has_today = {
        let today = today.clone();
        move || entries.get().iter().any(|e| e.date == today)
    };

    view! {
        <div class="modal is-active" style="z-index: 70;">
            <div class="modal-background" on:click=move |_| on_close.call(())></div>
            <div class="modal-card" style="max-width: 480px;">
                <header class="modal-card-head">
                    <p class="modal-card-title">{move || t("steps.title")}</p>
                    <button class="delete" aria-label="close" on:click=move |_| on_close.call(())></button>
                </header>

                // Fixed: chart + record button + explanation (always on top).
                <div style="flex-shrink: 0; padding: 16px 20px; background: var(--bulma-scheme-main); border-bottom: 0.5px solid var(--bulma-border-weak);">
                    <div inner_html=move || chart_svg_steps(&entries.get())></div>

                    <button
                        class="button is-link is-fullwidth"
                        style="margin-top: 16px;"
                        on:click={
                            let nav = navigate.clone();
                            move |_| {
                                on_close.call(());
                                nav("/steps", Default::default());
                            }
                        }
                    >
                        {move || if has_today() { t("steps.edit") } else { t("steps.add") }}
                    </button>
                    <p class="is-size-7 has-text-grey" style="margin: 8px 0 0 0; text-align: center;">
                        {move || t("steps.once_per_day")}
                    </p>
                </div>

                // Scrollable: the steps table.
                <section class="modal-card-body">
                    <table class="table is-fullwidth is-narrow">
                        <thead>
                            <tr>
                                <th>{move || t("weight.col_date")}</th>
                                <th>{move || t("weight.col_time")}</th>
                                <th style="text-align: right;">{move || t("steps.col_steps")}</th>
                            </tr>
                        </thead>
                        <tbody>
                            {move || {
                                let mut es = entries.get();
                                es.sort_by(|a, b| b.date.cmp(&a.date));
                                es.into_iter().map(|e| {
                                    view! {
                                        <tr>
                                            <td>{short_date(&e.date)}</td>
                                            <td>{entry_time(&e.created_at)}</td>
                                            <td style="text-align: right;">{e.steps.to_string()}</td>
                                        </tr>
                                    }
                                }).collect_view()
                            }}
                        </tbody>
                    </table>
                </section>
            </div>
        </div>
    }
}

/// Local HH:MM from an RFC3339 (UTC) timestamp.
fn entry_time(created_at: &str) -> String {
    chrono::DateTime::parse_from_rfc3339(created_at)
        .map(|dt| dt.with_timezone(&chrono::Local).format("%H:%M").to_string())
        .unwrap_or_default()
}
