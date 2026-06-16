use leptos::*;
use leptos_router::*;

use crate::services::{db, i18n::t, local, story};

const PAGE_BG: &str = "background: var(--bulma-background); min-height: 100vh; padding: 0; margin: -0.75rem;";
const CARD: &str = "background: var(--bulma-scheme-main); border-radius: 12px; overflow: hidden;";

#[component]
pub fn StoryAccountingPage() -> impl IntoView {
    let navigate = use_navigate();

    // Tasks 1 & 2 status: milestone events recorded in the story DB — the
    // weigh-in reminder was enabled, and a measurement was made.
    let story_ver = db::version("story");
    let weigh_in_on = create_rw_signal(false);
    let first_done = create_rw_signal(false);
    create_effect(move |_| {
        story_ver.get();
        spawn_local(async move {
            weigh_in_on.set(story::get_flag(story::WEIGH_IN_REMINDER).await);
            first_done.set(story::get_flag(story::FIRST_WEIGH).await);
        });
    });

    // Task 3 status: the consecutive-day weigh-in streak (a progress aggregate).
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

    let paragraphs_top = ["story.acc.p1", "story.acc.p2", "story.acc.p3", "story.acc.p4"];
    let top = paragraphs_top.iter().map(|&key| view! {
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

            <h1 class="is-size-1 has-text-weight-bold" style="margin: 0 16px 16px 16px;">{move || t("story.ch1.accounting")}</h1>

            <div style="padding: 0 16px 8px 16px;">
                {top}
                <p class="is-size-6" style="line-height: 1.55; margin: 0 0 8px 0;">{move || t("story.acc.p5")}</p>
                <ul style="margin: 0 0 14px 0; padding-left: 22px; list-style: disc;">
                    <li class="is-size-6" style="margin-bottom: 6px; line-height: 1.5;">{move || t("story.acc.li_weight")}</li>
                    <li class="is-size-6" style="line-height: 1.5;">{move || t("story.acc.li_calories")}</li>
                </ul>
                <p class="is-size-6" style="line-height: 1.55; margin: 0 0 14px 0;">{move || t("story.acc.p6")}</p>
                <p class="is-size-6" style="line-height: 1.55; margin: 0 0 14px 0;">{move || t("story.acc.p7")}</p>

                <p class="is-size-6 has-text-weight-semibold" style="margin: 0 0 6px 0;">{move || t("story.acc.howto_title")}</p>
                <p class="is-size-6" style="line-height: 1.55; margin: 0;">{move || t("story.acc.howto")}</p>
            </div>

            // ---- Tasks ----
            <div style="padding: 16px 16px 0 16px;">
                <p class="is-size-7 has-text-grey-light" style="text-transform: uppercase; letter-spacing: 0.02em; margin: 0 0 8px 4px;">
                    {move || t("story.acc.task_label")}
                </p>
                <div style=CARD>
                    <TaskRow done=Signal::derive(move || weigh_in_on.get()) text="story.acc.task1" />
                    <div style="border-bottom: 0.5px solid var(--bulma-border-weak); margin-left: 50px;"></div>
                    <TaskRow done=Signal::derive(move || first_done.get()) text="story.acc.task2" />
                    <div style="border-bottom: 0.5px solid var(--bulma-border-weak); margin-left: 50px;"></div>
                    <TaskRow done=Signal::derive(move || streak.get() >= 7) text="story.acc.task3" />
                    <div style="padding: 0 16px 12px 50px;">
                        <span class="is-size-7 has-text-grey-light">
                            {move || format!("{}: {}/7", t("story.acc.streak_label"), streak.get())}
                        </span>
                    </div>
                </div>

                {move || weigh_in_on.get().then(|| view! {
                    <p class="is-size-6 has-text-weight-semibold has-text-success" style="margin-top: 16px;">
                        {move || t("story.acc.next_unlocked")}
                    </p>
                })}
                {move || (streak.get() >= 7).then(|| view! {
                    <p class="is-size-6 has-text-weight-semibold has-text-success" style="margin-top: 8px;">
                        {move || t("story.acc.chapter_unlocked")}
                    </p>
                })}

                <button
                    class="button is-link is-fullwidth is-medium"
                    style="margin-top: 16px;"
                    on:click={
                        let nav = navigate.clone();
                        move |_| nav("/settings", Default::default())
                    }
                >
                    {move || t("story.setup.open_settings")}
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
