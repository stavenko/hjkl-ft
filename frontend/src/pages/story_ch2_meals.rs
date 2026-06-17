use leptos::*;
use leptos_router::*;

use crate::services::{i18n::t, story};

const PAGE_BG: &str = "background: var(--bulma-background); min-height: 100vh; padding: 0; margin: -0.75rem;";

#[component]
pub fn StoryCh2MealsPage() -> impl IntoView {
    let navigate = use_navigate();

    // Opening this section unlocks the meal-split UI in the diary and completes
    // the section's task. Set the flag once on mount.
    spawn_local(async move {
        if !story::get_flag(story::MEAL_SPLIT_UNLOCKED).await {
            story::set_flag(story::MEAL_SPLIT_UNLOCKED, true).await;
        }
    });

    let paragraphs = ["story.ch2.meals.p1", "story.ch2.meals.p2", "story.ch2.meals.p3", "story.ch2.meals.advice"];
    let body = paragraphs.iter().map(|&key| view! {
        <p class="is-size-6" style="line-height: 1.55; margin: 0 0 14px 0;">{move || t(key)}</p>
    }).collect_view();

    view! {
        <div style=PAGE_BG>
            <div style="display: flex; align-items: center; padding: 12px 16px;">
                <button
                    style="appearance: none; -webkit-appearance: none; border: none; background: none; cursor: pointer; padding: 4px; font: inherit;"
                    class="is-size-5"
                    on:click={ let nav = navigate.clone(); move |_| nav("/", Default::default()) }
                >
                    <span class="has-text-link">{move || t("common.back")}</span>
                </button>
            </div>

            <h1 class="is-size-1 has-text-weight-bold" style="margin: 0 16px 16px 16px;">{move || t("story.ch2.s6")}</h1>

            <div style="padding: 0 16px 8px 16px;">
                {body}
            </div>

            <p class="is-size-6 has-text-weight-semibold has-text-success" style="margin: 8px 16px 0 16px;">
                {move || t("story.ch2.meals.unlocked")}
            </p>

            <div style="padding: 16px 16px 0 16px;">
                <button
                    class="button is-link is-fullwidth is-medium"
                    on:click={ let nav = navigate.clone(); move |_| nav("/diary", Default::default()) }
                >
                    {move || t("story.ch2.mistake.open_diary")}
                </button>
            </div>

            <div style="height: 40px;"></div>
        </div>
    }
}
