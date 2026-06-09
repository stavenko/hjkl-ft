use leptos::*;
use api_types::*;

use crate::services::{local, sync};
use crate::services::i18n::t;
use crate::pages::pair_page::PairPageLoggedIn;

const STANDARD_NUTRIENTS: &[(&str, &str)] = &[
    ("Calories", "kcal"),
    ("Protein", "g"),
    ("Fat", "g"),
    ("Carbs", "g"),
];

fn is_standard(name: &str) -> bool {
    STANDARD_NUTRIENTS.iter().any(|(n, _)| *n == name)
}

#[component]
pub fn SettingsPage() -> impl IntoView {
    let show_pair = create_rw_signal(false);
    let version = create_rw_signal(0u32);
    let goals_res = create_resource(
        move || version.get(),
        |_| async { local::list_goals().await },
    );
    let goals = move || goals_res.get().unwrap_or_default();
    let invalidate = move || version.update(|v| *v += 1);

    // New custom goal form
    let new_name = create_rw_signal(String::new());
    let new_unit = create_rw_signal("G".to_string());

    let add_custom = move |_| {
        let name = new_name.get_untracked();
        if name.is_empty() { return; }
        let unit = match new_unit.get_untracked().as_str() {
            "Mg" => GoalUnit::Mg,
            "Mcg" => GoalUnit::Mcg,
            _ => GoalUnit::G,
        };
        spawn_local(async move {
            let input = CreateGoalInput {
                nutrient: name,
                direction: GoalDirection::AtLeast,
                amount: 0.0,
                unit,
                period: GoalPeriod::Day,
            };
            local::create_goal(input).await;
            invalidate();
            sync::push_background();
        });
        new_name.set(String::new());
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

    let toggle_standard = move |nutrient: &str, unit_str: &str| {
        let nutrient = nutrient.to_string();
        let unit_str = unit_str.to_string();
        spawn_local(async move {
            let all = local::list_goals().await;
            if let Some(existing) = all.iter().find(|g| g.nutrient == nutrient) {
                local::delete_goal(&existing.id).await;
            } else {
                let unit = if unit_str == "kcal" { GoalUnit::Kcal } else { GoalUnit::G };
                let input = CreateGoalInput {
                    nutrient,
                    direction: GoalDirection::AtMost,
                    amount: 0.0,
                    unit,
                    period: GoalPeriod::Day,
                };
                local::create_goal(input).await;
            }
            invalidate();
            sync::push_background();
        });
    };

    view! {
        {move || if show_pair.get() {
            view! {
                <PairPageLoggedIn on_close=Callback::new(move |_| show_pair.set(false)) />
            }.into_view()
        } else {
            view! {
                <div>
                    <h1 class="title is-4 mb-5">{t("settings.title")}</h1>

                    <section>
                        <h2 class="subtitle is-5 mb-4">{t("settings.goals")}</h2>

                        // Standard KBJU — toggle on/off, inline edit when enabled
                        {move || {
                            let gs = goals();
                            STANDARD_NUTRIENTS.iter().map(|(name, unit)| {
                                let existing = gs.iter().find(|g| g.nutrient == *name).cloned();
                                let is_on = existing.is_some();
                                let name_s = name.to_string();
                                let unit_s = unit.to_string();
                                view! {
                                    <div style="display: flex; align-items: center; gap: 0.5rem; padding: 0.4rem 0; border-bottom: 1px solid #f0f0f0;">
                                        <label class="checkbox" style="width: 7rem; flex-shrink: 0; display: flex; align-items: center; gap: 0.25rem; white-space: nowrap;">
                                            <input type="checkbox"
                                                attr:data-testid=format!("settings-checkbox-{}", name.to_lowercase())
                                                prop:checked=is_on
                                                on:change={
                                                    let n = name_s.clone();
                                                    let u = unit_s.clone();
                                                    move |_| toggle_standard(&n, &u)
                                                }
                                            />
                                            {crate::services::i18n::nutrient_name(name)}
                                        </label>
                                        {if let Some(goal) = existing {
                                            let gid = goal.id.clone();
                                            let gid2 = goal.id.clone();
                                            let gid3 = goal.id.clone();
                                            let g_nutrient = goal.nutrient.clone();
                                            let g_key = goal.key.clone();
                                            let g_unit = goal.unit;
                                            let g_created = goal.created_at.clone();
                                            view! {
                                                <div class="select is-small">
                                                    <select
                                                        on:change={
                                                            let id = gid.clone();
                                                            let n = g_nutrient.clone();
                                                            let k = g_key.clone();
                                                            let u = g_unit;
                                                            let cr = g_created.clone();
                                                            move |ev| {
                                                                let dir = match event_target_value(&ev).as_str() {
                                                                    "AtMost" => GoalDirection::AtMost,
                                                                    _ => GoalDirection::AtLeast,
                                                                };
                                                                let cur = goals().into_iter().find(|g| g.id == id).unwrap();
                                                                save_goal(Goal { id: id.clone(), nutrient: n.clone(), key: k.clone(), direction: dir, amount: cur.amount, unit: u, period: cur.period, created_at: cr.clone(), updated_at: String::new() });
                                                            }
                                                        }
                                                    >
                                                        <option value="AtLeast" selected=goal.direction == GoalDirection::AtLeast>{t("settings.not_less")}</option>
                                                        <option value="AtMost" selected=goal.direction == GoalDirection::AtMost>{t("settings.not_more")}</option>
                                                    </select>
                                                </div>
                                                <input type="text" inputmode="decimal" class="input is-small"
                                                    style="width: 5rem;"
                                                    value=format!("{}", goal.amount)
                                                    on:change={
                                                        let id = gid3.clone();
                                                        move |ev| {
                                                            let val: f64 = event_target_value(&ev).parse().unwrap_or(0.0);
                                                            if let Some(mut g) = goals().into_iter().find(|g| g.id == id) {
                                                                g.amount = val;
                                                                save_goal(g);
                                                            }
                                                        }
                                                    }
                                                />
                                                <span class="is-size-7 has-text-grey">{crate::services::i18n::unit_label(unit)}</span>
                                                <div class="select is-small">
                                                    {
                                                        let gid_per = goal.id.clone();
                                                        let cur_period = goal.period;
                                                        view! {
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
                                                                <option value="Day" selected=cur_period == GoalPeriod::Day>{t("settings.period.day")}</option>
                                                                <option value="Week" selected=cur_period == GoalPeriod::Week>{t("settings.period.week")}</option>
                                                                <option value="Month" selected=cur_period == GoalPeriod::Month>{t("settings.period.month")}</option>
                                                            </select>
                                                        }
                                                    }
                                                </div>
                                            }.into_view()
                                        } else {
                                            view! { <span class="is-size-7 has-text-grey">{t("settings.off")}</span> }.into_view()
                                        }}
                                    </div>
                                }
                            }).collect::<Vec<_>>()
                        }}

                        // Custom goals — editable inline rows
                        {move || {
                            goals().into_iter().filter(|g| !is_standard(&g.nutrient)).map(|goal| {
                                let gid = goal.id.clone();
                                let gid2 = goal.id.clone();
                                let gid3 = goal.id.clone();
                                let gid4 = goal.id.clone();
                                let gid_del = goal.id.clone();
                                view! {
                                    <div style="display: flex; align-items: center; gap: 0.5rem; padding: 0.4rem 0; border-bottom: 1px solid #f0f0f0;">
                                        <span class="is-size-7 has-text-weight-medium" style="width: 7rem; flex-shrink: 0;">{&goal.nutrient}</span>
                                        <div class="select is-small">
                                            <select
                                                on:change={
                                                    let id = gid2.clone();
                                                    move |ev| {
                                                        let dir = match event_target_value(&ev).as_str() {
                                                            "AtMost" => GoalDirection::AtMost,
                                                            _ => GoalDirection::AtLeast,
                                                        };
                                                        if let Some(mut g) = goals().into_iter().find(|g| g.id == id) {
                                                            g.direction = dir;
                                                            save_goal(g);
                                                        }
                                                    }
                                                }
                                            >
                                                <option value="AtLeast" selected=goal.direction == GoalDirection::AtLeast>{t("settings.not_less")}</option>
                                                <option value="AtMost" selected=goal.direction == GoalDirection::AtMost>{t("settings.not_more")}</option>
                                            </select>
                                        </div>
                                        <input type="text" inputmode="decimal" class="input is-small"
                                            style="width: 5rem;"
                                            value=format!("{}", goal.amount)
                                            on:change={
                                                let id = gid4.clone();
                                                move |ev| {
                                                    let val: f64 = event_target_value(&ev).parse().unwrap_or(0.0);
                                                    if let Some(mut g) = goals().into_iter().find(|g| g.id == id) {
                                                        g.amount = val;
                                                        save_goal(g);
                                                    }
                                                }
                                            }
                                        />
                                        <span class="is-size-7 has-text-grey">{crate::services::i18n::unit_label(goal.unit.label())}</span>
                                        <div class="select is-small">
                                            {
                                                let gid_per = goal.id.clone();
                                                let cur_period = goal.period;
                                                view! {
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
                                                        <option value="Day" selected=cur_period == GoalPeriod::Day>{t("settings.period.day")}</option>
                                                        <option value="Week" selected=cur_period == GoalPeriod::Week>{t("settings.period.week")}</option>
                                                        <option value="Month" selected=cur_period == GoalPeriod::Month>{t("settings.period.month")}</option>
                                                    </select>
                                                }
                                            }
                                        </div>
                                        <button
                                            class="button is-ghost has-text-grey-light"
                                            style="height: 2rem; width: 2rem; padding: 0; text-decoration: none;"
                                            on:click=move |_| delete_goal(gid_del.clone())
                                        >
                                            <svg xmlns="http://www.w3.org/2000/svg" width="16" height="16" viewBox="0 0 20 20" fill="currentColor">
                                                <path fill-rule="evenodd" d="M9 2a1 1 0 00-.894.553L7.382 4H4a1 1 0 000 2v10a2 2 0 002 2h8a2 2 0 002-2V6a1 1 0 100-2h-3.382l-.724-1.447A1 1 0 0011 2H9zM7 8a1 1 0 012 0v6a1 1 0 11-2 0V8zm5-1a1 1 0 00-1 1v6a1 1 0 102 0V8a1 1 0 00-1-1z" clip-rule="evenodd" />
                                            </svg>
                                        </button>
                                    </div>
                                }
                            }).collect::<Vec<_>>()
                        }}

                        // Add custom goal
                        <div style="display: flex; align-items: center; gap: 0.5rem; padding: 0.75rem 0;">
                            <input type="text" attr:data-testid="settings-input-new-nutrient" placeholder=t("settings.nutrient_placeholder") class="input is-small"
                                style="width: 8rem;"
                                prop:value=move || new_name.get()
                                on:input=move |ev| new_name.set(event_target_value(&ev))
                            />
                            <div class="select is-small">
                                <select on:change=move |ev| new_unit.set(event_target_value(&ev))>
                                    <option value="G">{t("common.unit.g")}</option>
                                    <option value="Mg">"mg"</option>
                                    <option value="Mcg">"µg"</option>
                                </select>
                            </div>
                            <button attr:data-testid="settings-btn-add-goal" class="button is-small is-link" on:click=add_custom>{t("settings.add")}</button>
                        </div>
                    </section>

                    <section class="mt-6">
                        <h2 class="subtitle is-5 mb-4">{t("settings.language")}</h2>
                        <div class="buttons has-addons">
                            <button
                                attr:data-testid="settings-btn-lang-ru"
                                class=move || if crate::services::i18n::get_lang() == crate::services::i18n::Lang::Ru {
                                    "button is-small is-link is-selected"
                                } else {
                                    "button is-small"
                                }
                                on:click=move |_| {
                                    crate::services::i18n::set_lang(crate::services::i18n::Lang::Ru);
                                    invalidate();
                                }
                            >"Русский"</button>
                            <button
                                attr:data-testid="settings-btn-lang-en"
                                class=move || if crate::services::i18n::get_lang() == crate::services::i18n::Lang::En {
                                    "button is-small is-link is-selected"
                                } else {
                                    "button is-small"
                                }
                                on:click=move |_| {
                                    crate::services::i18n::set_lang(crate::services::i18n::Lang::En);
                                    invalidate();
                                }
                            >"English"</button>
                        </div>
                    </section>

                    <section class="mt-6">
                        <h2 class="subtitle is-5 mb-4">{t("settings.add_device")}</h2>
                        <button
                            attr:data-testid="settings-btn-add-device"
                            class="button is-link is-light"
                            on:click=move |_| show_pair.set(true)
                        >
                            {t("settings.add_device")}
                        </button>
                    </section>

                    <section class="mt-6">
                        <h2 class="subtitle is-5 mb-4">{t("settings.data")}</h2>
                        <button
                            attr:data-testid="settings-btn-wipe-all"
                            class="button is-danger is-light"
                            on:click=move |_| {
                                spawn_local(async move {
                                    crate::services::db::wipe_all().await;
                                    invalidate();
                                });
                            }
                        >{t("settings.wipe_all")}</button>
                    </section>
                </div>
            }.into_view()
        }}
    }
}
