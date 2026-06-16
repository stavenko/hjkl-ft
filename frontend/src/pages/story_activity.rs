use leptos::*;
use leptos_router::*;

use crate::services::{db, i18n::t, local, story};

const PAGE_BG: &str = "background: var(--bulma-background); min-height: 100vh; padding: 0; margin: -0.75rem;";
const CARD: &str = "background: var(--bulma-scheme-main); border-radius: 12px; overflow: hidden;";

#[component]
pub fn StoryActivityPage() -> impl IntoView {
    let navigate = use_navigate();

    // Tasks 1 & 2: milestone events recorded in the story DB.
    let story_ver = db::version("story");
    let reminder_on = create_rw_signal(false);
    let first_steps = create_rw_signal(false);
    create_effect(move |_| {
        story_ver.get();
        spawn_local(async move {
            reminder_on.set(story::get_flag(story::STEPS_REMINDER).await);
            first_steps.set(story::get_flag(story::FIRST_STEPS).await);
        });
    });

    // Task 3: consecutive-day steps streak (a progress aggregate).
    let steps_ver = db::version("step_entries");
    let streak = create_rw_signal(0u32);
    create_effect(move |_| {
        steps_ver.get();
        spawn_local(async move {
            let dates: Vec<String> = local::list_step_entries().await
                .into_iter().map(|e| e.date).collect();
            streak.set(story::consecutive_day_streak(&dates));
        });
    });

    let paragraphs = [
        "story.act.p1", "story.act.p2", "story.act.p3", "story.act.p4",
        "story.act.p5", "story.act.p6", "story.act.p7", "story.act.p8",
    ];
    let body = paragraphs.iter().map(|&key| view! {
        <p class="is-size-6" style="line-height: 1.55; margin: 0 0 14px 0;">{move || t(key)}</p>
    }).collect_view();

    view! {
        <div style=PAGE_BG>
            <div style="display: flex; align-items: center; padding: 12px 16px;">
                <button
                    style="appearance: none; -webkit-appearance: none; border: none; background: none; cursor: pointer; padding: 4px; font: inherit;"
                    class="is-size-5"
                    on:click={
                        let nav = navigate.clone();
                        move |_| nav("/", Default::default())
                    }
                >
                    <span class="has-text-link">{move || t("common.back")}</span>
                </button>
            </div>

            <h1 class="is-size-1 has-text-weight-bold" style="margin: 0 16px 16px 16px;">{move || t("story.ch1.activity")}</h1>

            <div style="padding: 0 16px 8px 16px;">
                {body}
                <p class="is-size-6 has-text-weight-semibold" style="margin: 0 0 6px 0;">{move || t("story.act.howto_title")}</p>
                <p class="is-size-6" style="line-height: 1.55; margin: 0;">{move || t("story.act.howto")}</p>
            </div>

            // ---- Tasks ----
            <div style="padding: 16px 16px 0 16px;">
                <p class="is-size-7 has-text-grey-light" style="text-transform: uppercase; letter-spacing: 0.02em; margin: 0 0 8px 4px;">
                    {move || t("story.act.task_label")}
                </p>
                <div style=CARD>
                    <TaskRow done=Signal::derive(move || reminder_on.get()) text="story.act.task1" />
                    <div style="border-bottom: 0.5px solid var(--bulma-border-weak); margin-left: 50px;"></div>
                    <TaskRow done=Signal::derive(move || first_steps.get()) text="story.act.task2" />
                    <div style="border-bottom: 0.5px solid var(--bulma-border-weak); margin-left: 50px;"></div>
                    <TaskRow done=Signal::derive(move || streak.get() >= 7) text="story.act.task3" />
                    <div style="padding: 0 16px 12px 50px;">
                        <span class="is-size-7 has-text-grey-light">
                            {move || format!("{}: {}/7", t("story.act.streak_label"), streak.get())}
                        </span>
                    </div>
                </div>

                {move || first_steps.get().then(|| view! {
                    <p class="is-size-6 has-text-weight-semibold has-text-success" style="margin-top: 16px;">
                        {move || t("story.act.next_unlocked")}
                    </p>
                })}

                <button
                    class="button is-link is-fullwidth is-medium"
                    style="margin-top: 16px;"
                    on:click={
                        let nav = navigate.clone();
                        move |_| nav("/steps", Default::default())
                    }
                >
                    {move || t("story.act.record_steps")}
                </button>
            </div>

            <div style="height: 40px;"></div>
        </div>
    }
}

#[component]
fn TaskRow(done: Signal<bool>, text: &'static str) -> impl IntoView {
    view! {
        <div style="display: flex; align-items: flex-start; gap: 12px; padding: 14px 16px;">
            {move || if done.get() {
                view! { <span style="font-size: 22px; width: 22px; text-align: center;">"\u{2705}"</span> }.into_view()
            } else {
                view! { <span style="font-size: 22px; width: 22px; text-align: center;">"\u{23f3}"</span> }.into_view()
            }}
            <span class="is-size-6" style="flex: 1; line-height: 1.4;">{move || t(text)}</span>
        </div>
    }
}
