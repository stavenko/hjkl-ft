use leptos::*;
use leptos_router::*;

use api_types::{CreateGoalInput, GoalDirection, GoalPeriod, GoalUnit};

use crate::services::weight_trend::{self, DEFAULT_WINDOW_DAYS};
use crate::services::{i18n::t, local, sync};

const PAGE_BG: &str = "background: var(--bulma-background); min-height: 100vh; padding: 0; margin: -0.75rem;";
const CARD: &str = "background: var(--bulma-scheme-main); border-radius: 12px; overflow: hidden;";

/// Window for the average-calorie computation (last 14 days).
const KCAL_WINDOW_DAYS: i64 = 14;

/// State of the hidden calorie planka after attempting to set it on this page.
#[derive(Clone, Copy, PartialEq)]
enum PlankaState {
    Loading,
    /// Goal set; the value (kcal) is shown.
    Set(f64),
    /// No diary days in the window — cannot compute an average.
    NeedDiary,
}

#[component]
pub fn StoryCh3FatPage() -> impl IntoView {
    let navigate = use_navigate();

    // On open, set the HIDDEN non-track "Calories" goal (the daily planka).
    // avg = average daily effective kcal over the last 14 days that have diary
    // entries; balance from the weight trend; planka = avg in a deficit, else
    // avg * 0.95, rounded to the nearest 10 kcal. It surfaces in the diary-header
    // gauge automatically; SHOW_GOALS stays false so the user can't see or edit it.
    let state = create_rw_signal(PlankaState::Loading);
    spawn_local(async move {
        let Some(avg) = local::avg_daily_kcal(KCAL_WINDOW_DAYS).await else {
            state.set(PlankaState::NeedDiary);
            return;
        };

        let weights = local::list_weight_entries().await;
        let balance = weight_trend::weight_trend(&weights, DEFAULT_WINDOW_DAYS).balance();
        // Deficit -> avg; Maintenance/Surplus -> avg * 0.95; rounded to nearest 10.
        let planka = local::calorie_planka(avg, balance);

        // Find the standard "Calories" goal (match by nutrient) or create it.
        let existing = local::list_goals().await.into_iter().find(|g| g.nutrient == "Calories");
        match existing {
            Some(mut g) => {
                g.direction = GoalDirection::AtMost;
                g.amount = planka; // > 0 => non-track goal with a target
                g.unit = GoalUnit::Kcal;
                g.period = GoalPeriod::Day;
                g.updated_at = chrono::Utc::now().to_rfc3339(); // bump for LWW sync
                local::update_goal(&g).await;
            }
            None => {
                local::create_goal(CreateGoalInput {
                    nutrient: "Calories".to_string(),
                    direction: GoalDirection::AtMost,
                    amount: planka,
                    unit: GoalUnit::Kcal,
                    period: GoalPeriod::Day,
                })
                .await;
            }
        }
        sync::push_background();
        state.set(PlankaState::Set(planka));
    });

    let paragraphs = ["story.ch3.fat.p1", "story.ch3.fat.p2", "story.ch3.fat.p3"];
    let body = paragraphs.iter().map(|&key| view! {
        <p class="is-size-6" style="line-height: 1.55; margin: 0 0 14px 0;">{move || t(key)}</p>
    }).collect_view();

    view! {
        <div style=PAGE_BG>
            <div style="display: flex; align-items: center; padding: 12px 16px;">
                <button
                    style="appearance: none; -webkit-appearance: none; border: none; background: none; cursor: pointer; padding: 4px; font: inherit;"
                    class="is-size-5"
                    on:click={ let nav = navigate.clone(); move |_| nav("/", Default::default()) }
                >
                    <span class="has-text-link">{move || t("common.back")}</span>
                </button>
            </div>

            <h1 class="is-size-1 has-text-weight-bold" style="margin: 0 16px 16px 16px;">{move || t("story.ch3.s1")}</h1>

            <div style="padding: 0 16px 8px 16px;">
                {body}
            </div>

            // ---- Calorie planka ----
            <div style="padding: 16px 16px 0 16px;">
                <p class="is-size-7 has-text-grey-light" style="text-transform: uppercase; letter-spacing: 0.02em; margin: 0 0 8px 4px;">
                    {move || t("story.ch3.fat.goal_label")}
                </p>
                <div style=CARD>
                    <div style="display: flex; align-items: center; justify-content: center; padding: 18px 16px; text-align: center;">
                        {move || match state.get() {
                            PlankaState::Loading => view! {
                                <span class="is-size-6 has-text-grey">"\u{2026}"</span>
                            }.into_view(),
                            PlankaState::Set(k) => view! {
                                <span class="is-size-3 has-text-weight-bold">
                                    {format!("{} {}", k.round() as i64, t("story.ch3.fat.kcal_unit"))}
                                </span>
                            }.into_view(),
                            PlankaState::NeedDiary => view! {
                                <span class="is-size-6 has-text-grey">{move || t("story.ch3.fat.need_diary")}</span>
                            }.into_view(),
                        }}
                    </div>
                </div>

                {move || match state.get() {
                    PlankaState::Set(_) => view! {
                        <p class="is-size-6 has-text-weight-semibold has-text-success" style="margin-top: 16px;">
                            {move || t("story.ch3.fat.goal_set")}
                        </p>
                    }.into_view(),
                    PlankaState::NeedDiary => view! {
                        <button
                            class="button is-link is-fullwidth is-medium"
                            style="margin-top: 16px;"
                            on:click={ let nav = navigate.clone(); move |_| nav("/diary", Default::default()) }
                        >
                            {move || t("story.ch3.fat.open_diary")}
                        </button>
                    }.into_view(),
                    PlankaState::Loading => view! { <span></span> }.into_view(),
                }}
            </div>

            <div style="height: 40px;"></div>
        </div>
    }
}
