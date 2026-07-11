//! Dashboard "progress" widget: appears once the persona is set (alongside the
//! notifications bell). It nudges the user to log a full week of food / weight /
//! steps, shows «X/7» counters, and — once all three reach 7 — offers a button
//! that runs the same first-planka algorithm the story used
//! (`local::calorie_planka_suggestion` → `local::set_calorie_goal`).

use std::cell::RefCell;

use leptos::*;
use leptos_router::use_navigate;

use crate::services::i18n::t;
use crate::services::indicators::{self, IndicatorState};
use crate::services::profile::{self, CourseGoal};
use crate::services::sticky::sticky;
use crate::services::{db, local, sync};

// Process-lifetime caches so re-navigating to the dashboard paints the widget's
// real state on the first frame instead of the 0/7 / "add food" placeholder that
// then snaps to the loaded state (see `services::sticky`).
thread_local! {
    static PLANKA_CACHE: RefCell<Option<Option<f64>>> = const { RefCell::new(None) };
    static HASFOOD_CACHE: RefCell<Option<bool>> = const { RefCell::new(None) };
    static COUNTS_CACHE: RefCell<Option<(u32, u32, u32)>> = const { RefCell::new(None) };
    static INDS_CACHE: RefCell<Option<Option<Vec<(String, IndicatorState)>>>> = const { RefCell::new(None) };
}

const CARD: &str = "background: var(--bulma-scheme-main); border-radius: 16px; \
    padding: 16px; box-sizing: border-box; \
    display: flex; flex-direction: column; gap: 12px;";

// ── Nutrition indicators ─────────────────────────────────────────────────────
// Seven line icons (Lucide, inlined — same line style as the nav) showing how the
// user's food/drink is doing, coloured green / orange / red (grey = no data yet) by
// `services::indicators`. Shown only once there's ≥1 week of diary history.
const IC_BONE: &str = r#"<path d="M17 10c.7-.7 1.69 0 2.5 0a2.5 2.5 0 1 0 0-5 .5.5 0 0 1-.5-.5 2.5 2.5 0 1 0-5 0c0 .81.7 1.8 0 2.5l-7 7c-.7.7-1.69 0-2.5 0a2.5 2.5 0 0 0 0 5c.28 0 .5.22.5.5a2.5 2.5 0 1 0 5 0c0-.81-.7-1.8 0-2.5Z"/>"#;
const IC_FISH: &str = r#"<path d="M6.5 12c.94-3.46 4.94-6 8.5-6 3.56 0 6.06 2.54 7 6-.94 3.47-3.44 6-7 6s-7.56-2.53-8.5-6Z"/><path d="M18 12v.5"/><path d="M16 17.93a9.77 9.77 0 0 1 0-11.86"/><path d="M7 10.67C7 8 5.58 5.97 2.73 5.5c-1 1.5-1 5 .23 6.5-1.24 1.5-1.24 5-.23 6.5C5.58 18.03 7 16 7 13.33"/><path d="M10.46 7.26C10.2 5.88 9.17 4.24 8 3h5.8a2 2 0 0 1 1.98 1.67l.23 1.4"/><path d="m16.01 17.93-.23 1.4A2 2 0 0 1 13.8 21H9.5a5.96 5.96 0 0 0 1.49-3.98"/>"#;
const IC_EGG: &str = r#"<path d="M12 2C8 2 4 8 4 14a8 8 0 0 0 16 0c0-6-4-12-8-12"/>"#;
const IC_DROPLET: &str = r#"<path d="M12 22a7 7 0 0 0 7-7c0-2-1-3.9-3-5.5s-3.5-4-4-6.5c-.5 2.5-2 4.9-4 6.5C6 11.1 5 13 5 15a7 7 0 0 0 7 7z"/>"#;
const IC_HAM: &str = r#"<path d="M13.144 21.144A7.274 10.445 45 1 0 2.856 10.856"/><path d="M13.144 21.144A7.274 4.365 45 0 0 2.856 10.856a7.274 4.365 45 0 0 10.288 10.288"/><path d="M16.565 10.435 18.6 8.4a2.501 2.501 0 1 0 1.65-4.65 2.5 2.5 0 1 0-4.66 1.66l-2.024 2.025"/><path d="m8.5 16.5-1-1"/>"#;
const IC_APPLE: &str = r#"<path d="M12 6.528V3a1 1 0 0 1 1-1"/><path d="M18.237 21A15 15 0 0 0 22 11a6 6 0 0 0-10-4.472A6 6 0 0 0 2 11a15.1 15.1 0 0 0 3.763 10 3 3 0 0 0 3.648.648 5.5 5.5 0 0 1 5.178 0A3 3 0 0 0 18.237 21"/>"#;
const IC_WHEAT: &str = r#"<path d="M2 22 16 8"/><path d="M3.47 12.53 5 11l1.53 1.53a3.5 3.5 0 0 1 0 4.94L5 19l-1.53-1.53a3.5 3.5 0 0 1 0-4.94Z"/><path d="M7.47 8.53 9 7l1.53 1.53a3.5 3.5 0 0 1 0 4.94L9 15l-1.53-1.53a3.5 3.5 0 0 1 0-4.94Z"/><path d="M11.47 4.53 13 3l1.53 1.53a3.5 3.5 0 0 1 0 4.94L13 11l-1.53-1.53a3.5 3.5 0 0 1 0-4.94Z"/><path d="M20 2h2v2a4 4 0 0 1-4 4h-2V6a4 4 0 0 1 4-4Z"/><path d="M11.47 17.47 13 19l-1.53 1.53a3.5 3.5 0 0 1-4.94 0L5 19l1.53-1.53a3.5 3.5 0 0 1 4.94 0Z"/><path d="M15.47 13.47 17 15l-1.53 1.53a3.5 3.5 0 0 1-4.94 0L9 15l1.53-1.53a3.5 3.5 0 0 1 4.94 0Z"/><path d="M19.47 9.47 21 11l-1.53 1.53a3.5 3.5 0 0 1-4.94 0L13 11l1.53-1.53a3.5 3.5 0 0 1 4.94 0Z"/>"#;

