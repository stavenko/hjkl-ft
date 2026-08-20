use leptos::*;
use api_types::WeightEntry;

use crate::services::i18n::{t, weight_unit_signal, WeightUnit};
use crate::services::weight_trend::{weight_trend, BalanceState, DEFAULT_WINDOW_DAYS};

const CARD: &str = "background: var(--bulma-scheme-main); border-radius: 12px; padding: 10px 12px; height: 100%; \
    box-sizing: border-box; display: flex; flex-direction: column; justify-content: center;";

#[component]
pub fn WeightWidget(entries: Signal<Vec<WeightEntry>>) -> impl IntoView {
    let unit = weight_unit_signal();

    view! {
        <div style=CARD>
            {move || {
                // A line needs at least two points; with fewer, show a prompt + «+»
                // instead of an empty chart.
                if entries.get().len() < 2 {
                    view! { <EmptyPrompt text_key="weight.empty_prompt"/> }.into_view()
                } else {
                    // Ни подписи «Вес», ни сегодняшнего числа: на плитке говорит сам
                    // график, а что это вес — видно по нему же. Число доступно в
                    // раскрытой панели, куда плитка и ведёт.
                    view! {
                        <div style="flex: 1; min-height: 0;" inner_html=move || chart_svg(&entries.get(), unit.get())></div>
                    }.into_view()
                }
            }}
        </div>
    }
}

/// Цвет линии веса — направление, а не величина.
///
/// Зелёный — вес снижается, красный — растёт, синий — направление не определено:
/// колебания в пределах шума, и утверждать что-либо нельзя.
fn trend_color(entries: &[api_types::WeightEntry]) -> &'static str {
    match weight_trend(entries, DEFAULT_WINDOW_DAYS).balance() {
        BalanceState::Deficit => "#1fa463",
        BalanceState::Surplus => "#e0304f",
        BalanceState::Maintenance => "var(--bulma-link)",
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
    // Плитка на дашборде — половинной высоты: место там дороже подробности. Цвет
    // линии говорит о направлении, раз числа рядом больше нет.
    let mut es = entries.to_vec();
    es.sort_by(|a, b| a.date.cmp(&b.date));
    let dates: Vec<&str> = es.iter().map(|e| e.date.as_str()).collect();
    let values: Vec<f64> = es.iter().map(|e| unit.from_kg(e.weight_kg)).collect();
    crate::components::mini_chart::chart_block_coloured(
        &dates,
        &values,
        &[],
        crate::components::mini_chart::ChartSize::Tile,
        trend_color(entries),
    )
}

/// То же, плюс история КАЛОРИЙНОЙ планки поверх — по одному значению на точку веса
/// (в том же порядке, что и отсортированные записи). Планка нормируется: величины
/// несопоставимы, читается форма изменения — см. `mini_chart::chart_block_with_planka`.
pub fn chart_svg_with_planka(
    entries: &[WeightEntry],
    unit: WeightUnit,
    planka: &[Option<f64>],
) -> String {
    chart_svg_sized(entries, unit, planka, crate::components::mini_chart::ChartSize::Full)
}

/// То же, с явным размером: на дашборде график вдвое ниже и без дат, в раскрытой
/// панели — прежний, с подписями.
pub fn chart_svg_sized(
    entries: &[WeightEntry],
    unit: WeightUnit,
    planka: &[Option<f64>],
    size: crate::components::mini_chart::ChartSize,
) -> String {
    let mut es = entries.to_vec();
    es.sort_by(|a, b| a.date.cmp(&b.date));
    let dates: Vec<&str> = es.iter().map(|e| e.date.as_str()).collect();
    let values: Vec<f64> = es.iter().map(|e| unit.from_kg(e.weight_kg)).collect();
    crate::components::mini_chart::chart_block_with_planka(&dates, &values, planka, size)
}
