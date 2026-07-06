use leptos::*;
use leptos_router::*;
use api_types::*;

use crate::services::{local, sync};
use crate::services::i18n::t;

const STANDARD_NUTRIENTS: &[(&str, GoalUnit)] = &[
    ("Calories", GoalUnit::Kcal),
    ("Protein", GoalUnit::G),
    ("Fat", GoalUnit::G),
    ("Carbs", GoalUnit::G),
];

fn is_standard(name: &str) -> bool {
    STANDARD_NUTRIENTS.iter().any(|(n, _)| *n == name)
}

const IOS_BG: &str = "background: var(--bulma-background); min-height: 100vh; padding: 0; margin: -0.75rem;";
const IOS_CARD: &str = "background: var(--bulma-scheme-main); border-radius: 12px; overflow: hidden;";
const IOS_SECTION_LABEL: &str = "text-transform: uppercase; letter-spacing: 0.02em; padding: 24px 0 8px 16px; margin: 0;";
const IOS_ROW: &str = "padding: 12px 16px;";
const SEP: &str = "height: 0.5px; background: var(--bulma-border-weak); margin-left: 16px;";

const CIRCLE_BTN: &str = "display: inline-flex; align-items: center; justify-content: center; width: 22px; height: 22px; background: none; border: none; cursor: pointer; padding: 0; flex-shrink: 0;";

const SEG_ACTIVE: &str = "background: var(--bulma-link); color: var(--bulma-scheme-main); border: none; border-radius: 6px; padding: 3px 10px; font-size: 12px; cursor: pointer;";
const SEG_INACTIVE: &str = "background: transparent; color: var(--bulma-text-strong); border: none; border-radius: 6px; padding: 3px 10px; font-size: 12px; cursor: pointer;";

fn is_track_mode(goal: &Goal) -> bool {
    goal.amount == 0.0
}

fn str_to_unit(s: &str) -> GoalUnit {
    match s {
        "Kcal" => GoalUnit::Kcal,
        "Mg" => GoalUnit::Mg,
        "Mcg" => GoalUnit::Mcg,
        _ => GoalUnit::G,
    }
}

async fn ensure_standard_goals() {
    let all = local::list_goals().await;
    for (name, unit) in STANDARD_NUTRIENTS {
        if !all.iter().any(|g| g.nutrient == *name) {
            let input = CreateGoalInput {
                nutrient: name.to_string(),
                direction: GoalDirection::AtMost,
                amount: 0.0,
                unit: *unit,
                period: GoalPeriod::Day,
            };
            local::create_goal(input).await;
        }
    }
}

