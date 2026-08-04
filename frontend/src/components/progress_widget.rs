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
    static INDS_CACHE: RefCell<Option<Vec<(&'static str, IndicatorState)>>> = const { RefCell::new(None) };
    static GAUGES_CACHE: RefCell<Option<Vec<indicators::DailyGauge>>> = const { RefCell::new(None) };
    static GATE_CACHE: RefCell<Option<u32>> = const { RefCell::new(None) };
    static STEPS_GATE_CACHE: RefCell<Option<u32>> = const { RefCell::new(None) };
    static CALCIUM_GATE_CACHE: RefCell<Option<u32>> = const { RefCell::new(None) };
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
const IC_FLAME: &str = r#"<path d="M8.5 14.5A2.5 2.5 0 0 0 11 12c0-1.38-.5-2-1-3-1.072-2.143-.224-4.054 2-6 .5 2.5 2 4.9 4 6.5 2 1.6 3 3.5 3 5.5a7 7 0 1 1-14 0c0-1.153.433-2.294 1-3a2.5 2.5 0 0 0 2.5 2.5z"/>"#;
const IC_WHEAT: &str = r#"<path d="M2 22 16 8"/><path d="M3.47 12.53 5 11l1.53 1.53a3.5 3.5 0 0 1 0 4.94L5 19l-1.53-1.53a3.5 3.5 0 0 1 0-4.94Z"/><path d="M7.47 8.53 9 7l1.53 1.53a3.5 3.5 0 0 1 0 4.94L9 15l-1.53-1.53a3.5 3.5 0 0 1 0-4.94Z"/><path d="M11.47 4.53 13 3l1.53 1.53a3.5 3.5 0 0 1 0 4.94L13 11l-1.53-1.53a3.5 3.5 0 0 1 0-4.94Z"/><path d="M20 2h2v2a4 4 0 0 1-4 4h-2V6a4 4 0 0 1 4-4Z"/><path d="M11.47 17.47 13 19l-1.53 1.53a3.5 3.5 0 0 1-4.94 0L5 19l1.53-1.53a3.5 3.5 0 0 1 4.94 0Z"/><path d="M15.47 13.47 17 15l-1.53 1.53a3.5 3.5 0 0 1-4.94 0L9 15l1.53-1.53a3.5 3.5 0 0 1 4.94 0Z"/><path d="M19.47 9.47 21 11l-1.53 1.53a3.5 3.5 0 0 1-4.94 0L13 11l1.53-1.53a3.5 3.5 0 0 1 4.94 0Z"/>"#;
// Lucide "beef" — the protein indicator.
const IC_BEEF: &str = r#"<circle cx="12.5" cy="8.5" r="2.5"/><path d="M12.5 2a6.5 6.5 0 0 0-6.22 4.6c-1.1 3.13-.78 3.9-3.18 6.08A3 3 0 0 0 5 18c4 0 8.4-1.8 11.4-4.3A6.5 6.5 0 0 0 12.5 2Z"/><path d="m18.5 6 2.19 4.5a6.48 6.48 0 0 1 .31 2 6.49 6.49 0 0 1-2.6 5.2C15.4 20.2 11 22 7 22a3 3 0 0 1-2.68-1.66L2.4 16.5"/>"#;
// Lucide "footprints" — the steps (activity) indicator.
const IC_STEPS: &str = r#"<path d="M4 16v-2.38C4 11.5 2.97 10.5 3 8c.03-2.72 1.49-6 4.5-6C9.37 2 10 3.8 10 5.5c0 3.11-2 5.66-2 8.68V16a2 2 0 1 1-4 0Z"/><path d="M20 20v-2.38c0-2.12 1.03-3.12 1-5.62-.03-2.72-1.49-6-4.5-6C14.63 6 14 7.8 14 9.5c0 3.11 2 5.66 2 8.68V20a2 2 0 1 0 4 0Z"/><path d="M16 17h4"/><path d="M4 13h4"/>"#;

/// (stroke, tint background) for an indicator state.
pub fn state_colors(s: IndicatorState) -> (&'static str, &'static str) {
    match s {
        IndicatorState::Green => ("#1fa463", "rgba(31,164,99,0.15)"),
        IndicatorState::Orange => ("#e8850d", "rgba(232,133,13,0.15)"),
        IndicatorState::Red => ("#e0304f", "rgba(224,48,79,0.15)"),
        IndicatorState::Unknown => ("#9aa0a6", "rgba(154,160,166,0.14)"),
    }
}

/// (icon svg paths, short label) for an indicator key.
pub fn icon_for(k: &str) -> (&'static str, &'static str) {
    match k {
        "calories" => (IC_FLAME, "Калории"),
        "protein" => (IC_BEEF, "Белок"),
        "calcium" => (IC_BONE, "Кальций"),
        "omega3" => (IC_FISH, "Омега-3"),
        "eggs" => (IC_EGG, "Яйца"),
        "iron" => (IC_DROPLET, "Железо"),
        "red_meat" => (IC_HAM, "Мясо"),
        "veg_fruit" => (IC_APPLE, "Фр/овощи"),
        "steps" => (IC_STEPS, "Шаги"),
        "fiber" => (IC_WHEAT, "Клетчатка"),
        // No silent fallback: an unmapped key is a bug (e.g. a new indicator added
        // without an icon), so fail loudly instead of mislabeling it as fiber.
        _ => panic!("icon_for: no icon/label for indicator key {k:?}"),
    }
}

fn indicator(paths: &'static str, label: &'static str, state: IndicatorState) -> impl IntoView {
    let (color, tint) = state_colors(state);
    view! {
        <div attr:data-ind=label style="display: flex; flex-direction: column; align-items: center; gap: 3px; flex: 1; min-width: 0;">
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

fn indicators_row(states: Vec<(&'static str, IndicatorState)>) -> impl IntoView {
    view! {
        <div style="display: flex; gap: 4px; justify-content: space-between;">
            {states.into_iter().map(|(k, st)| {
                let (paths, label) = icon_for(k);
                indicator(paths, label, st)
            }).collect_view()}
        </div>
    }
}

/// Short label under a daily gauge.
fn gauge_label(key: &str) -> &'static str {
    match key {
        "protein" => "Белок",
        "veg_fruit" => "Фр/овощи",
        "steps" => "Шаги",
        "calcium" => "Кальций",
        "fiber" => "Клетчатка",
        _ => panic!("gauge_label: no label for gauge key {key:?}"),
    }
}

/// Grid of daily-nutrient bars (protein, veg/fruit, calcium, iron, fiber), two
/// per row so they fit vertically (calories stay full-width above). Each fills
/// toward its per-day target; the bar is the indicator's colour, or grey while
/// the metric has no data yet.
fn daily_gauges_grid(
    gauges: Vec<indicators::DailyGauge>,
    iron: Option<crate::services::iron::WeeklyIron>,
) -> impl IntoView {
    view! {
        <div style="display: grid; grid-template-columns: repeat(2, 1fr); gap: 12px 14px;">
            {gauges.into_iter().map(|g| {
                // At-least goals: neutral until met, green when met (bar + value).
                let (bar, val) = crate::components::gauge::at_least_colors(g.value, g.target);
                view! {
                    <crate::components::gauge::Gauge
                        value=g.value target=g.target
                        label=gauge_label(g.key).to_string()
                        unit=g.unit.to_string()
                        color=bar.to_string()
                        value_color=val.map(String::from)/>
                }
            }).collect_view()}
            // Недельное железо — последней ячейкой, чтобы встать напротив кальция.
            {iron.map(weekly_iron_gauge)}
        </div>
    }
}

/// Недельный gauge по железу — ЯЧЕЙКА той же сетки, что и дневные полосы, поэтому
/// он встаёт напротив кальция и занимает ровно половину ширины.
///
/// Шесть точек делят полосу на семь суточных отрезков; горят те, чьи дни уже
/// прошли. День 3 → два дня позади → две горящие точки, и норма «на сейчас» —
/// 2/7 недельной. На первом дне не горит ничего: должок ещё не набежал. Номер дня
/// отдельной подписью не выводим — его показывают сами точки.
fn weekly_iron_gauge(w: crate::services::iron::WeeklyIron) -> impl IntoView {
    let (bar, val) = crate::components::gauge::at_least_colors(w.absorbed_mg, w.target_mg);
    let pace = crate::components::gauge::GaugePace {
        segments: 7,
        passed: w.day_of_week.saturating_sub(1),
    };
    view! {
        <crate::components::gauge::Gauge
            value=w.absorbed_mg target=w.target_mg
            label="Железо/нед".to_string()
            unit="мг".to_string()
            color=bar.to_string()
            height=12.0
            decimals=1
            pace=Some(pace)
            value_color=val.map(String::from)/>
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

    // Calories eaten TODAY (for the done-state gauge). Refreshes on diary edits.
    let today_kcal = create_resource(
        move || food_ver.get(),
        |_| async {
            let today = chrono::Local::now().format("%Y-%m-%d").to_string();
            local::kcal_on(&today).await
        },
    );

    // Sticky views of the resources: `None` only until the first successful load
    // (→ render nothing), then fresh-or-last-known across navigations.
    let planka_s = move || sticky(&PLANKA_CACHE, planka.get());
    let hasfood_s = move || sticky(&HASFOOD_CACHE, has_food.get());
    let counts_s = move || sticky(&COUNTS_CACHE, counts.get());

    // Nutrition indicators (consistency over time): the states for the currently
    // UNLOCKED indicators, read through the per-day cache. Async — `None` until the
    // aggregate resolves, so the row paints grey first, then colours in. Refreshes
    // when the diary, foods (tags/nutrients) or weight (protein target) change.
    let foods_ver = db::version("foods");
    let inds = create_local_resource(
        move || (food_ver.get(), foods_ver.get(), weight_ver.get()),
        |_| async { indicators::unlocked_indicator_states().await },
    );
    let inds_s = move || sticky(&INDS_CACHE, inds.get());

    // Daily-nutrient gauges (today's amount vs each per-day target). Depends on the
    // diary, the foods (nutrient values / tags) and the latest weight (protein
    // target is derived from fat-free mass). Grey until data appears per metric.
    let gauges = create_local_resource(
        move || (food_ver.get(), foods_ver.get(), weight_ver.get()),
        |_| async { indicators::daily_gauges().await },
    );
    let gauges_s = move || sticky(&GAUGES_CACHE, gauges.get());

    // "Keep them green" gate: GREEN days accrued toward the required week. Same
    // dependencies as the indicators (they drive the per-day green check).
    let gate = create_local_resource(
        move || (food_ver.get(), foods_ver.get(), weight_ver.get()),
        |_| async { indicators::green_gate_progress().await },
    );
    let gate_s = move || sticky(&GATE_CACHE, gate.get());

    // The activity-week (steps) gate: GREEN steps-days accrued toward its own week.
    // Depends on step logging and the steps planka goal.
    let steps_gate = create_local_resource(
        move || (steps_ver.get(), goals_ver.get()),
        |_| async { indicators::steps_gate_progress().await },
    );
    let steps_gate_s = move || sticky(&STEPS_GATE_CACHE, steps_gate.get());

    // Недельное железо (усвоенные мг за текущую неделю железа против нормы).
    // Зависит от дневника и продуктов — коэффициент усвоения приезжает фоновым
    // проходом по железу.
    let iron_week = create_local_resource(
        move || (food_ver.get(), foods_ver.get()),
        |_| async { crate::services::iron::weekly_progress().await },
    );

    // The calcium-week gate: GREEN calcium-days accrued toward its own week. Depends
    // on food/nutrient data (calcium comes from enriched foods) and the goals store.
    let calcium_gate = create_local_resource(
        move || (food_ver.get(), foods_ver.get(), goals_ver.get()),
        |_| async { indicators::calcium_gate_progress().await },
    );
    let calcium_gate_s = move || sticky(&CALCIUM_GATE_CACHE, calcium_gate.get());

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

    // AUTO first planka: the moment the observation week completes (7/7/7 across
    // food/weight/steps) and no planka exists yet, compute and set it — no button.
    // Setting the goal bumps the goals version, so the widget flips to the gauge +
    // indicators + first gate by itself. `auto_tried` keeps a failed attempt from
    // looping; the in-flight re-check of the goal guards against a double set.
    let auto_tried = create_rw_signal(false);
    create_effect(move |_| {
        let all_done = matches!(counts.get(), Some((f, w, s)) if f >= 7 && w >= 7 && s >= 7);
        let no_planka = matches!(planka.get(), Some(None));
        if all_done && no_planka && !busy.get_untracked() && !auto_tried.get_untracked() {
            auto_tried.set(true);
            busy.set(true);
            spawn_local(async move {
                if local::calorie_goal_amount().await.is_none() {
                    if let Some(n) = local::calorie_planka_suggestion().await {
                        local::set_calorie_goal(n).await;
                        sync::push_background();
                        // Announce the FIRST planka with an inbox letter — same
                        // channel as the weekly recompute and the curator's
                        // set_planka, so the user learns the number even if they
                        // miss the widget flipping.
                        crate::services::letters::add(crate::services::letters::Letter {
                            id: format!("planka-first-{}", chrono::Local::now().format("%Y-%m-%d")),
                            created_at: chrono::Local::now().to_rfc3339(),
                            body: format!(
                                "Неделя наблюдений завершена — мы рассчитали вашу первую планку по калориям: {n:.0} ккал.\n\n\
                                 Старайтесь не выходить за неё: индикатор на главном экране показывает, сколько калорий осталось на сегодня. Каждую неделю мы будем пересчитывать планку автоматически.\n\n\
                                 Это письмо — просто уведомление.",
                            ),
                            read: false,
                        });
                    }
                }
                busy.set(false);
            });
        }
    });
    // Raised when the course goal changes → the calorie planka no longer fits, so
    // the widget prompts a recompute instead of showing the (stale) calorie gauge.
    let planka_stale = local::planka_stale_signal();

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
                <div attr:data-testid="progress-widget" style=CARD>
                    {match planka_v {
                        // Planka computed → a calorie GAUGE (eaten today / target),
                        // in place of the old plain number. Green while under the
                        // target, red once over (it's an «at most» goal).
                        Some(n) => {
                            let calorie = if planka_stale.get() {
                                // Goal changed → the planka is out of date. Show a prompt +
                                // recompute button in place of the calorie gauge.
                                view! {
                                    <div style="display: flex; flex-direction: column; gap: 10px;">
                                        <span class="is-size-7 has-text-grey has-text-weight-medium">
                                            {move || t("dashboard.progress.done_title")}
                                        </span>
                                        <p class="is-size-7" style="margin: 0; line-height: 1.4;">
                                            {move || t("dashboard.progress.recalc_needed")}
                                        </p>
                                        <button class="button is-link is-fullwidth"
                                            prop:disabled=move || busy.get()
                                            on:pointerup=|ev: web_sys::PointerEvent| ev.stop_propagation()
                                            on:click=calculate>
                                            {move || t("dashboard.progress.recalc")}
                                        </button>
                                    </div>
                                }.into_view()
                            } else {
                                let eaten = today_kcal.get().unwrap_or(0.0);
                                let color = if eaten > n { "#e0304f" } else { "#1fa463" }.to_string();
                                view! {
                                    <div style="display: flex; flex-direction: column; gap: 10px;">
                                        <span class="is-size-7 has-text-grey has-text-weight-medium">
                                            {move || t("dashboard.progress.done_title")}
                                        </span>
                                        <crate::components::gauge::Gauge
                                            value=eaten target=n
                                            label=t("dashboard.calories_title").to_string()
                                            unit=t("common.unit.kcal").to_string()
                                            color=color height=12.0
                                            value_color={(eaten > n).then(|| "#e0304f".to_string())}/>
                                    </div>
                                }.into_view()
                            };
                            view! {
                                {calorie}
                                // Daily-nutrient bars below the calorie one.
                                {move || gauges_s().map(|g| daily_gauges_grid(g, iron_week.get().flatten()))}
                            }.into_view()
                        },
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
                                        on:pointerup=|ev: web_sys::PointerEvent| ev.stop_propagation()
                                        style="width: 44px; height: 44px; border-radius: 50%; border: none; cursor: pointer; \
                                               background: var(--bulma-link); color: #fff; font-size: 1.5rem; \
                                               font-weight: 700; line-height: 1;">
                                        "?"
                                    </button>
                                </div>
                            }.into_view()
                        }
                        // Still collecting the week of observations. When the week
                        // completes, the auto-planka effect above computes the planka
                        // and this branch flips to the gauge state on its own — the
                        // completed state shows a brief spinner, not a button.
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
                                    <div style="display: flex; justify-content: center; padding: 6px 0 2px;">
                                        <div class="ft-spinner"></div>
                                    </div>
                                })}
                                // Documentation-style link (dashed underline) to the "how to
                                // keep the diary" help hub.
                                <div style="text-align: center; margin-top: 8px;">
                                    <a href="/help/diary" class="is-size-7"
                                        on:pointerup=|ev: web_sys::PointerEvent| ev.stop_propagation()
                                        style="color: var(--bulma-text-weak); text-decoration: underline; \
                                               text-decoration-style: dashed; text-underline-offset: 3px;">
                                        {move || t("help.link.diary")}
                                    </a>
                                </div>
                            }.into_view()
                        }
                    }}
                    // Nutrition indicators (CONSISTENCY over time) as icons at the
                    // bottom — ONLY once the FIRST PLANKA exists (the observation week
                    // is done and «Рассчитать» was pressed). Before that a newcomer
                    // must see just the 7-day counters: the indicators and the «keep
                    // them green» gate open TOGETHER WITH the planka, not with the
                    // first food entry. Drawn from the fixed unlocked list so they
                    // appear GREY immediately, then colour in when the cached
                    // aggregate resolves. Different purpose from the gauges above
                    // (what's still left TODAY), so an overlapping metric in both is
                    // intentional, not a duplicate.
                    {(planka_v.is_some()).then(|| {
                        let states: std::collections::HashMap<&'static str, IndicatorState> =
                            inds_s().unwrap_or_default().into_iter().collect();
                        let mut row: Vec<(&'static str, IndicatorState)> = indicators::displayed_indicators()
                            .into_iter()
                            .map(|k| (k, states.get(k).copied().unwrap_or(IndicatorState::Unknown)))
                            .collect();
                        // Sort left→right by severity: red, then orange, then green
                        // (unknown/grey last). Equal priority within a colour → by name.
                        // At most 7 indicators are drawn.
                        let rank = |s: IndicatorState| match s {
                            IndicatorState::Red => 0,
                            IndicatorState::Orange => 1,
                            IndicatorState::Green => 2,
                            IndicatorState::Unknown => 3,
                        };
                        row.sort_by(|a, b| {
                            rank(a.1).cmp(&rank(b.1)).then_with(|| icon_for(a.0).1.cmp(icon_for(b.0).1))
                        });
                        row.truncate(7);
                        // "Keep them green" gate caption, right before the indicators.
                        // Week-2 gate (protein/veg-fruit) first; once it's cleared and
                        // the activity week is unlocked, the SAME caption tracks the
                        // steps gate. Hidden once the active gate's 7 green days are done.
                        let green = gate_s().unwrap_or(0);
                        let steps_green = steps_gate_s().unwrap_or(0);
                        let calcium_green = calcium_gate_s().unwrap_or(0);
                        // The week-2 caption shows only while the activity week is
                        // still locked: retro-widening the gate set (calories joined
                        // it) must not resurface the caption for users who already
                        // cleared it and moved on to the steps/calcium gates.
                        let active_gate: Option<(&'static str, u32)> =
                            if !indicators::activity_unlocked() && green < indicators::GREEN_GATE_DAYS {
                                Some(("dashboard.progress.gate_title", green))
                            } else if indicators::activity_unlocked()
                                && steps_green < indicators::GREEN_GATE_DAYS
                            {
                                Some(("dashboard.progress.steps_gate_title", steps_green))
                            } else if indicators::calcium_unlocked()
                                && calcium_green < indicators::GREEN_GATE_DAYS
                            {
                                Some(("dashboard.progress.calcium_gate_title", calcium_green))
                            } else {
                                None
                            };
                        let gate_caption = active_gate.map(|(title_key, done)| {
                            // Show DAYS REMAINING (not "5/7", which read ambiguously as
                            // done-or-left). Russian day-word agrees with the number.
                            let left = indicators::GREEN_GATE_DAYS.saturating_sub(done);
                            let word = match crate::services::i18n::get_lang() {
                                crate::services::i18n::Lang::En => if left == 1 { "day" } else { "days" },
                                crate::services::i18n::Lang::Ru => {
                                    let (d10, d100) = (left % 10, left % 100);
                                    if d10 == 1 && d100 != 11 {
                                        "день"
                                    } else if (2..=4).contains(&d10) && !(12..=14).contains(&d100) {
                                        "дня"
                                    } else {
                                        "дней"
                                    }
                                }
                            };
                            let progress = t("dashboard.progress.gate_progress")
                                .replace("{n}", &left.to_string())
                                .replace("{w}", word);
                            view! {
                                <div style="display: flex; flex-direction: column; gap: 2px;">
                                    <span class="is-size-7 has-text-weight-semibold">
                                        {move || t(title_key)}
                                    </span>
                                    <span class="is-size-7 has-text-grey">{progress}</span>
                                </div>
                            }
                        });
                        view! {
                            <div style="border-bottom: 0.5px solid var(--bulma-border-weak);"></div>
                            {gate_caption}
                            {indicators_row(row)}
                        }
                    })}
                </div>
            }.into_view()
        }}
    }
}
