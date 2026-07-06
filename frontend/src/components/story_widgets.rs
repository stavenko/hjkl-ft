//! Named, self-contained widgets the Story DSL embeds in section content blocks
//! (`{widget: {id: ...}}`). Each is a real `#[component]` with a stable reactive
//! scope and manages its own state, so the generic section page can rebuild its
//! body on section change without remounting / resetting a widget mid-stream.

use chrono::Datelike;
use leptos::*;
use leptos_router::*;

use crate::services::story_dsl::{self, Engine, EngineSnapshot, Loc};
use crate::services::weekly_card::{self, Card, CardState, CourseGoalDto, EngineInput, TrendDir};
use crate::services::{app_flags, db, i18n, i18n::t, local, profile, story, sync};

const CARD: &str = "background: var(--bulma-scheme-main); border-radius: 12px; overflow: hidden;";

fn tr(l: &Loc) -> String {
    match i18n::get_lang() {
        i18n::Lang::En => l.en.clone(),
        i18n::Lang::Ru => l.ru.clone(),
    }
}

/// Load the engine snapshot into a signal, rebuilding it whenever any sensor
/// source changes. Shared by widgets that need task/section state.
fn snapshot_signal() -> RwSignal<Option<EngineSnapshot>> {
    let snap = create_rw_signal(None::<EngineSnapshot>);
    let reload = move || spawn_local(async move { snap.set(Some(story::engine_snapshot().await)); });
    let vers = [
        db::version("story"),
        db::version("weight_entries"),
        db::version("step_entries"),
        db::version("diary"),
        db::version("goals"),
        db::version("summaries"),
    ];
    create_effect(move |_| {
        for v in &vers {
            v.get();
        }
        reload();
    });
    snap
}

/// The section's task list (checkmark rows + a "section complete" line), driven
/// by the engine. Used by the generic `{tasks: true}` block.
#[component]
pub fn StoryTaskList(section_id: String) -> impl IntoView {
    let snap = snapshot_signal();
    view! {
        <div style="margin: 16px 0 0 0;">
            <p class="is-size-7 has-text-grey-light" style="text-transform: uppercase; letter-spacing: 0.02em; margin: 0 0 8px 4px;">
                {move || t("story.section_task_label")}
            </p>
            {move || {
                let Some(s) = snap.get() else {
                    return view! { <div style=CARD></div> }.into_view();
                };
                let e = Engine::new(story_dsl::story(), &s);
                let Some((_, sec)) = story_dsl::find_section(&section_id) else {
                    return ().into_view();
                };
                let rows = sec.tasks.iter().map(|tid| {
                    let done = e.task_closed(tid);
                    let raw = e.task(tid).map(|t| tr(&t.title)).unwrap_or_default();
                    let title = story::fill_task_target(tid, raw, &s.progress);
                    let icon = if done { "\u{2705}" } else { "\u{23f3}" };
                    // Counter tasks (7-day streaks etc.) show a "current/target" sub-line.
                    let counter = e.task_counter(tid).map(|(cur, target)| view! {
                        <div style="padding: 0 16px 10px 50px;">
                            <span class="is-size-7 has-text-grey-light">{format!("{cur}/{target}")}</span>
                        </div>
                    });
                    view! {
                        <div style="display: flex; align-items: flex-start; gap: 12px; padding: 14px 16px;">
                            <span style="font-size: 22px; width: 22px; text-align: center;">{icon}</span>
                            <span class="is-size-6" style="flex: 1; line-height: 1.4;">{title}</span>
                        </div>
                        {counter}
                    }
                }).collect_view();
                let complete = e.section_complete(sec);
                view! {
                    <div style=CARD>{rows}</div>
                    {complete.then(|| view! {
                        <p class="is-size-6 has-text-weight-semibold has-text-success" style="margin-top: 16px;">
                            {move || t("story.section_done")}
                        </p>
                    })}
                }.into_view()
            }}
        </div>
    }
}

/// A full-width navigation button (DSL: `{widget: {id: cta, route, label}}`).
#[component]
pub fn Cta(route: String, label: String) -> impl IntoView {
    view! {
        <div style="padding: 16px 16px 0 16px;">
            <A href=route class="button is-link is-fullwidth is-medium">
                {move || t(&label)}
            </A>
        </div>
    }
}

