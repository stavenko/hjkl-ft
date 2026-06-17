use leptos::*;
use leptos_router::*;

use api_types::{CreateGoalInput, GoalDirection, GoalPeriod, GoalUnit};

use crate::services::{i18n::t, local, sync};

const PAGE_BG: &str = "background: var(--bulma-background); min-height: 100vh; padding: 0; margin: -0.75rem;";
const CARD: &str = "background: var(--bulma-scheme-main); border-radius: 12px; overflow: hidden;";

/// State of the protein goal after attempting to set it on this page.
#[derive(Clone, Copy, PartialEq)]
enum ProteinState {
    Loading,
    /// Goal set; the value (grams) is shown.
    Set(f64),
    /// No weight entries yet — cannot set a goal.
    NeedWeight,
}

#[component]
pub fn StoryCh2ProteinPage() -> impl IntoView {
    let navigate = use_navigate();

    // On open, set the HIDDEN non-track Protein goal from the latest weight.
    // It surfaces in the diary-header gauge automatically; SHOW_GOALS stays false
    // so the user can neither see it on the goals page nor edit it.
    let state = create_rw_signal(ProteinState::Loading);
    spawn_local(async move {
        // Latest weight (list_weight_entries is sorted ascending by date).
        let latest = local::list_weight_entries().await.into_iter().last();
        let Some(latest) = latest else {
            state.set(ProteinState::NeedWeight);
            return;
        };
        // Round 1.2 g/kg UP to the nearest 10 g.
        let target_g = ((1.2 * latest.weight_kg) / 10.0).ceil() * 10.0;

        // Find the standard "Protein" goal (match by nutrient) or create it.
        let existing = local::list_goals().await.into_iter().find(|g| g.nutrient == "Protein");
        match existing {
            Some(mut g) => {
                g.direction = GoalDirection::AtLeast;
                g.amount = target_g; // > 0 => non-track goal with a target
                g.unit = GoalUnit::G;
                g.period = GoalPeriod::Day;
                g.updated_at = chrono::Utc::now().to_rfc3339(); // bump for LWW sync
                local::update_goal(&g).await;
            }
            None => {
                local::create_goal(CreateGoalInput {
                    nutrient: "Protein".to_string(),
                    direction: GoalDirection::AtLeast,
                    amount: target_g,
                    unit: GoalUnit::G,
                    period: GoalPeriod::Day,
                })
                .await;
            }
        }
        sync::push_background();
        state.set(ProteinState::Set(target_g));
    });

    let paragraphs = ["story.ch2.protein.p1", "story.ch2.protein.p2"];
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

            <h1 class="is-size-1 has-text-weight-bold" style="margin: 0 16px 16px 16px;">{move || t("story.ch2.s3")}</h1>

            <div style="padding: 0 16px 8px 16px;">
                {body}
            </div>

            // ---- Protein goal ----
            <div style="padding: 16px 16px 0 16px;">
                <p class="is-size-7 has-text-grey-light" style="text-transform: uppercase; letter-spacing: 0.02em; margin: 0 0 8px 4px;">
                    {move || t("story.ch2.protein.goal_label")}
                </p>
                <div style=CARD>
                    <div style="display: flex; align-items: center; justify-content: center; padding: 18px 16px; text-align: center;">
                        {move || match state.get() {
                            ProteinState::Loading => view! {
                                <span class="is-size-6 has-text-grey">"\u{2026}"</span>
                            }.into_view(),
                            ProteinState::Set(g) => view! {
                                <span class="is-size-3 has-text-weight-bold">
                                    {format!("{} \u{0433}", g.round() as i64)}
                                </span>
                            }.into_view(),
                            ProteinState::NeedWeight => view! {
                                <span class="is-size-6 has-text-grey">{move || t("story.ch2.protein.need_weight")}</span>
                            }.into_view(),
                        }}
                    </div>
                </div>

                {move || match state.get() {
                    ProteinState::Set(_) => view! {
                        <p class="is-size-6 has-text-weight-semibold has-text-success" style="margin-top: 16px;">
                            {move || t("story.ch2.protein.goal_set")}
                        </p>
                    }.into_view(),
                    ProteinState::NeedWeight => view! {
                        <button
                            class="button is-link is-fullwidth is-medium"
                            style="margin-top: 16px;"
                            on:click={ let nav = navigate.clone(); move |_| nav("/weight", Default::default()) }
                        >
                            {move || t("story.ch2.protein.open_weight")}
                        </button>
                    }.into_view(),
                    ProteinState::Loading => view! { <span></span> }.into_view(),
                }}
            </div>

            <div style="height: 40px;"></div>
        </div>
    }
}
