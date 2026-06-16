use leptos::*;
use leptos_router::*;

use crate::services::i18n::t;

const PAGE_BG: &str = "background: var(--bulma-background); min-height: 100vh; padding: 0; margin: -0.75rem;";

/// Closing section of chapter 1 «Что дальше» — insists on getting comfortable
/// with the app and steadily improving counting discipline. No task/gate here
/// (the subscription moves to an earlier stage, before the story).
#[component]
pub fn StoryNextPage() -> impl IntoView {
    let navigate = use_navigate();

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

            <h1 class="is-size-1 has-text-weight-bold" style="margin: 0 16px 16px 16px;">{move || t("story.ch1.next")}</h1>

            <div style="padding: 0 16px 8px 16px;">
                <p class="is-size-6" style="line-height: 1.55; margin: 0 0 14px 0;">{move || t("story.next.p_intro")}</p>

                <p class="is-size-6 has-text-weight-semibold" style="line-height: 1.55; margin: 0 0 8px 0;">{move || t("story.next.rules_label")}</p>
                <ol style="margin: 0 0 14px 0; padding-left: 22px;">
                    <li class="is-size-6" style="margin-bottom: 8px; line-height: 1.5;">{move || t("story.next.rule1")}</li>
                    <li class="is-size-6" style="margin-bottom: 8px; line-height: 1.5;">{move || t("story.next.rule2")}</li>
                    <li class="is-size-6" style="line-height: 1.5;">{move || t("story.next.rule3")}</li>
                </ol>

                <p class="is-size-6" style="line-height: 1.55; margin: 0 0 14px 0;">{move || t("story.next.p_discipline")}</p>

                <p class="is-size-6 has-text-weight-semibold" style="line-height: 1.55; margin: 0 0 8px 0;">{move || t("story.next.focus_label")}</p>
                <ol style="margin: 0 0 14px 0; padding-left: 22px;">
                    <li class="is-size-6" style="margin-bottom: 8px; line-height: 1.5;">{move || t("story.next.focus1")}</li>
                    <li class="is-size-6" style="margin-bottom: 8px; line-height: 1.5;">{move || t("story.next.focus2")}</li>
                    <li class="is-size-6" style="line-height: 1.5;">{move || t("story.next.focus3")}</li>
                </ol>

                <p class="is-size-6" style="line-height: 1.55; margin: 0 0 14px 0;">{move || t("story.next.p_goals")}</p>
                <p class="is-size-6" style="line-height: 1.55; margin: 0 0 8px 0;">{move || t("story.next.p_report")}</p>
            </div>

            <div style="padding: 16px 16px 0 16px;">
                <button
                    class="button is-link is-fullwidth is-medium"
                    on:click={
                        let nav = navigate.clone();
                        move |_| nav("/diary", Default::default())
                    }
                >
                    {move || t("story.next.open_diary")}
                </button>
            </div>

            <div style="height: 40px;"></div>
        </div>
    }
}
