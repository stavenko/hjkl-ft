use leptos::*;
use leptos_router::*;
use api_types::StepEntry;

use crate::components::bar_chart::BarChart;
use crate::components::mini_chart::short_date;
use crate::services::i18n::t;

/// Steps widget in its EXPANDED form — content only, to sit inside the shared
/// full-screen editor overlay (`EDITOR` + `EditorHead`), same as every other new
/// expanded widget. `on_close` is called just before navigating away to `/steps`.
#[component]
pub fn StepsPanel(
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
        <div style="display: flex; flex-direction: column; gap: 14px; padding: 4px 2px;">
            // Chart + record button + explanation.
            <div>
                <BarChart series=Signal::derive(move || steps_series(&entries.get()))
                    unit=t("common.unit.steps").to_string()/>
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

            // History table.
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
        </div>
    }
}

/// `(date, steps)` pairs oldest → newest for the bar chart.
fn steps_series(entries: &[StepEntry]) -> Vec<(String, f64)> {
    let mut es = entries.to_vec();
    es.sort_by(|a, b| a.date.cmp(&b.date));
    es.into_iter().map(|e| (e.date, e.steps as f64)).collect()
}

/// Local HH:MM from an RFC3339 (UTC) timestamp.
fn entry_time(created_at: &str) -> String {
    chrono::DateTime::parse_from_rfc3339(created_at)
        .map(|dt| dt.with_timezone(&chrono::Local).format("%H:%M").to_string())
        .unwrap_or_default()
}