/// Intro task: take the three progress photos (front / side / back). Completes
/// once `PROGRESS_PHOTOS_TAKEN` is set (by the progress page).
#[component]
pub fn ProgressPhotos() -> impl IntoView {
    let story_ver = db::version("story");
    let done = create_rw_signal(false);
    create_effect(move |_| {
        story_ver.get();
        spawn_local(async move { done.set(story::get_flag(story::PROGRESS_PHOTOS_TAKEN).await); });
    });

    view! {
        <div style="margin: 16px 0 0 0;">
            <p class="is-size-7 has-text-grey-light" style="text-transform: uppercase; letter-spacing: 0.02em; margin: 0 0 8px 4px;">
                {move || t("story.intro.photo_task_label")}
            </p>
            <div style=CARD>
                <div style="padding: 14px 16px;">
                    <p class="is-size-6" style="line-height: 1.55; margin: 0 0 12px 0;">{move || t("story.intro.photo_desc")}</p>
                    <div style="display: flex; justify-content: center;">
                        <img src="/progress-poses.jpg" alt="" style="display: block; width: 100%; max-width: 320px; height: auto;" />
                    </div>
                    <div style="display: flex; justify-content: space-around; max-width: 300px; margin: 4px auto 0 auto;">
                        <span class="is-size-7 has-text-grey">{move || t("progress.pose_front")}</span>
                        <span class="is-size-7 has-text-grey">{move || t("progress.pose_back")}</span>
                        <span class="is-size-7 has-text-grey">{move || t("progress.pose_side")}</span>
                    </div>
                </div>
                <div style="border-bottom: 0.5px solid var(--bulma-border-weak);"></div>
                <div style="display: flex; align-items: center; gap: 12px; padding: 14px 16px;">
                    <span style="font-size: 22px; width: 22px; text-align: center;">
                        {move || if done.get() { "\u{2705}" } else { "\u{23f3}" }}
                    </span>
                    <span class="is-size-6" style="flex: 1; line-height: 1.4;">{move || t("story.intro.photo_check")}</span>
                    <A href="/progress" class="button is-link is-small">{move || t("progress.capture")}</A>
                </div>
            </div>
            {move || done.get().then(|| view! {
                <p class="is-size-7 has-text-success" style="margin: 12px 4px 0 4px;">
                    {move || t("story.intro.unlocked_hint")}
                </p>
            })}
        </div>
    }
}

/// Hidden-goal status card (Protein / Calorie planka). The goal value is set by
/// the section's `on_open` action; this shows it once present, or a "need …"
/// prompt + CTA when the precondition (a weight / some diary days) isn't met yet.
#[component]
pub fn GoalStatus(
    nutrient: String,
    unit: String,
    title: String,
    set: String,
    need: String,
    route: String,
    label: String,
) -> impl IntoView {
    let goals_ver = db::version("goals");
    let amount = create_rw_signal(None::<f64>);
    {
        let nutrient = nutrient.clone();
        create_effect(move |_| {
            goals_ver.get();
            let nutrient = nutrient.clone();
            spawn_local(async move {
                let v = local::list_goals().await.into_iter()
                    .find(|g| g.nutrient == nutrient && g.amount > 0.0)
                    .map(|g| g.amount);
                amount.set(v);
            });
        });
    }
    // Stored (Copy) so each reactive closure can read its own clone.
    let unit = store_value(unit);
    let title = store_value(title);
    let set = store_value(set);
    let need = store_value(need);
    let route = store_value(route);
    let label = store_value(label);
    view! {
        <div style="margin: 16px 0 0 0;">
            <p class="is-size-7 has-text-grey-light" style="text-transform: uppercase; letter-spacing: 0.02em; margin: 0 0 8px 4px;">
                {move || t(&title.get_value())}
            </p>
            <div style=CARD>
                <div style="display: flex; align-items: center; justify-content: center; padding: 18px 16px; text-align: center;">
                    {move || match amount.get() {
                        Some(v) => view! {
                            <span class="is-size-3 has-text-weight-bold">{format!("{} {}", v.round() as i64, t(&unit.get_value()))}</span>
                        }.into_view(),
                        None => view! { <span class="is-size-6 has-text-grey">{t(&need.get_value())}</span> }.into_view(),
                    }}
                </div>
            </div>
            {move || match amount.get() {
                Some(_) => view! {
                    <p class="is-size-6 has-text-weight-semibold has-text-success" style="margin-top: 16px;">{t(&set.get_value())}</p>
                }.into_view(),
                None => view! {
                    <div style="padding: 16px 0 0 0;">
                        <A href=route.get_value() class="button is-link is-fullwidth is-medium">{t(&label.get_value())}</A>
                    </div>
                }.into_view(),
            }}
        </div>
    }
}

