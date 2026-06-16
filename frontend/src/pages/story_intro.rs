use leptos::*;
use leptos_router::*;

use crate::services::{db, i18n::t, story};

const PAGE_BG: &str = "background: var(--bulma-background); min-height: 100vh; padding: 0; margin: -0.75rem;";
const CARD: &str = "background: var(--bulma-scheme-main); border-radius: 12px; overflow: hidden;";

/// Schematic of the three required poses (front / side / back) — clean filled
/// silhouettes (head + rounded torso + limbs), not stick figures.
const POSE_SCHEMA: &str = r#"<svg viewBox="0 0 300 116" fill="currentColor" style="width:100%;max-width:320px;color:var(--bulma-text-weak)">
<g transform="translate(30,8)"><circle cx="20" cy="9" r="7.5"/><rect x="10.5" y="17" width="19" height="37" rx="8.5"/><rect x="4.5" y="19" width="5.5" height="31" rx="2.7"/><rect x="30" y="19" width="5.5" height="31" rx="2.7"/><rect x="12.5" y="51" width="6.5" height="45" rx="3.2"/><rect x="21" y="51" width="6.5" height="45" rx="3.2"/></g>
<g transform="translate(130,8)"><circle cx="20" cy="9" r="7.5"/><path d="M27.5 7 l4.5 2 -4.5 2 z"/><rect x="14.5" y="17" width="11" height="37" rx="5.5"/><rect x="17.5" y="21" width="5" height="29" rx="2.5"/><rect x="15.5" y="51" width="8.5" height="45" rx="4.2"/></g>
<g transform="translate(230,8)"><circle cx="20" cy="9" r="7.5"/><rect x="10.5" y="17" width="19" height="37" rx="8.5"/><rect x="4.5" y="19" width="5.5" height="31" rx="2.7"/><rect x="30" y="19" width="5.5" height="31" rx="2.7"/><rect x="12.5" y="51" width="6.5" height="45" rx="3.2"/><rect x="21" y="51" width="6.5" height="45" rx="3.2"/><line x1="20" y1="20" x2="20" y2="50" stroke="var(--bulma-scheme-main)" stroke-width="1.6"/></g>
</svg>"#;

#[component]
pub fn StoryIntroPage() -> impl IntoView {
    let navigate = use_navigate();

    // The section's task: take the three progress photos. Required to advance —
    // completes once all poses are taken (sets PROGRESS_PHOTOS_TAKEN).
    let story_ver = db::version("story");
    let photos_done = create_rw_signal(false);
    create_effect(move |_| {
        story_ver.get();
        spawn_local(async move {
            photos_done.set(story::get_flag(story::PROGRESS_PHOTOS_TAKEN).await);
        });
    });

    let paragraphs = [
        "story.intro.p1", "story.intro.p2", "story.intro.p3", "story.intro.p4",
        "story.intro.p5", "story.intro.p6", "story.intro.p7", "story.intro.p8",
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

            <h1 class="is-size-1 has-text-weight-bold" style="margin: 0 16px 16px 16px;">{move || t("story.ch1.intro")}</h1>

            <div style="padding: 0 16px 8px 16px;">
                {body}
            </div>

            // ---- Task: progress photos (front / side / back) — required ----
            <div style="padding: 16px 16px 0 16px;">
                <p class="is-size-7 has-text-grey-light" style="text-transform: uppercase; letter-spacing: 0.02em; margin: 0 0 8px 4px;">
                    {move || t("story.intro.photo_task_label")}
                </p>
                <div style=CARD>
                    <div style="padding: 14px 16px;">
                        <p class="is-size-6" style="line-height: 1.55; margin: 0 0 12px 0;">{move || t("story.intro.photo_desc")}</p>
                        <div style="display: flex; justify-content: center;" inner_html=POSE_SCHEMA></div>
                        <div style="display: flex; justify-content: space-around; max-width: 300px; margin: 4px auto 0 auto;">
                            <span class="is-size-7 has-text-grey">{move || t("progress.pose_front")}</span>
                            <span class="is-size-7 has-text-grey">{move || t("progress.pose_side")}</span>
                            <span class="is-size-7 has-text-grey">{move || t("progress.pose_back")}</span>
                        </div>
                    </div>
                    <div style="border-bottom: 0.5px solid var(--bulma-border-weak);"></div>
                    <div style="display: flex; align-items: center; gap: 12px; padding: 14px 16px;">
                        <span style="font-size: 22px; width: 22px; text-align: center;">
                            {move || if photos_done.get() { "\u{2705}" } else { "\u{23f3}" }}
                        </span>
                        <span class="is-size-6" style="flex: 1; line-height: 1.4;">{move || t("story.intro.photo_check")}</span>
                        <button class="button is-link is-small"
                            on:click={ let nav = navigate.clone(); move |_| nav("/progress", Default::default()) }
                        >{move || t("progress.capture")}</button>
                    </div>
                </div>
                {move || photos_done.get().then(|| view! {
                    <p class="is-size-7 has-text-success" style="margin: 12px 4px 0 4px;">
                        {move || t("story.intro.unlocked_hint")}
                    </p>
                })}
            </div>

            <div style="height: 40px;"></div>
        </div>
    }
}
