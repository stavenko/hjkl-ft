use leptos::*;
use leptos_router::*;

use crate::services::{db, i18n::t, local, story};

const PAGE_BG: &str = "background: var(--bulma-background); min-height: 100vh; padding: 0; margin: -0.75rem;";
const CARD: &str = "background: var(--bulma-scheme-main); border-radius: 12px; overflow: hidden;";

/// Evening-protein threshold (grams) for the positive feedback line.
const GOOD_EVENING_PROTEIN: f64 = 30.0;

#[component]
pub fn StoryCh2NightPage() -> impl IntoView {
    let navigate = use_navigate();

    // Opening this section + viewing today's feedback completes the task.
    spawn_local(async move {
        if !story::get_flag(story::NIGHT_FEEDBACK_VIEWED).await {
            story::set_flag(story::NIGHT_FEEDBACK_VIEWED, true).await;
        }
    });

    let paragraphs = ["story.ch2.night.p1", "story.ch2.night.p2", "story.ch2.night.p3"];
    let body = paragraphs.iter().map(|&key| view! {
        <p class="is-size-6" style="line-height: 1.55; margin: 0 0 14px 0;">{move || t(key)}</p>
    }).collect_view();

    // Today's evening protein (Dinner + NightSnack buckets).
    let diary_ver = db::version("diary");
    let evening_protein = create_rw_signal(0.0_f64);
    create_effect(move |_| {
        diary_ver.get();
        spawn_local(async move {
            let today = chrono::Local::now().format("%Y-%m-%d").to_string();
            evening_protein.set(local::evening_protein_on(&today).await);
        });
    });
    let good = move || evening_protein.get() >= GOOD_EVENING_PROTEIN;

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

            <h1 class="is-size-1 has-text-weight-bold" style="margin: 0 16px 16px 16px;">{move || t("story.ch2.s7")}</h1>

            <div style="padding: 0 16px 8px 16px;">
                {body}
            </div>

            // ---- Today's evening feedback ----
            <div style="padding: 16px 16px 0 16px;">
                <p class="is-size-7 has-text-grey-light" style="text-transform: uppercase; letter-spacing: 0.02em; margin: 0 0 8px 4px;">
                    {move || t("story.ch2.night.feedback_label")}
                </p>
                <div style=CARD>
                    <div style="display: flex; align-items: flex-start; gap: 12px; padding: 14px 16px;">
                        {move || if good() {
                            view! {
                                <span style="font-size: 22px; width: 22px; text-align: center;">"\u{1f4aa}"</span>
                                <span class="is-size-6" style="flex: 1; line-height: 1.4;">{move || t("story.ch2.night.feedback_good")}</span>
                            }.into_view()
                        } else {
                            view! {
                                <span style="font-size: 22px; width: 22px; text-align: center;">"\u{1f319}"</span>
                                <span class="is-size-6" style="flex: 1; line-height: 1.4;">{move || t("story.ch2.night.feedback_hint")}</span>
                            }.into_view()
                        }}
                    </div>
                </div>
            </div>

            <div style="height: 40px;"></div>
        </div>
    }
}