/// Chapter-3 calorie planka: shows the daily planka computed LIVE as the average
/// of the last 7 logged days, and a button to accept it. Accepting sets the hidden
/// daily-Calories goal (completing the section). The suggested figure is recomputed
/// every time the widget mounts, so returning a day later offers a fresh planka.
#[component]
pub fn CaloriePlanka() -> impl IntoView {
    let diary_ver = db::version("diary");
    let goals_ver = db::version("goals");
    let suggestion = create_rw_signal(None::<f64>); // live 7-day average (kcal)
    let accepted = create_rw_signal(None::<f64>); // the currently-set planka, if any

    create_effect(move |_| {
        diary_ver.get();
        goals_ver.get();
        spawn_local(async move {
            suggestion.set(local::calorie_planka_suggestion().await);
            accepted.set(
                local::list_goals().await.into_iter()
                    .find(|g| g.nutrient == "Calories" && g.amount > 0.0)
                    .map(|g| g.amount),
            );
        });
    });

    let accept = move |_| {
        let Some(n) = suggestion.get() else { return };
        spawn_local(async move {
            local::set_calorie_goal(n).await;
            crate::services::sync::push_background();
        });
    };

    // The task's intro text, shared by the "take the planka" states (before accept).
    let intro = || view! {
        <p class="is-size-6" style="line-height: 1.55; margin: 0 0 14px 0;">{t("story.ch3.fat.task_intro")}</p>
    };

    view! {
        // Same wrapper + "ЗАДАНИЕ" label as the standard task list in other chapters.
        <div style="margin: 16px 0 0 0;">
            <p class="is-size-7 has-text-grey-light" style="text-transform: uppercase; letter-spacing: 0.02em; margin: 0 0 8px 4px;">
                {move || t("story.section_task_label")}
            </p>
            {move || match suggestion.get() {
                // Already accepted at the current value → ONLY the confirmation text.
                Some(v) if accepted.get().map(|a| a.round() as i64) == Some(v.round() as i64) => view! {
                    <p class="is-size-6 has-text-weight-semibold">
                        {t("story.ch3.fat.planka_accepted").replace("{n}", &(v.round() as i64).to_string())}
                    </p>
                    <p class="is-size-6 has-text-grey" style="margin-top: 6px; line-height: 1.5;">
                        {t("story.ch3.fat.planka_accepted_note")}
                    </p>
                }.into_view(),
                // A figure to take → intro + a full-width button carrying the number
                // (same style as the section CTAs — no separate number plate).
                Some(v) => {
                    let label = t("story.ch3.fat.planka_accept").replace("{n}", &(v.round() as i64).to_string());
                    view! {
                        {intro()}
                        <button class="button is-link is-fullwidth is-medium" on:click=accept>{label}</button>
                    }.into_view()
                }
                // No diary yet → intro + a grey note + the diary CTA.
                None => view! {
                    {intro()}
                    <p class="is-size-6 has-text-grey" style="margin: 0 0 12px 0; line-height: 1.5;">{t("story.ch3.fat.need_diary")}</p>
                    <A href="/diary" class="button is-link is-fullwidth is-medium">{t("story.ch3.fat.open_diary")}</A>
                }.into_view(),
            }}
        </div>
    }
}

