use leptos::*;
use api_types::WeightEntry;

use crate::components::mini_chart::chart_block;
use crate::services::i18n::{t, weight_unit_signal, WeightUnit};

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

    view! {
        <div style=CARD>
            <div style="display: flex; align-items: baseline; justify-content: space-between; margin-bottom: 4px;">
                <span class="is-size-7 has-text-grey">{move || t("weight.widget_title")}</span>
                <span class="is-size-6 has-text-weight-semibold">{last_value}</span>
            </div>
            <div inner_html=move || chart_svg(&entries.get(), unit.get())></div>
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
