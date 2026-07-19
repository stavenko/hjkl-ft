use leptos::*;
use api_types::StepEntry;

use crate::components::mini_chart::bar_block_with_line;
use crate::components::weight_widget::EmptyPrompt;
use crate::services::i18n::t;
use crate::services::{db, local};

const CARD: &str = "background: var(--bulma-scheme-main); border-radius: 12px; padding: 10px 12px; height: 100%; box-sizing: border-box;";

#[component]
pub fn StepsWidget(entries: Signal<Vec<StepEntry>>) -> impl IntoView {
    let last_value = move || {
        let mut es = entries.get();
        es.sort_by(|a, b| a.date.cmp(&b.date));
        match es.last() {
            Some(last) => last.steps.to_string(),
            None => "—".to_string(),
        }
    };

    // The steps planka (a Steps/AtLeast goal), drawn as a horizontal line on the
    // chart. Reactive to `goals` so it appears the moment the activity week sets it.
    let goals_ver = db::version("goals");
    let planka = create_resource(move || goals_ver.get(), |_| async {
        local::steps_goal_amount().await
    });

    view! {
        <div style=CARD>
            {move || {
                if entries.get().len() < 2 {
                    view! { <EmptyPrompt text_key="steps.empty_prompt"/> }.into_view()
                } else {
                    let line = planka.get().flatten();
                    view! {
                        <div style="display: flex; align-items: baseline; justify-content: space-between; margin-bottom: 4px;">
                            <span class="is-size-7 has-text-grey">{move || t("steps.title")}</span>
                            <span class="is-size-6 has-text-weight-semibold">{last_value}</span>
                        </div>
                        <div inner_html=move || chart_svg_steps(&entries.get(), line)></div>
                    }.into_view()
                }
            }}
        </div>
    }
}

/// Chart block (placeholder or real chart) for an unsorted set of step entries,
/// with an optional horizontal planka line.
pub fn chart_svg_steps(entries: &[StepEntry], planka: Option<f64>) -> String {
    let mut es = entries.to_vec();
    es.sort_by(|a, b| a.date.cmp(&b.date));
    let dates: Vec<&str> = es.iter().map(|e| e.date.as_str()).collect();
    let values: Vec<f64> = es.iter().map(|e| e.steps as f64).collect();
    bar_block_with_line(&dates, &values, planka)
}