/// Chapter-2 night feedback: today's evening protein (dinner + night) ≥ 30 g.
#[component]
pub fn NightFeedback() -> impl IntoView {
    let diary_ver = db::version("diary");
    let protein = create_rw_signal(0.0_f64);
    create_effect(move |_| {
        diary_ver.get();
        spawn_local(async move {
            let today = chrono::Local::now().format("%Y-%m-%d").to_string();
            protein.set(local::evening_protein_on(&today).await);
        });
    });
    view! {
        <div style="margin: 16px 0 0 0;">
            <p class="is-size-7 has-text-grey-light" style="text-transform: uppercase; letter-spacing: 0.02em; margin: 0 0 8px 4px;">
                {move || t("story.ch2.night.feedback_label")}
            </p>
            <div style=CARD>
                <div style="display: flex; align-items: flex-start; gap: 12px; padding: 14px 16px;">
                    {move || if protein.get() >= 30.0 {
                        view! {
                            <span style="font-size: 22px; width: 22px; text-align: center;">"\u{1f4aa}"</span>
                            <span class="is-size-6" style="flex: 1; line-height: 1.4;">{move || t("story.ch2.night.feedback_good")}</span>
                        }.into_view()
                    } else {
                        view! {
                            <span style="font-size: 22px; width: 22px; text-align: center;">"\u{1f319}"</span>
                            <span class="is-size-6" style="flex: 1; line-height: 1.4;">{move || t("story.ch2.night.feedback_hint")}</span>
                        }.into_view()
                    }}
                </div>
            </div>
        </div>
    }
}

// app_flags keys backing the weekly-card cadence (non-synced, per-device).
const WC_CARD_KEY: &str = "weekly_card";
const WC_LAST_RECOMPUTE_KEY: &str = "weekly_card_last_recompute";

/// The most recent Monday on/before `d` (ISO week start).
fn week_start(d: chrono::NaiveDate) -> chrono::NaiveDate {
    let dow = d.weekday().num_days_from_monday() as i64;
    d - chrono::Duration::days(dow)
}

/// Whether the card should be recomputed: never computed yet, or the current
/// week's Monday is strictly after the last-recompute's date (the §10 Monday
/// boundary, NOT a rolling 7-day window). Hysteresis: between Mondays the stored
/// card is reused so mid-week weight noise never flips the tier.
fn should_recompute(last_recompute: Option<&str>, today: chrono::NaiveDate) -> bool {
    let Some(ts) = last_recompute else { return true };
    let Ok(dt) = chrono::DateTime::parse_from_rfc3339(ts) else { return true };
    let last_date = dt.with_timezone(&chrono::Local).date_naive();
    week_start(today) > week_start(last_date)
}

/// Gather the engine inputs from the local services and call `decide`.
async fn compute_card() -> Card {
    let weight_entries = local::list_weight_entries().await;
    let step_entries = local::list_step_entries().await;
    let avg_intake_14d = local::avg_daily_kcal(14).await;

    // Distinct diary days within the last 14 calendar days (coverage numerator).
    let today = chrono::Local::now().date_naive();
    let window_start = today - chrono::Duration::days(13);
    let logged_days_14d = local::list_diary_dates()
        .await
        .iter()
        .filter_map(|d| chrono::NaiveDate::parse_from_str(d, "%Y-%m-%d").ok())
        .filter(|d| *d >= window_start && *d <= today)
        .collect::<std::collections::BTreeSet<_>>()
        .len() as u32;

    let current_cal_planka = local::list_goals().await.into_iter().find(|g| {
        g.nutrient == "Calories" && g.direction == api_types::GoalDirection::AtMost && g.amount > 0.0
    }).map(|g| g.amount);

    let today_year = chrono::Local::now().format("%Y").to_string().parse::<i32>().unwrap_or(2026);

    let input = EngineInput {
        sex: profile::get_sex(),
        height_cm: profile::get_height_cm(),
        birth_year: profile::get_birth_year(),
        today_year,
        weight_entries,
        avg_intake_14d,
        logged_days_14d,
        step_entries,
        current_cal_planka,
        current_steps_target: local::steps_goal_amount().await.map(|a| a as u32).unwrap_or(story::STEPS_PLANKA),
        goal: profile::get_goal(),
    };
    weekly_card::decide(&input)
}

