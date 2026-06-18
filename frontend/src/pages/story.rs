use leptos::*;

use crate::services::{db, i18n::t, local, story, subscription};

const IOS_BG: &str = "background: var(--bulma-background); min-height: 100vh; padding: 16px; margin: -0.75rem;";
const IOS_CARD: &str = "background: var(--bulma-scheme-main); border-radius: 12px; overflow: hidden;";
const IOS_SECTION_LABEL: &str = "text-transform: uppercase; letter-spacing: 0.02em; padding: 24px 0 8px 16px; margin: 0;";
const IOS_SEPARATOR: &str = "border-bottom: 0.5px solid var(--bulma-border-weak); margin-left: 52px;";

use story::{CH1_SECTIONS, CH2_SECTIONS, CH3_SECTIONS};

#[component]
pub fn StoryPage() -> impl IntoView {
    // Opening the Story page acknowledges every completed task (clears the
    // "task done" attention marker) and refreshes the nav-icon dot.
    spawn_local(async move {
        story::ack_done_tasks().await;
        story::refresh_attention();
    });
    // Story progress is driven by the IndexedDB `story` store. Reload whenever
    // that store is written to (the `want_new_body` flag toggled on the intro page).
    let story_ver = db::version("story");
    let lang_done = create_rw_signal(false);
    let notif_done = create_rw_signal(false);
    // Milestone events recorded in the story DB.
    let weigh_in_on = create_rw_signal(false);
    let first_weigh = create_rw_signal(false);
    let first_food_done = create_rw_signal(false);
    let steps_reminder_on = create_rw_signal(false);
    let first_steps = create_rw_signal(false);
    let dish_created = create_rw_signal(false);
    let dish_in_diary = create_rw_signal(false);
    let bones_waste = create_rw_signal(false);
    let restaurant_food = create_rw_signal(false);
    let progress_photos = create_rw_signal(false);
    let sex_done = create_rw_signal(false);
    // Chapter 2 / s6 & s7 flags (s6: meal-split opened; s7: night feedback viewed).
    let meal_split_unlocked = create_rw_signal(false);
    let night_feedback_viewed = create_rw_signal(false);
    create_effect(move |_| {
        story_ver.get();
        spawn_local(async move {
            sex_done.set(story::get_flag(story::SEX_SELECTED).await);
            meal_split_unlocked.set(story::get_flag(story::MEAL_SPLIT_UNLOCKED).await);
            night_feedback_viewed.set(story::get_flag(story::NIGHT_FEEDBACK_VIEWED).await);
            lang_done.set(story::get_flag(story::LANGUAGE_CONFIGURED).await);
            notif_done.set(story::get_flag(story::NOTIFICATION_RECEIVED).await);
            weigh_in_on.set(story::get_flag(story::WEIGH_IN_REMINDER).await);
            first_weigh.set(story::get_flag(story::FIRST_WEIGH).await);
            first_food_done.set(story::get_flag(story::FIRST_FOOD_DONE).await);
            steps_reminder_on.set(story::get_flag(story::STEPS_REMINDER).await);
            first_steps.set(story::get_flag(story::FIRST_STEPS).await);
            dish_created.set(story::get_flag(story::COOKING_DISH_CREATED).await);
            dish_in_diary.set(story::get_flag(story::COOKING_DISH_IN_DIARY).await);
            bones_waste.set(story::get_flag(story::BONES_WASTE_ENTERED).await);
            restaurant_food.set(story::get_flag(story::RESTAURANT_FOOD_ENTERED).await);
            progress_photos.set(story::get_flag(story::PROGRESS_PHOTOS_TAKEN).await);
        });
    });

    // Progress aggregates: the consecutive-day weigh-in and steps streaks.
    let weight_ver = db::version("weight_entries");
    let streak = create_rw_signal(0u32);
    create_effect(move |_| {
        weight_ver.get();
        spawn_local(async move {
            let dates: Vec<String> = local::list_weight_entries().await
                .into_iter().map(|e| e.date).collect();
            streak.set(story::consecutive_day_streak(&dates));
        });
    });

    let steps_ver = db::version("step_entries");
    let steps_streak = create_rw_signal(0u32);
    create_effect(move |_| {
        steps_ver.get();
        spawn_local(async move {
            let dates: Vec<String> = local::list_step_entries().await
                .into_iter().map(|e| e.date).collect();
            steps_streak.set(story::consecutive_day_streak(&dates));
        });
    });

    // Chapter 3 / section 1: the hidden calorie planka has been set (an AtMost
    // "Calories" goal with amount > 0). Opening the s1 page sets it; that both
    // completes s1 and gates the rest of chapter 3.
    let goals_ver = db::version("goals");
    let calorie_planka_set = create_rw_signal(false);
    create_effect(move |_| {
        goals_ver.get();
        spawn_local(async move {
            let set = local::list_goals().await.into_iter().any(|g| {
                g.nutrient == "Calories"
                    && g.direction == api_types::GoalDirection::AtMost
                    && g.amount > 0.0
            });
            calorie_planka_set.set(set);
        });
    });

    // Chapter 2 / section 1: consecutive-day food-diary streak; and the count of
    // DISTINCT tracked days (drives the chapter-3 unlock — "учёт ведётся несколько дней").
    let diary_ver = db::version("diary");
    let diary_streak = create_rw_signal(0u32);
    let diary_days = create_rw_signal(0u32);
    create_effect(move |_| {
        diary_ver.get();
        spawn_local(async move {
            let mut dates = local::list_diary_dates().await;
            diary_streak.set(story::consecutive_day_streak(&dates));
            dates.sort();
            dates.dedup();
            diary_days.set(dates.len() as u32);
        });
    });

    // Chapter 2 / s4 & s5: yesterday's report-grounded checks. Both require the
    // report for yesterday to be ready; s4 also needs a snack logged yesterday,
    // s5 needs NO high-cal drink yesterday. Keyed on diary + summaries versions.
    let summaries_ver = db::version("summaries");
    let s4_done = create_rw_signal(false);
    let s5_done = create_rw_signal(false);
    create_effect(move |_| {
        diary_ver.get();
        summaries_ver.get();
        spawn_local(async move {
            let y = local::yesterday();
            let ready = local::report_ready_on(&y).await;
            s4_done.set(ready && local::snack_logged_on(&y).await);
            s5_done.set(ready && !local::high_cal_drink_on(&y).await);
        });
    });

    // Subscription status (gates chapter 2). Seed from cache, refresh live.
    let sub_active = create_rw_signal(subscription::cached().map(|s| s.active).unwrap_or(false));
    let sub_paid = create_rw_signal(subscription::cached().map(|s| s.is_paid()).unwrap_or(false));
    spawn_local(async move {
        if let Ok(s) = subscription::status().await {
            sub_active.set(s.active);
            sub_paid.set(s.is_paid());
        }
    });

    // The section routes the user has already opened — drives the per-row "new"
    // dot (an unlocked-but-unread section). Reloaded whenever the story DB writes.
    let seen_routes = create_rw_signal(std::collections::HashSet::<String>::new());
    create_effect(move |_| {
        story_ver.get();
        spawn_local(async move {
            seen_routes.set(story::seen_routes().await);
        });
    });

    // Single source of truth: assemble the current progress snapshot from the
    // signals above and let the shared rules (story::Progress) decide what's
    // unlocked / completed — the nav-icon attention marker reads the same rules.
    let progress = move || story::Progress {
        progress_photos: progress_photos.get(),
        sex_done: sex_done.get(),
        lang_done: lang_done.get(),
        notif_done: notif_done.get(),
        weigh_in_on: weigh_in_on.get(),
        first_weigh: first_weigh.get(),
        first_food_done: first_food_done.get(),
        steps_reminder_on: steps_reminder_on.get(),
        first_steps: first_steps.get(),
        dish_created: dish_created.get(),
        dish_in_diary: dish_in_diary.get(),
        bones_waste: bones_waste.get(),
        restaurant_food: restaurant_food.get(),
        meal_split_unlocked: meal_split_unlocked.get(),
        night_feedback_viewed: night_feedback_viewed.get(),
        weight_streak: streak.get(),
        steps_streak: steps_streak.get(),
        diary_streak: diary_streak.get(),
        diary_days: diary_days.get(),
        calorie_planka_set: calorie_planka_set.get(),
        s4_done: s4_done.get(),
        s5_done: s5_done.get(),
        sub_active: sub_active.get(),
        sub_paid: sub_paid.get(),
    };

    let is_unlocked = move |i: usize| progress().ch1_unlocked(i);
    let chapter2_unlocked = move || progress().chapter2_unlocked();
    let ch2_is_unlocked = move |i: usize| progress().ch2_unlocked(i);
    let chapter3_unlocked = move || progress().chapter3_unlocked();
    let ch3_is_unlocked = move |i: usize| progress().ch3_unlocked(i);

    // A section row shows a "new" dot when it's unlocked, has a page, and the
    // user hasn't opened it yet.
    let is_new = move |unlocked: bool, route: Option<&str>| {
        unlocked && matches!(route, Some(r) if !seen_routes.get().contains(r))
    };

    let sections_total = CH1_SECTIONS.len();
    let sections_open = move || (0..sections_total).filter(|&i| is_unlocked(i)).count();
    let tasks_total = story::TASK_KEYS.len();
    let tasks_done = move || progress().tasks().iter().filter(|&&v| v).count();

    const ROW_STYLE: &str = "padding: 12px 16px; display: flex; align-items: center; gap: 12px; color: inherit; text-decoration: none;";

    let rows = CH1_SECTIONS.iter().enumerate().map(|(i, (icon, label, route))| {
        let route = *route;
        let icon = *icon;
        let label = *label;
        view! {
            {(i > 0).then(|| view! { <div style=IOS_SEPARATOR></div> })}
            {move || {
                let unlocked = is_unlocked(i);
                let icon_span = view! { <span style="font-size: 22px; width: 28px; text-align: center;">{icon}</span> };
                let label_span = view! { <span class="is-size-6" style="flex: 1;">{t(label)}</span> };
                match (unlocked, route) {
                    (true, Some(r)) => view! {
                        <a href=r style=format!("{}cursor: pointer;", ROW_STYLE)>
                            {icon_span}
                            {label_span}
                            {is_new(unlocked, route).then(|| view! {
                                <span attr:data-testid="story-section-new-dot"
                                    style="width: 8px; height: 8px; border-radius: 50%; background: var(--bulma-danger); flex: none;"></span>
                            })}
                            <span style="color: var(--bulma-text-weak); font-size: 18px;">"›"</span>
                        </a>
                    }.into_view(),
                    (true, None) => view! {
                        <div style=ROW_STYLE>
                            {icon_span}
                            {label_span}
                        </div>
                    }.into_view(),
                    (false, _) => view! {
                        <div style=format!("{}opacity: 0.45;", ROW_STYLE)>
                            {icon_span}
                            {label_span}
                            <span style="font-size: 15px;">"\u{1f512}"</span>
                        </div>
                    }.into_view(),
                }
            }}
        }
    }).collect_view();

    view! {
        <div style=IOS_BG>
            <h1 class="is-size-1 has-text-weight-bold" style="margin: 0 0 8px 0;">{move || t("story.title")}</h1>

            // ---- Chapter 1 ----
            <p class="is-size-7 has-text-grey-light" style=IOS_SECTION_LABEL>
                {move || format!("{} 1 \u{00b7} {}", t("story.chapter"), t("story.ch1.title"))}
            </p>
            <div style=IOS_CARD>
                {rows}
            </div>

            <div style="padding: 12px 16px 0 16px;">
                <p class="is-size-7 has-text-grey-light" style="margin: 0;">
                    {move || format!("{}: {}/{}", t("story.sections_opened"), sections_open(), sections_total)}
                </p>
                <p class="is-size-7 has-text-grey-light" style="margin: 4px 0 0 0;">
                    {move || format!("{}: {}/{}", t("story.tasks_done"), tasks_done(), tasks_total)}
                </p>
            </div>

            // ---- Chapter 2 · Appetite ----
            <p class="is-size-7 has-text-grey-light" style=IOS_SECTION_LABEL>
                {move || format!("{} 2 \u{00b7} {} {}", t("story.chapter"), t("story.ch2.title"), if chapter2_unlocked() { "" } else { "\u{1f512}" })}
            </p>
            <div style=IOS_CARD>
                {CH2_SECTIONS.iter().enumerate().map(|(i, (icon, label, route))| {
                    let route = *route;
                    let icon = *icon;
                    let label = *label;
                    view! {
                        {(i > 0).then(|| view! { <div style=IOS_SEPARATOR></div> })}
                        {move || {
                            let unlocked = ch2_is_unlocked(i);
                            let icon_span = view! { <span style="font-size: 22px; width: 28px; text-align: center;">{icon}</span> };
                            let label_span = view! { <span class="is-size-6" style="flex: 1;">{t(label)}</span> };
                            match (unlocked, route) {
                                (true, Some(r)) => view! {
                                    <a href=r style=format!("{}cursor: pointer;", ROW_STYLE)>
                                        {icon_span}
                                        {label_span}
                                        <span style="color: var(--bulma-text-weak); font-size: 18px;">"›"</span>
                                    </a>
                                }.into_view(),
                                (true, None) => view! {
                                    <div style=ROW_STYLE>
                                        {icon_span}
                                        {label_span}
                                    </div>
                                }.into_view(),
                                (false, _) => view! {
                                    <div style=format!("{}opacity: 0.45;", ROW_STYLE)>
                                        {icon_span}
                                        {label_span}
                                        <span style="font-size: 15px;">"\u{1f512}"</span>
                                    </div>
                                }.into_view(),
                            }
                        }}
                    }
                }).collect_view()}
            </div>
            <div style="padding: 12px 16px 0 16px;">
                {move || if chapter2_unlocked() {
                    view! {
                        <p class="is-size-7 has-text-success" style="margin: 0;">{move || t("story.ch2.unlocked")}</p>
                    }.into_view()
                } else {
                    view! {
                        <p class="is-size-7 has-text-grey" style="margin: 0 0 6px 0;">{move || t("story.locked_hint")}</p>
                        <ul style="margin: 0; padding-left: 22px; list-style: disc;">
                            <li class="is-size-7">{format!("{} ({}/7)", t("story.ch2.task_weight"), streak.get())}</li>
                            {(!sub_active.get()).then(|| view! {
                                <li class="is-size-7">{move || t("story.ch2.task_subscription")}</li>
                            })}
                        </ul>
                    }.into_view()
                }}
            </div>

            // ---- Chapter 3 · Why lose weight? ----
            <p class="is-size-7 has-text-grey-light" style=IOS_SECTION_LABEL>
                {move || format!("{} 3 \u{00b7} {} {}", t("story.chapter"), t("story.ch3.title"), if chapter3_unlocked() { "" } else { "\u{1f512}" })}
            </p>
            <div style=IOS_CARD>
                {CH3_SECTIONS.iter().enumerate().map(|(i, (icon, label, route))| {
                    let route = *route;
                    let icon = *icon;
                    let label = *label;
                    view! {
                        {(i > 0).then(|| view! { <div style=IOS_SEPARATOR></div> })}
                        {move || {
                            let unlocked = ch3_is_unlocked(i);
                            let icon_span = view! { <span style="font-size: 22px; width: 28px; text-align: center;">{icon}</span> };
                            let label_span = view! { <span class="is-size-6" style="flex: 1;">{t(label)}</span> };
                            match (unlocked, route) {
                                (true, Some(r)) => view! {
                                    <a href=r style=format!("{}cursor: pointer;", ROW_STYLE)>
                                        {icon_span}
                                        {label_span}
                                        <span style="color: var(--bulma-text-weak); font-size: 18px;">"›"</span>
                                    </a>
                                }.into_view(),
                                (true, None) => view! {
                                    <div style=ROW_STYLE>
                                        {icon_span}
                                        {label_span}
                                    </div>
                                }.into_view(),
                                (false, _) => view! {
                                    <div style=format!("{}opacity: 0.45;", ROW_STYLE)>
                                        {icon_span}
                                        {label_span}
                                        <span style="font-size: 15px;">"\u{1f512}"</span>
                                    </div>
                                }.into_view(),
                            }
                        }}
                    }
                }).collect_view()}
            </div>
            <div style="padding: 12px 16px 0 16px;">
                {move || if chapter3_unlocked() {
                    view! {
                        <p class="is-size-7 has-text-success" style="margin: 0;">{move || t("story.ch3.unlocked")}</p>
                    }.into_view()
                } else {
                    view! {
                        <p class="is-size-7 has-text-grey" style="margin: 0;">{move || t("story.ch3.locked_hint")}</p>
                    }.into_view()
                }}
            </div>

            <div style="height: 40px;"></div>
        </div>
    }
}
