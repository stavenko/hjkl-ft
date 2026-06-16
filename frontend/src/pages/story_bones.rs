use leptos::*;
use leptos_router::*;

use crate::services::{db, i18n::t, story};

const PAGE_BG: &str = "background: var(--bulma-background); min-height: 100vh; padding: 0; margin: -0.75rem;";
const CARD: &str = "background: var(--bulma-scheme-main); border-radius: 12px; overflow: hidden;";

#[component]
pub fn StoryBonesPage() -> impl IntoView {
    let navigate = use_navigate();

    // Task: a diary entry with a non-zero waste value was saved.
    let story_ver = db::version("story");
    let bones_waste = create_rw_signal(false);
    create_effect(move |_| {
        story_ver.get();
        spawn_local(async move {
            bones_waste.set(story::get_flag(story::BONES_WASTE_ENTERED).await);
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

            <h1 class="is-size-1 has-text-weight-bold" style="margin: 0 16px 16px 16px;">{move || t("story.ch1.bones")}</h1>

            <div style="padding: 0 16px 8px 16px;">
                <p class="is-size-6" style="line-height: 1.55; margin: 0 0 14px 0;">{move || t("story.bones.p1")}</p>
                <p class="is-size-6" style="line-height: 1.55; margin: 0 0 14px 0;">{move || t("story.bones.p2")}</p>
                <p class="is-size-6" style="line-height: 1.55; margin: 0 0 14px 0;">{move || t("story.bones.p3")}</p>
                <p class="is-size-6" style="line-height: 1.55; margin: 0 0 8px 0;">{move || t("story.bones.p4")}</p>
            </div>

            // ---- Task ----
            <div style="padding: 16px 16px 0 16px;">
                <p class="is-size-7 has-text-grey-light" style="text-transform: uppercase; letter-spacing: 0.02em; margin: 0 0 8px 4px;">
                    {move || t("story.bones.task_label")}
                </p>
                <div style=CARD>
                    <TaskRow done=Signal::derive(move || bones_waste.get()) text="story.bones.task1" />
                </div>

                {move || bones_waste.get().then(|| view! {
                    <p class="is-size-6 has-text-weight-semibold has-text-success" style="margin-top: 16px;">
                        {move || t("story.bones.next_unlocked")}
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
                    {move || t("story.bones.open_diary")}
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