/// The weekly recommendation card (DSL: `{widget: {id: weekly_card}}`). A DUMB
/// component: it gathers inputs, calls the pure `decide`, and renders STRICTLY by
/// `state` + `levers`. It makes no decisions. Recompute is gated to a weekly
/// Monday boundary (cadence + hysteresis); within the week the stored card is
/// rendered verbatim. A "Пересчитать" button forces a recompute.
#[component]
pub fn WeeklyCard() -> impl IntoView {
    // Recompute on the version signals AND when the profile cache may have changed.
    let vers = [
        db::version("weight_entries"),
        db::version("step_entries"),
        db::version("diary"),
        db::version("goals"),
        db::version("story"),
        db::version("summaries"),
        db::version("profile"),
    ];
    let card = create_rw_signal(None::<Card>);
    // A manual-recompute nonce: bumping it re-runs the effect (the click also
    // clears last_recompute, so should_recompute then returns true).
    let force = create_rw_signal(0u32);

    create_effect(move |_| {
        for v in &vers {
            v.get();
        }
        force.get();
        spawn_local(async move {
            let today = chrono::Local::now().date_naive();
            let last = app_flags::get(WC_LAST_RECOMPUTE_KEY);
            if should_recompute(last.as_deref(), today) {
                let c = compute_card().await;
                if let Ok(json) = serde_json::to_string(&c) {
                    app_flags::set(WC_CARD_KEY, &json);
                }
                app_flags::set(WC_LAST_RECOMPUTE_KEY, &chrono::Utc::now().to_rfc3339());
                card.set(Some(c));
            } else if let Some(stored) = app_flags::get(WC_CARD_KEY) {
                // Render the STORED card verbatim (hysteresis: no recompute mid-week).
                match serde_json::from_str::<Card>(&stored) {
                    Ok(c) => card.set(Some(c)),
                    Err(_) => card.set(Some(compute_card().await)),
                }
            } else {
                card.set(Some(compute_card().await));
            }
        });
    });

    let recompute = move |_| {
        app_flags::remove(WC_LAST_RECOMPUTE_KEY);
        force.update(|n| *n += 1);
    };

    // Accept the calorie suggestion: set the daily Calories goal + push.
    let accept_calories = move |n: f64| {
        spawn_local(async move {
            local::set_calorie_goal(n).await;
            sync::push_background();
        });
    };
    // Accept the steps suggestion: set the daily Steps goal + push.
    let accept_steps = move |n: u32| {
        spawn_local(async move {
            local::set_steps_goal(n as f64).await;
            sync::push_background();
        });
    };

    view! {
        <div style="margin: 16px 0 0 0;">
            <p class="is-size-7 has-text-grey-light" style="text-transform: uppercase; letter-spacing: 0.02em; margin: 0 0 8px 4px;">
                {move || t("weekly_card.title")}
            </p>
            {move || {
                let Some(c) = card.get() else {
                    return view! { <div style=CARD></div> }.into_view();
                };
                render_card(c, accept_calories, accept_steps, recompute)
            }}
        </div>
    }
}

