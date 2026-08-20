use leptos::*;
use api_types::WeightEntry;

use crate::services::i18n::{t, weight_unit_signal, WeightUnit};
use crate::services::weight_trend::{weight_trend, BalanceState, DEFAULT_WINDOW_DAYS};

const CARD: &str = "background: var(--bulma-scheme-main); border-radius: 12px; padding: 10px 12px; height: 100%; \
    box-sizing: border-box; display: flex; flex-direction: column; justify-content: center; position: relative;";

/// Подпись плитки — «бабл» по центру у нижнего края, ПОВЕРХ графика.
///
/// Заголовок строкой сверху отнимал бы у графика высоту, а место на дашборде
/// дорого. Здесь подпись ничего не занимает: она лежит на рисунке, а собственный
/// фон и скругление отделяют её от линий под ней.
pub const TILE_LABEL: &str = "position: absolute; left: 50%; bottom: 6px; transform: translateX(-50%); \
    z-index: 2; padding: 2px 10px; border-radius: 999px; background: var(--bulma-scheme-main-bis); \
    color: var(--bulma-text-weak); font-size: 11px; line-height: 1.4; white-space: nowrap; \
    pointer-events: none;";

/// Тот же бабл, но у ВЕРХНЕГО края: там стоит само число — последний вес, среднее
/// за неделю по шагам. Цвет текста задаётся отдельно, поэтому его здесь нет.
pub const TILE_VALUE: &str = "position: absolute; left: 50%; top: 6px; transform: translateX(-50%); \
    z-index: 2; padding: 2px 10px; border-radius: 999px; background: var(--bulma-scheme-main-bis); \
    font-size: 13px; font-weight: 600; line-height: 1.4; white-space: nowrap; pointer-events: none;";

#[component]
pub fn WeightWidget(entries: Signal<Vec<WeightEntry>>) -> impl IntoView {
    let unit = weight_unit_signal();

    // Последняя записанная величина — в единицах, выбранных человеком.
    let last_value = move || {
        let mut es = entries.get();
        es.sort_by(|a, b| a.date.cmp(&b.date));
        match es.last() {
            Some(last) => {
                let u = unit.get();
                let ul = match u {
                    WeightUnit::Kg => t("weight.unit_kg"),
                    WeightUnit::Lbs => t("weight.unit_lbs"),
                };
                format!("{:.1} {}", u.from_kg(last.weight_kg), ul)
            }
            None => "—".to_string(),
        }
    };

    view! {
        <div style=CARD>
            {move || {
                // A line needs at least two points; with fewer, show a prompt + «+»
                // instead of an empty chart.
                if entries.get().len() < 2 {
                    view! { <EmptyPrompt text_key="weight.empty_prompt"/> }.into_view()
                } else {
                    // Последний вес — баблом у верхнего края, цветом линии: число и
                    // рисунок говорят об одном, и цвет их связывает.
                    view! {
                        <div style="flex: 1; min-height: 0;" inner_html=move || chart_svg(&entries.get(), unit.get())></div>
                        <span attr:data-testid="weight-widget-value"
                            style=move || format!("{TILE_VALUE} color: {};", trend_color(&entries.get()))>
                            {last_value}
                        </span>
                        <span style=TILE_LABEL>{move || t("weight.widget_title")}</span>
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
