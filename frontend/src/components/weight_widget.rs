use leptos::*;
use api_types::WeightEntry;

use crate::components::mini_chart::chart_block;
use crate::services::i18n::{t, weight_unit_signal, WeightUnit};
use crate::services::weight_trend::{weight_trend, BalanceState, DEFAULT_WINDOW_DAYS};

const CARD: &str = "background: var(--bulma-scheme-main); border-radius: 12px; padding: 10px 12px; height: 100%; box-sizing: border-box;";

#[component]
pub fn WeightWidget(entries: Signal<Vec<WeightEntry>>) -> impl IntoView {
    let unit = weight_unit_signal();

    let last_value = move || {
        let mut es = entries.get();
        es.sort_by(|a, b| a.date.cmp(&b.date));
        match es.last() {
            Some(last) => {
                let u = unit.get();
                let val = u.from_kg(last.weight_kg);
                let ul = match u {
                    WeightUnit::Kg => t("weight.unit_kg"),
                    WeightUnit::Lbs => t("weight.unit_lbs"),
                };
                format!("{:.1} {}", val, ul)
            }
            None => "—".to_string(),
        }
    };

    // Colour the current weight by energy balance inferred from the trend:
    // green = deficit (losing), pink = surplus (gaining), default = maintenance.
    let weight_color = move || match weight_trend(&entries.get(), DEFAULT_WINDOW_DAYS).balance() {
        BalanceState::Deficit => "var(--bulma-success)",
        BalanceState::Surplus => "#e0699b",
        BalanceState::Maintenance => "var(--bulma-text)",
    };

    view! {
        <div style=CARD>
            {move || {
                // A line needs at least two points; with fewer, show a prompt + «+»
                // instead of an empty chart.
                if entries.get().len() < 2 {
                    view! { <EmptyPrompt text_key="weight.empty_prompt"/> }.into_view()
                } else {
                    view! {
                        <div style="display: flex; align-items: baseline; justify-content: space-between; margin-bottom: 4px;">
                            <span class="is-size-7 has-text-grey">{move || t("weight.widget_title")}</span>
                            <span attr:data-testid="weight-widget-value" class="is-size-6 has-text-weight-semibold" style:color=weight_color>{last_value}</span>
                        </div>
                        <div inner_html=move || chart_svg(&entries.get(), unit.get())></div>
                    }.into_view()
                }
            }}
        </div>
    }
}

/// Empty-state body for a data widget: a short prompt centred over a round «+»
/// affordance. It's a plain (non-button) element — the surrounding tile is the
/// clickable button that opens the add/chart modal, so nesting buttons is avoided.
#[component]
pub fn EmptyPrompt(text_key: &'static str) -> impl IntoView {
    view! {
        <div style="height: 100%; display: flex; flex-direction: column; align-items: center; \
                    justify-content: center; text-align: center; gap: 10px; padding: 4px;">
            <span class="is-size-7 has-text-grey" style="line-height: 1.35;">{move || t(text_key)}</span>
            <span style="width: 40px; height: 40px; border-radius: 50%; background: var(--bulma-link); \
                         color: #fff; display: flex; align-items: center; justify-content: center; \
                         font-size: 1.6rem; line-height: 1; flex-shrink: 0;">"+"</span>
        </div>
    }
}

/// Chart block (placeholder or real chart) for an unsorted set of weight entries.
pub fn chart_svg(entries: &[WeightEntry], unit: WeightUnit) -> String {
    let mut es = entries.to_vec();
    es.sort_by(|a, b| a.date.cmp(&b.date));
    let dates: Vec<&str> = es.iter().map(|e| e.date.as_str()).collect();
    let values: Vec<f64> = es.iter().map(|e| unit.from_kg(e.weight_kg)).collect();
    chart_block(&dates, &values)
}