/// Render the card strictly by `state` + `levers`. The calorie button is emitted
/// ONLY inside `levers.calories.then(...)` with `computed.new_cal` present — so in
/// HARD (levers.calories == false) no calorie node is constructed at all (§7.1).
fn render_card(
    c: Card,
    accept_calories: impl Fn(f64) + Copy + 'static,
    accept_steps: impl Fn(u32) + Copy + 'static,
    recompute: impl Fn(web_sys::MouseEvent) + Copy + 'static,
) -> View {
    // Universal terminal states (goal-independent).
    let body = if c.state == CardState::NeedBirthYear {
        view! {
            <div style=CARD>
                <div style="padding: 16px;">
                    <p class="is-size-6" style="line-height: 1.55; margin: 0 0 12px 0;">{t("weekly_card.need_birth_year")}</p>
                    <A href="/settings" class="button is-link is-fullwidth is-medium">{t("weekly_card.open_settings")}</A>
                </div>
            </div>
        }.into_view()
    } else if c.state == CardState::InsufficientData {
        view! {
            <div style=CARD><div style="padding: 16px;">
                <p class="is-size-6" style="line-height: 1.55; margin: 0;">{t("weekly_card.insufficient")}</p>
            </div></div>
        }.into_view()
    } else if c.state == CardState::Hard {
        view! {
            <div style=CARD><div style="padding: 16px;">
                <p class="is-size-6" style="line-height: 1.55; margin: 0;">{t("weekly_card.msg_hard")}</p>
                // §7.1: NO calorie button is constructed in HARD.
                {steps_button(&c, accept_steps)}
            </div></div>
        }.into_view()
    } else if c.goal == CourseGoalDto::Maintain {
        // Maintenance: copy by direction; the calorie lever is off by construction.
        // Flat = success; Rising = add steps; Falling = info only.
        let msg = match c.trend {
            TrendDir::Flat => t("weekly_card.maintain_ok"),
            TrendDir::Rising => t("weekly_card.maintain_rising"),
            TrendDir::Falling => t("weekly_card.maintain_falling"),
        };
        view! {
            <div style=CARD><div style="padding: 16px;">
                <p class="is-size-6" style="line-height: 1.55; margin: 0;">{msg}</p>
                {steps_button(&c, accept_steps)}
            </div></div>
        }.into_view()
    } else if c.state == CardState::OnTrack {
        // Lose goal, confidently falling, clean ratio → all good.
        view! {
            <div style=CARD><div style="padding: 16px;">
                <p class="is-size-6" style="line-height: 1.55; margin: 0;">{t("weekly_card.on_track")}</p>
            </div></div>
        }.into_view()
    } else {
        // Lose goal, Plateau | Surplus | Soft. Message by direction (#2 / #3).
        let msg = if c.trend == TrendDir::Rising {
            t("weekly_card.msg_surplus")
        } else {
            t("weekly_card.msg_plateau")
        };
        let soft_note = (c.state == CardState::Soft).then(|| view! {
            <p class="is-size-7 has-text-grey" style="margin: 8px 0 0 0; line-height: 1.45;">{t("weekly_card.soft_note")}</p>
        });
        let cal_btn = (c.levers.calories).then(|| c.computed.new_cal.map(|n| {
            let label = t("weekly_card.btn_calories").replace("{n}", &(n.round() as i64).to_string());
            view! {
                <button class="button is-link is-fullwidth is-medium" style="margin-top: 12px;"
                    on:click=move |_| accept_calories(n)>{label}</button>
            }
        })).flatten();
        view! {
            <div style=CARD><div style="padding: 16px;">
                <p class="is-size-6" style="line-height: 1.55; margin: 0;">{msg}</p>
                {soft_note}
                {cal_btn}
                {steps_button(&c, accept_steps)}
            </div></div>
        }.into_view()
    };

    view! {
        {body}
        <div style="padding: 12px 4px 0 4px;">
            <button class="button is-small is-light" on:click=recompute>{t("weekly_card.recompute")}</button>
        </div>
    }.into_view()
}

/// The steps button, emitted only when `levers.steps` is set and a value exists.
/// Accepting it persists the new daily Steps goal (same machinery as nutrient goals).
fn steps_button(c: &Card, accept_steps: impl Fn(u32) + Copy + 'static) -> View {
    if !c.levers.steps {
        return ().into_view();
    }
    let Some(n) = c.computed.new_steps else {
        return ().into_view();
    };
    let label = t("weekly_card.btn_steps").replace("{n}", &n.to_string());
    view! {
        <button class="button is-link is-fullwidth is-medium" style="margin-top: 12px;"
            on:click=move |_| accept_steps(n)>{label}</button>
    }.into_view()
}

