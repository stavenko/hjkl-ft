use leptos::*;
use leptos_router::*;

use crate::services::i18n::t;

const PAGE_BG: &str = "background: var(--bulma-background); min-height: 100vh; padding: 0; margin: -0.75rem;";

#[component]
pub fn StoryCh3BeautyPage() -> impl IntoView {
    let navigate = use_navigate();

    let paragraphs = ["story.ch3.beauty.p1", "story.ch3.beauty.p2", "story.ch3.beauty.p3"];
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

            <h1 class="is-size-1 has-text-weight-bold" style="margin: 0 16px 16px 16px;">{move || t("story.ch3.s2")}</h1>

            <div style="padding: 0 16px 8px 16px;">
                {body}
            </div>

            <div style="height: 40px;"></div>
        </div>
    }
}