#[component]
pub fn GoalsPage() -> impl IntoView {
    let navigate = use_navigate();
    let version = create_rw_signal(0u32);
    let goals_res = create_resource(
        move || version.get(),
        |_| async {
            ensure_standard_goals().await;
            local::list_goals().await
        },
    );
    let goals = move || goals_res.get().unwrap_or_default();
    let invalidate = move || version.update(|v| *v += 1);

    let new_name = create_rw_signal(String::new());
    let new_is_goal = create_rw_signal(false);
    let new_dir = create_rw_signal("AtLeast".to_string());
    let new_amount = create_rw_signal(String::new());
    let new_unit = create_rw_signal("G".to_string());
    let new_period = create_rw_signal("Day".to_string());

    let add_custom = move |_| {
        let name = new_name.get_untracked();
        if name.is_empty() { return; }
        let is_goal = new_is_goal.get_untracked();
        let unit = str_to_unit(&new_unit.get_untracked());
        let direction = match new_dir.get_untracked().as_str() {
            "AtMost" => GoalDirection::AtMost,
            _ => GoalDirection::AtLeast,
        };
        let amount = if is_goal {
            let v: f64 = new_amount.get_untracked().parse().unwrap_or(0.0);
            if v == 0.0 { 1.0 } else { v }
        } else {
            0.0
        };
        let period = match new_period.get_untracked().as_str() {
            "Week" => GoalPeriod::Week,
            "Month" => GoalPeriod::Month,
            _ => GoalPeriod::Day,
        };
        spawn_local(async move {
            let input = CreateGoalInput {
                nutrient: name,
                direction,
                amount,
                unit,
                period,
            };
            local::create_goal(input).await;
            invalidate();
            sync::push_background();
        });
        new_name.set(String::new());
        new_is_goal.set(false);
        new_amount.set(String::new());
        new_dir.set("AtLeast".to_string());
        new_unit.set("G".to_string());
        new_period.set("Day".to_string());
    };

    let save_goal = move |goal: Goal| {
        spawn_local(async move {
            local::update_goal(&goal).await;
            invalidate();
            sync::push_background();
        });
    };

    let delete_goal = move |id: String| {
        spawn_local(async move {
            local::delete_goal(&id).await;
            invalidate();
            sync::push_background();
        });
    };

    view! {
        <div style=IOS_BG>
            // Nav bar
            <div style="display: flex; align-items: center; padding: 12px 16px; background: var(--bulma-background);">
                <button
                    attr:data-testid="goals-btn-back"
                    class="has-text-link is-size-5"
                    style="background: none; border: none; cursor: pointer; padding: 0; display: flex; align-items: center; gap: 4px;"
                    on:click={
                        let nav = navigate.clone();
                        move |_| {
                            let nav = nav.clone();
                            nav("/settings", Default::default());
                        }
                    }
                >
                    {move || t("goals.back")}
                </button>
                <h1 class="is-size-5 has-text-weight-semibold" style="margin: 0 auto;">{move || t("goals.title")}</h1>
                <span class="is-size-5" style="visibility: hidden;">{move || t("goals.back")}</span>
            </div>

            // ---- Standard nutrients (always present, Track mode by default) ----
            <div style="padding: 0 16px;">
                <p class="is-size-7 has-text-grey-light" style=IOS_SECTION_LABEL>{move || t("goals.standard")}</p>
                <div style=IOS_CARD>
                    {move || {
                        let gs = goals();
                        let len = STANDARD_NUTRIENTS.len();
                        STANDARD_NUTRIENTS.iter().enumerate().flat_map(|(i, (name, _unit))| {
                            let goal = gs.iter().find(|g| g.nutrient == *name).cloned();
                            let mut items = Vec::new();
                            items.push(view! {
                                <div style=IOS_ROW>
                                    <div style="display: flex; align-items: center; justify-content: space-between; width: 100%;">
                                        <span
                                            attr:data-testid=format!("goals-nutrient-{}", name.to_lowercase())
                                            class="is-size-6"
                                        >
                                            {move || crate::services::i18n::nutrient_name(name)}
                                        </span>
                                        {if let Some(g) = goal {
                                            goal_controls(g, goals, save_goal, true).into_view()
                                        } else {
                                            view! { <span class="is-size-7 has-text-grey-light">{move || t("goals.mode_track")}</span> }.into_view()
                                        }}
                                    </div>
                                </div>
                            }.into_view());
                            if i < len - 1 {
                                items.push(view! { <div style=SEP></div> }.into_view());
                            }
                            items
                        }).collect::<Vec<_>>()
                    }}
                </div>
            </div>

            // ---- Custom nutrients ----
            <div style="padding: 0 16px;">
                <p class="is-size-7 has-text-grey-light" style=IOS_SECTION_LABEL>{move || t("goals.custom")}</p>
                <div style=IOS_CARD>
                    {move || {
                        let custom: Vec<_> = goals().into_iter().filter(|g| !is_standard(&g.nutrient)).collect();
                        let len = custom.len();
                        custom.into_iter().enumerate().flat_map(|(i, goal)| {
                            let gid_del = goal.id.clone();
                            let mut items = Vec::new();
                            items.push(view! {
                                <div style=IOS_ROW>
                                    <div style="display: flex; align-items: center; justify-content: space-between; width: 100%;">
                                        <span class="is-size-6 has-text-weight-medium">
                                            {goal.nutrient.clone()}
                                        </span>
                                        <div style="display: flex; align-items: center; gap: 8px;">
                                            {goal_controls(goal, goals, save_goal, false).into_view()}
                                            <button
                                                style=CIRCLE_BTN
                                                on:click=move |_| delete_goal(gid_del.clone())
                                            >
                                                <svg width="22" height="22" viewBox="0 0 22 22" fill="none">
                                                    <circle cx="11" cy="11" r="11" fill="var(--bulma-danger)"/>
                                                    <rect x="5.5" y="10" width="11" height="2" rx="1" fill="var(--bulma-scheme-main)"/>
                                                </svg>
                                            </button>
                                        </div>
                                    </div>
                                </div>
                            }.into_view());
                            if i < len - 1 {
                                items.push(view! { <div style=SEP></div> }.into_view());
                            }
                            items
                        }).collect::<Vec<_>>()
                    }}

                    // Separator before add row (if there are custom goals)
                    {move || if goals().iter().any(|g| !is_standard(&g.nutrient)) {
                        view! { <div style=SEP></div> }.into_view()
                    } else {
                        view! {}.into_view()
                    }}

                    // Add custom goal row
                    <div style=IOS_ROW>
                        <div style="display: flex; align-items: center; justify-content: space-between; width: 100%;">
                            <span class="is-size-6 has-text-weight-medium">
                                <input type="text"
                                    attr:data-testid="goals-input-new-nutrient"
                                    placeholder=move || t("settings.nutrient_placeholder")
                                    class="input is-small"
                                    style="width: 120px; min-width: 0;"
                                    prop:value=move || new_name.get()
                                    on:input=move |ev| new_name.set(event_target_value(&ev))
                                />
                            </span>
                            <div style="display: flex; align-items: center; gap: 8px;">
                                <div style="display: flex; flex-direction: column; gap: 6px; align-items: flex-end;">
                                    // Track / Goal toggle
                                    <div style="display: flex; gap: 2px; background: var(--bulma-background); border-radius: 8px; padding: 2px;">
                                        <button
                                            style=move || if !new_is_goal.get() { SEG_ACTIVE } else { SEG_INACTIVE }
                                            on:click=move |_| new_is_goal.set(false)
                                        >
                                            {move || t("goals.mode_track")}
                                        </button>
                                        <button
                                            style=move || if new_is_goal.get() { SEG_ACTIVE } else { SEG_INACTIVE }
                                            on:click=move |_| new_is_goal.set(true)
                                        >
                                            {move || t("goals.mode_goal")}
                                        </button>
                                    </div>
                                    // Goal settings (only when Goal mode)
                                    {move || if new_is_goal.get() {
                                        view! {
                                            <div style="display: flex; align-items: center; gap: 6px;">
                                                <div class="select is-small">
                                                    <select on:change=move |ev| new_dir.set(event_target_value(&ev))>
                                                        <option value="AtLeast" selected=move || new_dir.get() == "AtLeast">{move || t("settings.not_less")}</option>
                                                        <option value="AtMost" selected=move || new_dir.get() == "AtMost">{move || t("settings.not_more")}</option>
                                                    </select>
                                                </div>
                                                <input type="text" inputmode="decimal" class="input is-small"
                                                    style="width: 5rem;"
                                                    prop:value=move || new_amount.get()
                                                    on:input=move |ev| new_amount.set(event_target_value(&ev))
                                                />
                                                <div class="select is-small">
                                                    <select on:change=move |ev| new_unit.set(event_target_value(&ev))>
                                                        <option value="Kcal" selected=move || new_unit.get() == "Kcal">{move || t("common.unit.kcal")}</option>
                                                        <option value="G" selected=move || new_unit.get() == "G">{move || t("common.unit.g")}</option>
                                                        <option value="Mg" selected=move || new_unit.get() == "Mg">{move || t("common.unit.mg")}</option>
                                                        <option value="Mcg" selected=move || new_unit.get() == "Mcg">{move || t("common.unit.mcg")}</option>
                                                    </select>
                                                </div>
                                                <div class="select is-small">
                                                    <select on:change=move |ev| new_period.set(event_target_value(&ev))>
                                                        <option value="Day" selected=move || new_period.get() == "Day">{move || t("settings.period.day")}</option>
                                                        <option value="Week" selected=move || new_period.get() == "Week">{move || t("settings.period.week")}</option>
                                                        <option value="Month" selected=move || new_period.get() == "Month">{move || t("settings.period.month")}</option>
                                                    </select>
                                                </div>
                                            </div>
                                        }.into_view()
                                    } else {
                                        view! {}.into_view()
                                    }}
                                </div>
                                <button
                                    attr:data-testid="goals-btn-add"
                                    style=CIRCLE_BTN
                                    on:click=add_custom
                                >
                                    <svg width="22" height="22" viewBox="0 0 22 22" fill="none">
                                        <circle cx="11" cy="11" r="11" fill="var(--bulma-success)"/>
                                        <rect x="5.5" y="10" width="11" height="2" rx="1" fill="var(--bulma-scheme-main)"/>
                                        <rect x="10" y="5.5" width="2" height="11" rx="1" fill="var(--bulma-scheme-main)"/>
                                    </svg>
                                </button>
                            </div>
                        </div>
                    </div>
                </div>
            </div>

            <div style="height: 40px;"></div>
        </div>
    }
}

