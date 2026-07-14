use leptos::*;
use leptos_router::*;
use api_types::WeightEntry;

use crate::components::weight_widget::chart_svg;
use crate::components::mini_chart::short_date;
use crate::services::i18n::{t, weight_unit_signal, WeightUnit};
use crate::services::profile::{self, Sex};
use crate::services::weight_cycle::{weight_cycle, CycleResult, CYCLE_WINDOW_DAYS};
use crate::services::weight_trend::{
    weight_trend, BalanceState, Direction, WeightTrend, CONFIDENT, DEFAULT_WINDOW_DAYS,
};

/// Weight widget in its EXPANDED form — content only, to sit inside the shared
/// full-screen editor overlay (`EDITOR` + `EditorHead`), same as every other new
/// expanded widget. `on_close` is called just before navigating away to `/weight`.
#[component]
pub fn WeightPanel(
    entries: Signal<Vec<WeightEntry>>,
    on_close: Callback<()>,
) -> impl IntoView {
    let navigate = use_navigate();
    let unit = weight_unit_signal();

    // "Today" in local time — the add button resets at local midnight because
    // this is recomputed each time the panel opens.
    let today = chrono::Local::now().format("%Y-%m-%d").to_string();
    let has_today = {
        let today = today.clone();
        move || entries.get().iter().any(|e| e.date == today)
    };

    let unit_label = move || match unit.get() {
        WeightUnit::Kg => t("weight.unit_kg"),
        WeightUnit::Lbs => t("weight.unit_lbs"),
    };

    // 14-day trend summary shown under the chart.
    let trend_text = move || {
        let u = unit.get();
        let ul = unit_label();
        match weight_trend(&entries.get(), DEFAULT_WINDOW_DAYS) {
            WeightTrend::Insufficient { .. } => t("weight.trend.insufficient").to_string(),
            WeightTrend::Tentative { direction, .. } => {
                let (arrow, key) = dir_label(direction);
                format!("{arrow} {} · {}", t(key), t("weight.trend.preliminary"))
            }
            WeightTrend::Estimated { direction, slope_kg_per_week, confidence, .. } => {
                if confidence < CONFIDENT {
                    t("weight.trend.stable").to_string()
                } else {
                    let (arrow, key) = dir_label(direction);
                    let rate = u.from_kg(slope_kg_per_week.abs());
                    format!(
                        "{arrow} {} · {:.1} {}/{} · {} {}%",
                        t(key),
                        rate,
                        ul,
                        t("weight.trend.week"),
                        t("weight.trend.confidence"),
                        (confidence * 100.0).round() as i64,
                    )
                }
            }
        }
    };
    let trend_color = move || match weight_trend(&entries.get(), DEFAULT_WINDOW_DAYS).balance() {
        BalanceState::Deficit => "var(--bulma-success)",
        BalanceState::Surplus => "#e0699b",
        BalanceState::Maintenance => "var(--bulma-text)",
    };

    // Female-only: menstrual-cycle detection + the de-cycled current weight.
    let cycle_view = move || {
        if profile::get_sex() != Some(Sex::Female) {
            return None;
        }
        let es = entries.get();
        let u = unit.get();
        let ul = unit_label();
        let label = t("weight.cycle.label");
        let inner = match weight_cycle(&es, CYCLE_WINDOW_DAYS) {
            CycleResult::Detected(f) => {
                let latest_kg = es
                    .iter()
                    .max_by(|a, b| a.date.cmp(&b.date))
                    .map(|e| e.weight_kg)
                    .unwrap_or(0.0);
                let decycled = u.from_kg(latest_kg - f.current_deviation_kg);
                let amp = u.from_kg(f.amplitude_kg);
                view! {
                    <p class="is-size-7">
                        <span class="has-text-grey">{format!("{}: ", label)}</span>
                        <span class="has-text-weight-semibold">
                            {format!("~{:.0} {} · ±{:.1} {}", f.period_days, t("weight.cycle.day_short"), amp, ul)}
                        </span>
                    </p>
                    <p class="is-size-7">
                        <span class="has-text-grey">{format!("{}: ", t("weight.cycle.decycled"))}</span>
                        <span class="has-text-weight-semibold">{format!("{:.1} {}", decycled, ul)}</span>
                    </p>
                }
                .into_view()
            }
            CycleResult::NotDetected { .. } => view! {
                <p class="is-size-7">
                    <span class="has-text-grey">{format!("{}: ", label)}</span>
                    <span>{t("weight.cycle.none")}</span>
                </p>
            }
            .into_view(),
            CycleResult::Insufficient { .. } => view! {
                <p class="is-size-7">
                    <span class="has-text-grey">{format!("{}: ", label)}</span>
                    <span>{t("weight.cycle.insufficient")}</span>
                </p>
            }
            .into_view(),
        };
        Some(view! { <div style="margin-top: 6px; text-align: center;">{inner}</div> })
    };

    view! {
        <div style="display: flex; flex-direction: column; gap: 14px; padding: 4px 2px;">
            // Chart + trend + cycle + add button.
            <div>
                <div inner_html=move || chart_svg(&entries.get(), unit.get())></div>
                <p style="margin-top: 10px; text-align: center;">
                    <span class="is-size-7 has-text-grey">{move || format!("{}: ", t("weight.trend.title"))}</span>
                    <span class="is-size-7 has-text-weight-semibold" style:color=trend_color>{trend_text}</span>
                </p>
                {cycle_view}
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

            // History table.
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
        </div>
    }
}

/// Arrow glyph + i18n key for a trend direction.
fn dir_label(direction: Direction) -> (&'static str, &'static str) {
    match direction {
        Direction::Down => ("\u{2193}", "weight.trend.down"),
        Direction::Up => ("\u{2191}", "weight.trend.up"),
    }
}

/// Local HH:MM from an RFC3339 (UTC) timestamp.
fn entry_time(created_at: &str) -> String {
    chrono::DateTime::parse_from_rfc3339(created_at)
        .map(|dt| dt.with_timezone(&chrono::Local).format("%H:%M").to_string())
        .unwrap_or_default()
}