/// Chapter-1 setup controls: the language checkbox (toggles the task), the test
/// notification status, and the sex-selection status. Opening with `?notif=1`
/// (the test push deep-link) marks the notification task done.
#[component]
pub fn SetupControls() -> impl IntoView {
    let story_ver = db::version("story");
    let lang_done = create_rw_signal(false);
    let notif_done = create_rw_signal(false);
    let sex_done = create_rw_signal(false);
    let age_done = create_rw_signal(false);
    let height_done = create_rw_signal(false);
    create_effect(move |_| {
        story_ver.get();
        spawn_local(async move {
            lang_done.set(story::get_flag(story::LANGUAGE_CONFIGURED).await);
            notif_done.set(story::get_flag(story::NOTIFICATION_RECEIVED).await);
            sex_done.set(story::get_flag(story::SEX_SELECTED).await);
            // Backfill: users who set birth_year / height BEFORE this feature have
            // the profile value but never fired the story flag. Reflect the real
            // profile value so they can complete the section.
            if !story::get_flag(story::BIRTH_YEAR_SET).await && profile::get_birth_year().is_some() {
                story::set_flag(story::BIRTH_YEAR_SET, true).await;
            }
            if !story::get_flag(story::HEIGHT_SET).await && profile::get_height_cm().is_some() {
                story::set_flag(story::HEIGHT_SET, true).await;
            }
            age_done.set(story::get_flag(story::BIRTH_YEAR_SET).await);
            height_done.set(story::get_flag(story::HEIGHT_SET).await);
        });
    });

    let toggle_lang = move |_| {
        let v = !lang_done.get_untracked();
        lang_done.set(v);
        spawn_local(async move { story::set_flag(story::LANGUAGE_CONFIGURED, v).await; });
    };

    view! {
        <div style="margin: 16px 0 0 0;">
            <p class="is-size-7 has-text-grey-light" style="text-transform: uppercase; letter-spacing: 0.02em; margin: 0 0 8px 4px;">
                {move || t("story.setup.task_label")}
            </p>
            <div style=CARD>
                <label style="display: flex; align-items: center; gap: 12px; padding: 14px 16px; cursor: pointer;">
                    <input type="checkbox"
                        attr:data-testid="story-setup-language-configured"
                        style="width: 22px; height: 22px; accent-color: var(--bulma-link);"
                        prop:checked=move || lang_done.get()
                        on:change=toggle_lang
                    />
                    <span class="is-size-6 has-text-weight-semibold">{move || t("story.setup.checkbox_lang")}</span>
                </label>
                <div style="border-bottom: 0.5px solid var(--bulma-border-weak);"></div>
                <div style="display: flex; align-items: center; gap: 12px; padding: 14px 16px;">
                    <span style="font-size: 22px; width: 22px; text-align: center;">{move || if notif_done.get() { "\u{2705}" } else { "\u{23f3}" }}</span>
                    <span class="is-size-6 has-text-weight-semibold" style="flex: 1;">
                        {move || if notif_done.get() { t("story.setup.notif_status_done") } else { t("story.setup.notif_status_pending") }}
                    </span>
                </div>
                <div style="border-bottom: 0.5px solid var(--bulma-border-weak);"></div>
                <div style="display: flex; align-items: center; gap: 12px; padding: 14px 16px;">
                    <span style="font-size: 22px; width: 22px; text-align: center;">{move || if sex_done.get() { "\u{2705}" } else { "\u{23f3}" }}</span>
                    <span class="is-size-6 has-text-weight-semibold" style="flex: 1;">
                        {move || if sex_done.get() { t("story.setup.sex_status_done") } else { t("story.setup.sex_status_pending") }}
                    </span>
                </div>
                <div style="border-bottom: 0.5px solid var(--bulma-border-weak);"></div>
                <div style="display: flex; align-items: center; gap: 12px; padding: 14px 16px;">
                    <span style="font-size: 22px; width: 22px; text-align: center;">{move || if age_done.get() { "\u{2705}" } else { "\u{23f3}" }}</span>
                    <span class="is-size-6 has-text-weight-semibold" style="flex: 1;">
                        {move || if age_done.get() { t("story.setup.age_status_done") } else { t("story.setup.age_status_pending") }}
                    </span>
                </div>
                <div style="border-bottom: 0.5px solid var(--bulma-border-weak);"></div>
                <div style="display: flex; align-items: center; gap: 12px; padding: 14px 16px;">
                    <span style="font-size: 22px; width: 22px; text-align: center;">{move || if height_done.get() { "\u{2705}" } else { "\u{23f3}" }}</span>
                    <span class="is-size-6 has-text-weight-semibold" style="flex: 1;">
                        {move || if height_done.get() { t("story.setup.height_status_done") } else { t("story.setup.height_status_pending") }}
                    </span>
                </div>
            </div>
            {move || (lang_done.get() && notif_done.get() && sex_done.get() && age_done.get() && height_done.get()).then(|| view! {
                <p class="is-size-6 has-text-weight-semibold has-text-success" style="margin-top: 16px;">
                    {move || t("story.setup.next_unlocked")}
                </p>
            })}
        </div>
    }
}
