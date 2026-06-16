use leptos::*;
use leptos_router::*;
use api_types::WeightEntry;

use crate::components::weight_widget::chart_svg;
use crate::components::mini_chart::short_date;
use crate::services::i18n::{t, weight_unit_signal, WeightUnit};

#[component]
pub fn WeightChartModal(
    entries: Signal<Vec<WeightEntry>>,
    on_close: Callback<()>,
) -> impl IntoView {
    let navigate = use_navigate();
    let unit = weight_unit_signal();

    // "Today" in local time — the add button resets at local midnight because
    // this is recomputed each time the modal opens.
    let today = chrono::Local::now().format("%Y-%m-%d").to_string();
    let has_today = {
        let today = today.clone();
        move || entries.get().iter().any(|e| e.date == today)
    };

    let unit_label = move || match unit.get() {
        WeightUnit::Kg => t("weight.unit_kg"),
        WeightUnit::Lbs => t("weight.unit_lbs"),
    };

    view! {
        <div class="modal is-active" style="z-index: 70;">
            <div class="modal-background" on:click=move |_| on_close.call(())></div>
            <div class="modal-card" style="max-width: 480px;">
                <header class="modal-card-head">
                    <p class="modal-card-title">{move || t("weight.widget_title")}</p>
                    <button class="delete" aria-label="close" on:click=move |_| on_close.call(())></button>
                </header>

                // Fixed: chart + add button + explanation (always on top).
                <div style="flex-shrink: 0; padding: 16px 20px; background: var(--bulma-scheme-main); border-bottom: 0.5px solid var(--bulma-border-weak);">
                    <div inner_html=move || chart_svg(&entries.get(), unit.get())></div>

                    <button
                        class="button is-link is-fullwidth"
                        style="margin-top: 16px;"
                        on:click={
                            let nav = navigate.clone();
                            move |_| {
                                on_close.call(());
                                nav("/weight", Default::default());
                            }
                        }
                    >
                        {move || if has_today() { t("weight.edit") } else { t("weight.add") }}
                    </button>
                    <p class="is-size-7 has-text-grey" style="margin: 8px 0 0 0; text-align: center;">
                        {move || t("weight.once_per_day")}
                    </p>
                </div>

                // Scrollable: the weights table.
                <section class="modal-card-body">
                    <table class="table is-fullwidth is-narrow">
                        <thead>
                            <tr>
                                <th>{move || t("weight.col_date")}</th>
                                <th>{move || t("weight.col_time")}</th>
                                <th>{move || t("weight.col_quality")}</th>
                                <th style="text-align: right;">{move || t("weight.col_weight")}</th>
                            </tr>
                        </thead>
                        <tbody>
                            {move || {
                                let mut es = entries.get();
                                es.sort_by(|a, b| b.date.cmp(&a.date));
                                let u = unit.get();
                                let ul = unit_label();
                                es.into_iter().map(|e| {
                                    let v = u.from_kg(e.weight_kg);
                                    let quality = [e.no_water, e.no_food, e.no_wash, e.used_toilet, e.morning]
                                        .iter().filter(|&&b| b).count();
                                    view! {
                                        <tr>
                                            <td>{short_date(&e.date)}</td>
                                            <td>{entry_time(&e.created_at)}</td>
                                            <td>{format!("{}/5", quality)}</td>
                                            <td style="text-align: right;">{format!("{:.1} {}", v, ul)}</td>
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
