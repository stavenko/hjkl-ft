use leptos::*;
use leptos_router::*;

use crate::services::{db, i18n::t, story};

const PAGE_BG: &str = "background: var(--bulma-background); min-height: 100vh; padding: 0; margin: -0.75rem;";
const CARD: &str = "background: var(--bulma-scheme-main); border-radius: 12px; overflow: hidden;";

#[component]
pub fn StoryCookingPage() -> impl IntoView {
    let navigate = use_navigate();

    // Tasks 1 & 2: milestone events recorded in the story DB.
    let story_ver = db::version("story");
    let dish_created = create_rw_signal(false);
    let dish_in_diary = create_rw_signal(false);
    create_effect(move |_| {
        story_ver.get();
        spawn_local(async move {
            dish_created.set(story::get_flag(story::COOKING_DISH_CREATED).await);
            dish_in_diary.set(story::get_flag(story::COOKING_DISH_IN_DIARY).await);
        });
    });

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

            <h1 class="is-size-1 has-text-weight-bold" style="margin: 0 16px 16px 16px;">{move || t("story.ch1.cooking")}</h1>

            <div style="padding: 0 16px 8px 16px;">
                <p class="is-size-6" style="line-height: 1.55; margin: 0 0 14px 0;">{move || t("story.cook.p1")}</p>
                <p class="is-size-6" style="line-height: 1.55; margin: 0 0 14px 0;">{move || t("story.cook.p2")}</p>
                <p class="is-size-6" style="line-height: 1.55; margin: 0 0 8px 0;">{move || t("story.cook.p3")}</p>
                <ol style="margin: 0; padding-left: 22px;">
                    <li class="is-size-6" style="margin-bottom: 10px; line-height: 1.5;">{move || t("story.cook.step1")}</li>
                    <li class="is-size-6" style="line-height: 1.5;">{move || t("story.cook.step2")}</li>
                </ol>
            </div>

            // ---- Tasks ----
            <div style="padding: 16px 16px 0 16px;">
                <p class="is-size-7 has-text-grey-light" style="text-transform: uppercase; letter-spacing: 0.02em; margin: 0 0 8px 4px;">
                    {move || t("story.cook.task_label")}
                </p>
                <div style=CARD>
                    <TaskRow done=Signal::derive(move || dish_created.get()) text="story.cook.task1" />
                    <div style="border-bottom: 0.5px solid var(--bulma-border-weak); margin-left: 50px;"></div>
                    <TaskRow done=Signal::derive(move || dish_in_diary.get()) text="story.cook.task2" />
                </div>

                {move || (dish_created.get() && dish_in_diary.get()).then(|| view! {
                    <p class="is-size-6 has-text-weight-semibold has-text-success" style="margin-top: 16px;">
                        {move || t("story.cook.next_unlocked")}
                    </p>
                })}

                <button
                    class="button is-link is-fullwidth is-medium"
                    style="margin-top: 16px;"
                    on:click={
                        let nav = navigate.clone();
                        move |_| nav("/recipes", Default::default())
                    }
                >
                    {move || t("story.cook.open_recipes")}
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
