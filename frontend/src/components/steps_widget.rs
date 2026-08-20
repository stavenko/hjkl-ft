use leptos::*;
use api_types::StepEntry;

use crate::components::mini_chart::steps_bar_block;
use crate::components::weight_widget::EmptyPrompt;
use crate::services::{db, local};

const CARD: &str = "background: var(--bulma-scheme-main); border-radius: 12px; padding: 10px 12px; height: 100%; \
    box-sizing: border-box; display: flex; flex-direction: column; justify-content: center;";

#[component]
pub fn StepsWidget(entries: Signal<Vec<StepEntry>>) -> impl IntoView {
    // ИСТОРИЯ планок, а не одна нынешняя: планка меняется по неделям, и прошлые дни
    // надо судить той, что действовала тогда. Читается из журнала планок, поэтому
    // ресурсом — и обновляется, когда журнал пополнился.
    let planka_ver = db::version("planka_history");
    let plankas = create_local_resource(
        move || planka_ver.get(),
        |_| async { local::planka_history(local::PLANKA_STEPS).await },
    );

    view! {
        <div style=CARD>
            {move || {
                if entries.get().len() < 2 {
                    view! { <EmptyPrompt text_key="steps.empty_prompt"/> }.into_view()
                } else {
                    let history: Vec<api_types::PlankaEntry> = plankas.get().unwrap_or_default();
                    // Ни подписи «Шаги», ни сегодняшнего числа: на плитке говорят
                    // столбики и линия планок. Числа — в раскрытой панели.
                    view! {
                        <div style="flex: 1; min-height: 0;" inner_html=move || chart_svg_steps(&entries.get(), &history)></div>
                    }.into_view()
                }
            }}
        </div>
    }
}

/// Планка, действовавшая в каждый из этих дней: последняя установка не позже дня.
/// `None` там, где планки ещё не было, — такой день не судится.
fn plankas_for(dates: &[&str], history: &[api_types::PlankaEntry]) -> Vec<Option<f64>> {
    dates
        .iter()
        .map(|d| {
            history
                .iter()
                .take_while(|e| e.date.as_str() <= *d)
                .last()
                .map(|e| e.amount)
        })
        .collect()
}

/// Chart block (placeholder or real chart) for an unsorted set of step entries,
/// with an optional horizontal planka line.
pub fn chart_svg_steps(entries: &[StepEntry], history: &[api_types::PlankaEntry]) -> String {
    let mut es = entries.to_vec();
    es.sort_by(|a, b| a.date.cmp(&b.date));
    let dates: Vec<&str> = es.iter().map(|e| e.date.as_str()).collect();
    let values: Vec<f64> = es.iter().map(|e| e.steps as f64).collect();
    let plankas = plankas_for(&dates, history);
    // Плитка на дашборде — половинной высоты, как и у веса.
    steps_bar_block(&dates, &values, &plankas, crate::components::mini_chart::ChartSize::Tile)
}
