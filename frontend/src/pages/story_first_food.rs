use leptos::*;
use leptos_router::*;

use crate::services::{db, i18n::t, local, story};

const PAGE_BG: &str = "background: var(--bulma-background); min-height: 100vh; padding: 0; margin: -0.75rem;";
const CARD: &str = "background: var(--bulma-scheme-main); border-radius: 12px; overflow: hidden;";

#[component]
pub fn StoryFirstFoodPage() -> impl IntoView {
    let navigate = use_navigate();

    // Opening this section arms the "enter a food" trigger and unlocks the
    // meal/steps reminders in settings.
    spawn_local(async move {
        story::set_flag(story::FIRST_FOOD_ARMED, true).await;
        story::set_flag(story::MEAL_REMINDERS_UNLOCKED, true).await;
        // If the user has already logged food today, count the task as done.
        let today = chrono::Local::now().format("%Y-%m-%d").to_string();
        if !local::list_diary(&today).await.is_empty() {
            story::fire_first_food_if_armed().await;
        }
    });

    let story_ver = db::version("story");
    let done = create_rw_signal(false);
    create_effect(move |_| {
        story_ver.get();
        spawn_local(async move {
            done.set(story::get_flag(story::FIRST_FOOD_DONE).await);
        });
    });

    let ways = ["story.ff.way1", "story.ff.way2", "story.ff.way3"];
    let way_items = ways.iter().map(|&key| view! {
        <li class="is-size-6" style="margin-bottom: 6px; line-height: 1.5;">
            {move || match t(key).split_once(" \u{2014} ") {
                Some((name, rest)) => view! { <strong>{name}</strong>" \u{2014} "{rest} }.into_view(),
                None => view! { {t(key)} }.into_view(),
            }}
        </li>
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

            <h1 class="is-size-1 has-text-weight-bold" style="margin: 0 16px 16px 16px;">{move || t("story.ch1.first_food")}</h1>

            <div style="padding: 0 16px 8px 16px;">
                <p class="is-size-6" style="line-height: 1.55; margin: 0 0 14px 0;">{move || t("story.ff.p1")}</p>
                <p class="is-size-6" style="line-height: 1.55; margin: 0 0 14px 0;">{move || t("story.ff.p2")}</p>
                <p class="is-size-6" style="line-height: 1.55; margin: 0 0 14px 0;">{move || t("story.ff.p3")}</p>

                <p class="is-size-6" style="line-height: 1.55; margin: 0 0 8px 0;">{move || t("story.ff.ways_intro")}</p>
                <ul style="margin: 0 0 16px 0; padding-left: 22px; list-style: disc;">
                    {way_items}
                </ul>

                // ---- How-to with the drawn (+) button ----
                <p class="is-size-6" style="line-height: 1.55; margin: 0 0 10px 0;">{move || t("story.ff.howto_open")}</p>
                <div style="display: flex; justify-content: center; margin: 0 0 14px 0;">
                    <div style="width: 56px; height: 56px; border-radius: 50%; background: var(--bulma-success); color: #fff; display: flex; align-items: center; justify-content: center; font-size: 34px; line-height: 1; box-shadow: 0 4px 12px rgba(0,0,0,0.2);">"+"</div>
                </div>
                <ol style="margin: 0; padding-left: 22px;">
                    <li class="is-size-6" style="margin-bottom: 8px; line-height: 1.5;">{move || t("story.ff.step_new")}</li>
                    <li class="is-size-6" style="margin-bottom: 8px; line-height: 1.5;">{move || t("story.ff.step_name")}</li>
                    <li class="is-size-6" style="margin-bottom: 8px; line-height: 1.5;">{move || t("story.ff.step_add")}</li>
                    <li class="is-size-6" style="line-height: 1.5;">{move || t("story.ff.step_more")}</li>
                </ol>
            </div>

            // ---- Task ----
            <div style="padding: 16px 16px 0 16px;">
                <p class="is-size-7 has-text-grey-light" style="text-transform: uppercase; letter-spacing: 0.02em; margin: 0 0 8px 4px;">
                    {move || t("story.ff.task_label")}
                </p>
                <div style=CARD>
                    <div style="display: flex; align-items: flex-start; gap: 12px; padding: 14px 16px;">
                        {move || if done.get() {
                            view! { <span style="font-size: 22px; width: 22px; text-align: center;">"\u{2705}"</span> }.into_view()
                        } else {
                            view! { <span style="font-size: 22px; width: 22px; text-align: center;">"\u{23f3}"</span> }.into_view()
                        }}
                        <span class="is-size-6 has-text-weight-semibold" style="flex: 1; line-height: 1.4;">{move || t("story.ff.task")}</span>
                    </div>
                </div>

                {move || done.get().then(|| view! {
                    <p class="is-size-6 has-text-weight-semibold has-text-success" style="margin-top: 16px;">
                        {move || t("story.ff.next_unlocked")}
                    </p>
                })}

                <button
                    class="button is-link is-fullwidth is-medium"
                    style="margin-top: 16px;"
                    on:click={
                        let nav = navigate.clone();
                        move |_| nav("/diary", Default::default())
                    }
                >
                    {move || t("story.ff.open_diary")}
                </button>
            </div>

            <div style="height: 40px;"></div>
        </div>
    }
}