fn goal_controls(
    goal: Goal,
    goals: impl Fn() -> Vec<Goal> + Copy + 'static,
    save_goal: impl Fn(Goal) + Copy + 'static,
    is_std: bool,
) -> impl IntoView {
    let gid = goal.id.clone();
    let gid2 = goal.id.clone();
    let gid3 = goal.id.clone();
    let gid_mode = goal.id.clone();

    view! {
        <div style="display: flex; flex-direction: column; gap: 6px; align-items: flex-end;">
            // Mode toggle: Track / Goal
            <div
                attr:data-testid=format!("goals-mode-{}", goal.nutrient.to_lowercase())
                style="display: flex; gap: 2px; background: var(--bulma-background); border-radius: 8px; padding: 2px;"
            >
                <button
                    style=move || if goals().iter().find(|g| g.id == gid_mode).map(|g| is_track_mode(g)).unwrap_or(true) { SEG_ACTIVE } else { SEG_INACTIVE }
                    on:click={
                        let id = gid.clone();
                        move |_| {
                            if let Some(mut g) = goals().into_iter().find(|g| g.id == id) {
                                g.amount = 0.0;
                                save_goal(g);
                            }
                        }
                    }
                >
                    {move || t("goals.mode_track")}
                </button>
                <button
                    style=move || if goals().iter().find(|g| g.id == gid2).map(|g| !is_track_mode(g)).unwrap_or(false) { SEG_ACTIVE } else { SEG_INACTIVE }
                    on:click={
                        let id = gid3.clone();
                        move |_| {
                            if let Some(mut g) = goals().into_iter().find(|g| g.id == id) {
                                if g.amount == 0.0 {
                                    g.amount = 1.0;
                                    save_goal(g);
                                }
                            }
                        }
                    }
                >
                    {move || t("goals.mode_goal")}
                </button>
            </div>

            // Goal settings (only in Goal mode)
            {
                let goal_id = goal.id.clone();
                move || {
                    let cur = goals().into_iter().find(|g| g.id == goal_id);
                    let Some(cur) = cur else { return view! {}.into_view() };
                    if is_track_mode(&cur) {
                        return view! {}.into_view();
                    }

                    let gid_dir = cur.id.clone();
                    let gid_amt = cur.id.clone();
                    let gid_unit = cur.id.clone();
                    let gid_per = cur.id.clone();
                    let cur_period = cur.period;
                    let cur_dir = cur.direction;
                    let cur_amount = cur.amount;
                    let cur_unit = cur.unit;

                    let unit_label_text = match cur_unit {
                        GoalUnit::Kcal => "common.unit.kcal",
                        GoalUnit::G => "common.unit.g",
                        GoalUnit::Mg => "common.unit.mg",
                        GoalUnit::Mcg => "common.unit.mcg",
                        GoalUnit::Steps => "common.unit.steps",
                    };

                    view! {
                        <div style="display: flex; align-items: center; gap: 6px;">
                            <div class="select is-small">
                                <select
                                    on:change=move |ev| {
                                        let dir = match event_target_value(&ev).as_str() {
                                            "AtMost" => GoalDirection::AtMost,
                                            _ => GoalDirection::AtLeast,
                                        };
                                        if let Some(mut g) = goals().into_iter().find(|g| g.id == gid_dir) {
                                            g.direction = dir;
                                            save_goal(g);
                                        }
                                    }
                                >
                                    <option value="AtLeast" selected=cur_dir == GoalDirection::AtLeast>{move || t("settings.not_less")}</option>
                                    <option value="AtMost" selected=cur_dir == GoalDirection::AtMost>{move || t("settings.not_more")}</option>
                                </select>
                            </div>
                            <input type="text" inputmode="decimal" class="input is-small"
                                style="width: 5rem;"
                                value=format!("{}", cur_amount)
                                on:change=move |ev| {
                                    let val: f64 = event_target_value(&ev).parse().unwrap_or(0.0);
                                    if let Some(mut g) = goals().into_iter().find(|g| g.id == gid_amt) {
                                        g.amount = if val == 0.0 { 1.0 } else { val };
                                        save_goal(g);
                                    }
                                }
                            />
                            {if is_std {
                                view! {
                                    <span class="is-size-7 has-text-grey-light">{move || t(unit_label_text)}</span>
                                }.into_view()
                            } else {
                                view! {
                                    <div class="select is-small">
                                        <select
                                            on:change=move |ev| {
                                                let u = str_to_unit(&event_target_value(&ev));
                                                if let Some(mut g) = goals().into_iter().find(|g| g.id == gid_unit) {
                                                    g.unit = u;
                                                    save_goal(g);
                                                }
                                            }
                                        >
                                            <option value="Kcal" selected=cur_unit == GoalUnit::Kcal>{move || t("common.unit.kcal")}</option>
                                            <option value="G" selected=cur_unit == GoalUnit::G>{move || t("common.unit.g")}</option>
                                            <option value="Mg" selected=cur_unit == GoalUnit::Mg>{move || t("common.unit.mg")}</option>
                                            <option value="Mcg" selected=cur_unit == GoalUnit::Mcg>{move || t("common.unit.mcg")}</option>
                                        </select>
                                    </div>
                                }.into_view()
                            }}
                            <div class="select is-small">
                                <select
                                    on:change=move |ev| {
                                        let per = match event_target_value(&ev).as_str() {
                                            "Week" => GoalPeriod::Week,
                                            "Month" => GoalPeriod::Month,
                                            _ => GoalPeriod::Day,
                                        };
                                        if let Some(mut g) = goals().into_iter().find(|g| g.id == gid_per) {
                                            g.period = per;
                                            save_goal(g);
                                        }
                                    }
                                >
                                    <option value="Day" selected=cur_period == GoalPeriod::Day>{move || t("settings.period.day")}</option>
                                    <option value="Week" selected=cur_period == GoalPeriod::Week>{move || t("settings.period.week")}</option>
                                    <option value="Month" selected=cur_period == GoalPeriod::Month>{move || t("settings.period.month")}</option>
                                </select>
                            </div>
                        </div>
                    }.into_view()
                }
            }
        </div>
    }
}
