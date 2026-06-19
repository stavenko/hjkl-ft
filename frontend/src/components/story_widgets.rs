//! Named, self-contained widgets the Story DSL embeds in section content blocks
//! (`{widget: {id: ...}}`). Each is a real `#[component]` with a stable reactive
//! scope and manages its own state, so the generic section page can rebuild its
//! body on section change without remounting / resetting a widget mid-stream.

use leptos::*;
use leptos_router::*;

use crate::services::story_dsl::{self, Engine, EngineSnapshot, Loc};
use crate::services::{db, i18n, i18n::t, story};

const CARD: &str = "background: var(--bulma-scheme-main); border-radius: 12px; overflow: hidden;";

fn tr(l: &Loc) -> String {
    match i18n::get_lang() {
        i18n::Lang::En => l.en.clone(),
        i18n::Lang::Ru => l.ru.clone(),
    }
}

/// Load the engine snapshot into a signal, rebuilding it whenever any sensor
/// source changes. Shared by widgets that need task/section state.
fn snapshot_signal() -> RwSignal<Option<EngineSnapshot>> {
    let snap = create_rw_signal(None::<EngineSnapshot>);
    let reload = move || spawn_local(async move { snap.set(Some(story::engine_snapshot().await)); });
    let vers = [
        db::version("story"),
        db::version("weight_entries"),
        db::version("step_entries"),
        db::version("diary"),
        db::version("goals"),
        db::version("summaries"),
    ];
    create_effect(move |_| {
        for v in &vers {
            v.get();
        }
        reload();
    });
    snap
}

/// The section's task list (checkmark rows + a "section complete" line), driven
/// by the engine. Used by the generic `{tasks: true}` block.
#[component]
pub fn StoryTaskList(section_id: String) -> impl IntoView {
    let snap = snapshot_signal();
    view! {
        <div style="margin: 16px 0 0 0;">
            <p class="is-size-7 has-text-grey-light" style="text-transform: uppercase; letter-spacing: 0.02em; margin: 0 0 8px 4px;">
                {move || t("story.section_task_label")}
            </p>
            {move || {
                let Some(s) = snap.get() else {
                    return view! { <div style=CARD></div> }.into_view();
                };
                let e = Engine::new(story_dsl::story(), &s);
                let Some((_, sec)) = story_dsl::find_section(&section_id) else {
                    return ().into_view();
                };
                let rows = sec.tasks.iter().map(|tid| {
                    let done = e.task_closed(tid);
                    let title = e.task(tid).map(|t| tr(&t.title)).unwrap_or_default();
                    let icon = if done { "\u{2705}" } else { "\u{23f3}" };
                    // Counter tasks (7-day streaks etc.) show a "current/target" sub-line.
                    let counter = e.task_counter(tid).map(|(cur, target)| view! {
                        <div style="padding: 0 16px 10px 50px;">
                            <span class="is-size-7 has-text-grey-light">{format!("{cur}/{target}")}</span>
                        </div>
                    });
                    view! {
                        <div style="display: flex; align-items: flex-start; gap: 12px; padding: 14px 16px;">
                            <span style="font-size: 22px; width: 22px; text-align: center;">{icon}</span>
                            <span class="is-size-6" style="flex: 1; line-height: 1.4;">{title}</span>
                        </div>
                        {counter}
                    }
                }).collect_view();
                let complete = e.section_complete(sec);
                view! {
                    <div style=CARD>{rows}</div>
                    {complete.then(|| view! {
                        <p class="is-size-6 has-text-weight-semibold has-text-success" style="margin-top: 16px;">
                            {move || t("story.section_done")}
                        </p>
                    })}
                }.into_view()
            }}
        </div>
    }
}

/// A full-width navigation button (DSL: `{widget: {id: cta, route, label}}`).
#[component]
pub fn Cta(route: String, label: String) -> impl IntoView {
    view! {
        <div style="padding: 16px 16px 0 16px;">
            <A href=route class="button is-link is-fullwidth is-medium">
                {move || t(&label)}
            </A>
        </div>
    }
}

/// Schematic of the three required poses (front / side / back).
const POSE_SCHEMA: &str = r#"<svg viewBox="0 0 300 116" fill="currentColor" style="width:100%;max-width:320px;color:var(--bulma-text-weak)">
<g transform="translate(30,8)"><circle cx="20" cy="9" r="7.5"/><rect x="10.5" y="17" width="19" height="37" rx="8.5"/><rect x="4.5" y="19" width="5.5" height="31" rx="2.7"/><rect x="30" y="19" width="5.5" height="31" rx="2.7"/><rect x="12.5" y="51" width="6.5" height="45" rx="3.2"/><rect x="21" y="51" width="6.5" height="45" rx="3.2"/></g>
<g transform="translate(130,8)"><circle cx="20" cy="9" r="7.5"/><path d="M27.5 7 l4.5 2 -4.5 2 z"/><rect x="14.5" y="17" width="11" height="37" rx="5.5"/><rect x="17.5" y="21" width="5" height="29" rx="2.5"/><rect x="15.5" y="51" width="8.5" height="45" rx="4.2"/></g>
<g transform="translate(230,8)"><circle cx="20" cy="9" r="7.5"/><rect x="10.5" y="17" width="19" height="37" rx="8.5"/><rect x="4.5" y="19" width="5.5" height="31" rx="2.7"/><rect x="30" y="19" width="5.5" height="31" rx="2.7"/><rect x="12.5" y="51" width="6.5" height="45" rx="3.2"/><rect x="21" y="51" width="6.5" height="45" rx="3.2"/><line x1="20" y1="20" x2="20" y2="50" stroke="var(--bulma-scheme-main)" stroke-width="1.6"/></g>
</svg>"#;

/// Intro task: take the three progress photos (front / side / back). Completes
/// once `PROGRESS_PHOTOS_TAKEN` is set (by the progress page).
#[component]
pub fn ProgressPhotos() -> impl IntoView {
    let story_ver = db::version("story");
    let done = create_rw_signal(false);
    create_effect(move |_| {
        story_ver.get();
        spawn_local(async move { done.set(story::get_flag(story::PROGRESS_PHOTOS_TAKEN).await); });
    });

    view! {
        <div style="margin: 16px 0 0 0;">
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
                        {move || if done.get() { "\u{2705}" } else { "\u{23f3}" }}
                    </span>
                    <span class="is-size-6" style="flex: 1; line-height: 1.4;">{move || t("story.intro.photo_check")}</span>
                    <A href="/progress" class="button is-link is-small">{move || t("progress.capture")}</A>
                </div>
            </div>
            {move || done.get().then(|| view! {
                <p class="is-size-7 has-text-success" style="margin: 12px 4px 0 4px;">
                    {move || t("story.intro.unlocked_hint")}
                </p>
            })}
        </div>
    }
}
