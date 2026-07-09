//! Dashboard "progress" widget: appears once the persona is set (alongside the
//! notifications bell). It nudges the user to log a full week of food / weight /
//! steps, shows «X/7» counters, and — once all three reach 7 — offers a button
//! that runs the same first-planka algorithm the story used
//! (`local::calorie_planka_suggestion` → `local::set_calorie_goal`).

use leptos::*;
use leptos_router::use_navigate;

use crate::services::i18n::t;
use crate::services::profile::{self, CourseGoal};
use crate::services::{db, local, sync};

const CARD: &str = "background: var(--bulma-scheme-main); border-radius: 16px; \
    padding: 16px; height: 100%; box-sizing: border-box; overflow-y: auto; \
    display: flex; flex-direction: column; gap: 12px;";

#[component]
pub fn ProgressWidget() -> impl IntoView {
    // «X/7» counters refresh when any of the three stores change.
    let food_ver = db::version("diary");
    let weight_ver = db::version("weight_entries");
    let steps_ver = db::version("step_entries");
    let counts = create_resource(
        move || (food_ver.get(), weight_ver.get(), steps_ver.get()),
        |_| async { local::progress_week_counts().await },
    );
    let c = move || counts.get().unwrap_or((0, 0, 0));

    // Before the very first food entry we show how to add food instead of counters.
    let has_food = create_resource(
        move || food_ver.get(),
        |_| async { !local::list_diary_dates().await.is_empty() },
    );

    // The planka, once set, flips the widget to its "done" state.
    let goals_ver = db::version("goals");
    let planka = create_resource(move || goals_ver.get(), |_| async { local::calorie_goal_amount().await });

    let busy = create_rw_signal(false);
    let calculate = move |_| {
        busy.set(true);
        spawn_local(async move {
            if let Some(n) = local::calorie_planka_suggestion().await {
                local::set_calorie_goal(n).await;
                sync::push_background();
            }
            busy.set(false);
        });
    };

    let goal_word = move || match profile::get_goal() {
        CourseGoal::Lose => t("dashboard.progress.word_lose"),
        CourseGoal::Gain => t("dashboard.progress.word_gain"),
        CourseGoal::Maintain => t("dashboard.progress.word_maintain"),
    };

    let counter = move |label_key: &'static str, done: u32| {
        let hit = done >= 7;
        view! {
            <div style="display: flex; align-items: center; justify-content: space-between;">
                <span class="is-size-6">{move || t(label_key)}</span>
                <span class="is-size-6 has-text-weight-semibold"
                    style:color=move || if hit { "var(--bulma-success)" } else { "var(--bulma-text)" }>
                    {format!("{}/7", done.min(7))}
                </span>
            </div>
        }
    };

    view! {
        <div style=CARD>
            {move || match planka.get().flatten() {
                // Already computed → show the resulting daily calorie target.
                Some(n) => view! {
                    <div style="display: flex; flex-direction: column; gap: 6px;">
                        <span class="is-size-6 has-text-grey">{move || t("dashboard.progress.done_title")}</span>
                        <span class="is-size-3 has-text-weight-bold">
                            {format!("{} {}", n.round() as i64, t("dashboard.progress.kcal_day"))}
                        </span>
                        <span class="is-size-7 has-text-grey">{move || t("dashboard.progress.done_hint")}</span>
                    </div>
                }.into_view(),
                // Before the first food entry: explain how to add food + «?».
                None if !has_food.get().unwrap_or(false) => {
                    let go_help = move |_| use_navigate()("/help/food", Default::default());
                    view! {
                        <p class="is-size-6" style="line-height: 1.5; margin: 0;">
                            {move || t("dashboard.progress.help_1")}
                        </p>
                        <p class="is-size-6" style="line-height: 1.5; margin: 0;">
                            {move || t("dashboard.progress.help_2")}
                        </p>
                        <p class="is-size-6" style="line-height: 1.5; margin: 0;">
                            {move || t("dashboard.progress.help_3")}
                        </p>
                        <div style="display: flex; justify-content: center; margin-top: 6px;">
                            <button attr:aria-label="?" on:click=go_help
                                style="width: 44px; height: 44px; border-radius: 50%; border: none; cursor: pointer; \
                                       background: var(--bulma-link); color: #fff; font-size: 1.5rem; \
                                       font-weight: 700; line-height: 1;">
                                "?"
                            </button>
                        </div>
                    }.into_view()
                }
                // Still collecting the week of observations.
                None => {
                    let (food, weight, steps) = c();
                    let all_done = food >= 7 && weight >= 7 && steps >= 7;
                    view! {
                        <p class="is-size-7 has-text-grey" style="line-height: 1.45; margin: 0;">
                            {move || t("dashboard.progress.intro").replace("{word}", goal_word())}
                        </p>
                        <div style="display: flex; flex-direction: column; gap: 8px; margin-top: 2px;">
                            {counter("dashboard.progress.nutrition", food)}
                            {counter("weight.widget_title", weight)}
                            {counter("steps.title", steps)}
                        </div>
                        {all_done.then(|| view! {
                            <button class="button is-link is-fullwidth" style="margin-top: 4px;"
                                prop:disabled=move || busy.get()
                                on:click=calculate>
                                {move || t("dashboard.progress.calculate")}
                            </button>
                        })}
                    }.into_view()
                }
            }}
        </div>
    }
}
