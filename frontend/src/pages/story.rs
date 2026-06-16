use leptos::*;

use crate::services::{db, i18n::t, local, story, subscription};

const IOS_BG: &str = "background: var(--bulma-background); min-height: 100vh; padding: 16px; margin: -0.75rem;";
const IOS_CARD: &str = "background: var(--bulma-scheme-main); border-radius: 12px; overflow: hidden;";
const IOS_SECTION_LABEL: &str = "text-transform: uppercase; letter-spacing: 0.02em; padding: 24px 0 8px 16px; margin: 0;";
const IOS_SEPARATOR: &str = "border-bottom: 0.5px solid var(--bulma-border-weak); margin-left: 52px;";

/// Sections of chapter 1: (emoji icon, i18n label key, route once unlocked).
const CH1_SECTIONS: [(&str, &str, Option<&str>); 9] = [
    ("\u{27a1}\u{fe0f}", "story.ch1.intro", Some("/story/intro")),
    ("\u{2699}\u{fe0f}", "story.ch1.setup", Some("/story/setup")),
    ("\u{1f4b0}", "story.ch1.accounting", Some("/story/accounting")),
    ("\u{1f37d}\u{fe0f}", "story.ch1.first_food", Some("/story/first-food")),
    ("\u{1f6b6}", "story.ch1.activity", Some("/story/activity")),
    ("\u{1f468}\u{200d}\u{1f373}", "story.ch1.cooking", Some("/story/cooking")),
    ("\u{1f9b4}", "story.ch1.bones", Some("/story/bones")),
    ("\u{1f389}", "story.ch1.restaurant", Some("/story/restaurant")),
    ("\u{1f513}", "story.ch1.next", Some("/story/next")),
];

/// Sections of chapter 2 «Appetite». Content/tasks added per-section as written;
/// routes are None until each page exists.
const CH2_SECTIONS: [(&str, &str, Option<&str>); 5] = [
    ("\u{26a0}\u{fe0f}", "story.ch2.s1", None),
    ("\u{1f966}", "story.ch2.s2", None),
    ("\u{1f963}", "story.ch2.s3", None),
    ("\u{1f357}", "story.ch2.s4", None),
    ("\u{1f6ab}", "story.ch2.s5", None),
];

#[component]
pub fn StoryPage() -> impl IntoView {
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
    create_effect(move |_| {
        story_ver.get();
        spawn_local(async move {
            sex_done.set(story::get_flag(story::SEX_SELECTED).await);
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

    // Subscription status (gates chapter 2). Seed from cache, refresh live.
    let sub_active = create_rw_signal(subscription::cached().map(|s| s.active).unwrap_or(false));
    let sub_paid = create_rw_signal(subscription::cached().map(|s| s.is_paid()).unwrap_or(false));
    spawn_local(async move {
        if let Ok(s) = subscription::status().await {
            sub_active.set(s.active);
            sub_paid.set(s.is_paid());
        }
    });

    // A section's *completion* — the task(s) that open the next section.
    let completed = move |i: usize| match i {
        0 => progress_photos.get(),               // intro: front/side/back photos taken
        1 => lang_done.get() && notif_done.get(), // setup: language + notification
        2 => weigh_in_on.get(),                   // accounting: weigh-in reminder enabled
        3 => first_food_done.get(),               // first food: a food was entered
        4 => first_steps.get(),                   // activity: steps recorded at least once
        5 => dish_created.get() && dish_in_diary.get(), // cooking: dish created + added to diary
        6 => bones_waste.get(),                    // bones: a waste value was entered
        7 => restaurant_food.get(),                // restaurant: restaurant food logged
        8 => sub_paid.get(),                        // next: subscribed (paid) via the paywall
        _ => false,
    };
    // Unlocking is cumulative: a section is open only if every earlier section
    // is completed. This keeps the chain strictly sequential — there can never
    // be a locked section sitting before an unlocked one.
    let is_unlocked = move |i: usize| (0..i).all(completed);

    // Chapter 2 opens once the user has weighed in 7 days in a row AND has an
    // active subscription (Trial not expired, or Paid).
    let chapter2_unlocked = move || streak.get() >= 7 && sub_active.get();

    let sections_total = CH1_SECTIONS.len();
    let sections_open = move || (0..sections_total).filter(|&i| is_unlocked(i)).count();
    // Chapter 1 tasks: want a new body, language, test notification, weigh-in
    // reminder, first measurement, 7-day weigh-in streak, first food entry,
    // steps reminder, first steps, 7-day steps streak, dish created, dish in diary.
    let tasks_total = 16;
    let tasks_done = move || {
        [progress_photos.get(), sex_done.get(), lang_done.get(), notif_done.get(), weigh_in_on.get(),
         first_weigh.get(), streak.get() >= 7, first_food_done.get(),
         steps_reminder_on.get(), first_steps.get(), steps_streak.get() >= 7,
         dish_created.get(), dish_in_diary.get(), bones_waste.get(), restaurant_food.get(),
         sub_paid.get()]
            .iter().filter(|&&v| v).count()
    };

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
                {CH2_SECTIONS.iter().enumerate().map(|(i, (icon, label, _))| {
                    let icon = *icon;
                    let label = *label;
                    view! {
                        {(i > 0).then(|| view! { <div style=IOS_SEPARATOR></div> })}
                        <div style=format!("{}opacity: 0.5;", ROW_STYLE)>
                            <span style="font-size: 22px; width: 28px; text-align: center;">{icon}</span>
                            <span class="is-size-6" style="flex: 1;">{move || t(label)}</span>
                            <span class="is-size-7 has-text-grey-light">{move || t("story.ch2.soon")}</span>
                        </div>
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

            <div style="height: 40px;"></div>
        </div>
    }
}