/// (stroke, tint background) for an indicator state.
fn state_colors(s: IndicatorState) -> (&'static str, &'static str) {
    match s {
        IndicatorState::Green => ("#1fa463", "rgba(31,164,99,0.15)"),
        IndicatorState::Orange => ("#e8850d", "rgba(232,133,13,0.15)"),
        IndicatorState::Red => ("#e0304f", "rgba(224,48,79,0.15)"),
        IndicatorState::Unknown => ("#9aa0a6", "rgba(154,160,166,0.14)"),
    }
}

/// (icon svg paths, short label) for an indicator key.
fn icon_for(k: &str) -> (&'static str, &'static str) {
    match k {
        "calcium" => (IC_BONE, "Кальций"),
        "omega3" => (IC_FISH, "Омега-3"),
        "eggs" => (IC_EGG, "Яйца"),
        "iron" => (IC_DROPLET, "Железо"),
        "red_meat" => (IC_HAM, "Мясо"),
        "veg_fruit" => (IC_APPLE, "Фр/овощи"),
        _ => (IC_WHEAT, "Клетчатка"),
    }
}

fn indicator(paths: &'static str, label: &'static str, state: IndicatorState) -> impl IntoView {
    let (color, tint) = state_colors(state);
    view! {
        <div style="display: flex; flex-direction: column; align-items: center; gap: 3px; flex: 1; min-width: 0;">
            <div style=format!("width: 38px; height: 38px; border-radius: 50%; background: {tint}; \
                    display: flex; align-items: center; justify-content: center;")>
                <svg xmlns="http://www.w3.org/2000/svg" width="21" height="21" viewBox="0 0 24 24" fill="none"
                    stroke=color stroke-width="2" stroke-linecap="round" stroke-linejoin="round"
                    inner_html=paths></svg>
            </div>
            <span style="font-size: 0.55rem; line-height: 1.1; text-align: center; color: var(--bulma-text-weak);">{label}</span>
        </div>
    }
}

fn indicators_row(states: Vec<(String, IndicatorState)>) -> impl IntoView {
    view! {
        <div style="display: flex; gap: 4px; justify-content: space-between;">
            {states.into_iter().map(|(k, st)| {
                let (paths, label) = icon_for(&k);
                indicator(paths, label, st)
            }).collect_view()}
        </div>
    }
}

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

    // Before the very first food entry we show how to add food instead of counters.
    let has_food = create_resource(
        move || food_ver.get(),
        |_| async { !local::list_diary_dates().await.is_empty() },
    );

    // The planka, once set, flips the widget to its "done" state.
    let goals_ver = db::version("goals");
    let planka = create_resource(move || goals_ver.get(), |_| async { local::calorie_goal_amount().await });

    // Sticky views of the resources: `None` only until the first successful load
    // (→ render nothing), then fresh-or-last-known across navigations.
    let planka_s = move || sticky(&PLANKA_CACHE, planka.get());
    let hasfood_s = move || sticky(&HASFOOD_CACHE, has_food.get());
    let counts_s = move || sticky(&COUNTS_CACHE, counts.get());

    // Nutrition indicators: None until a week of diary history exists; then the 7
    // states. Refreshes when the diary or foods (tags) change.
    let foods_ver = db::version("foods");
    let inds = create_resource(
        move || (food_ver.get(), foods_ver.get()),
        |_| async {
            if indicators::enough_history().await {
                let states: Vec<(String, IndicatorState)> = indicators::compute().await
                    .into_iter().map(|(k, s)| (k.to_string(), s)).collect();
                Some(states)
            } else {
                None
            }
        },
    );
    let inds_s = move || sticky(&INDS_CACHE, inds.get());

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
        {move || {
            // Render nothing until the primary data has loaded ONCE. After that the
            // sticky caches keep these `Some`, so navigating back to the dashboard
            // paints the real state immediately — no 0/7 / "add food" flash.
            let (Some(planka_v), Some(has_food_v), Some((food, weight, steps))) =
                (planka_s(), hasfood_s(), counts_s())
            else {
                return ().into_view();
            };
            view! {
                <div style=CARD>
                    {match planka_v {
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
                        None if !has_food_v => {
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
                                // Documentation-style link (dashed underline) to the "how to
                                // keep the diary" help hub.
                                <div style="text-align: center; margin-top: 8px;">
                                    <a href="/help/diary" class="is-size-7"
                                        style="color: var(--bulma-text-weak); text-decoration: underline; \
                                               text-decoration-style: dashed; text-underline-offset: 3px;">
                                        {move || t("help.link.diary")}
                                    </a>
                                </div>
                            }.into_view()
                        }
                    }}
                    // Nutrition indicators row at the BOTTOM (only after ≥1 week of diary).
                    {move || inds_s().flatten().map(|states| view! {
                        <div style="border-bottom: 0.5px solid var(--bulma-border-weak);"></div>
                        {indicators_row(states)}
                    })}
                </div>
            }.into_view()
        }}
    }
}
