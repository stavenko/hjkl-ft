use leptos::*;

use crate::services::story_dsl::{self, Engine, Loc};
use crate::services::{db, i18n, story, subscription};

const IOS_BG: &str = "background: var(--bulma-background); min-height: 100vh; padding: 16px; margin: -0.75rem;";
const IOS_CARD: &str = "background: var(--bulma-scheme-main); border-radius: 12px; overflow: hidden;";
const IOS_SECTION_LABEL: &str = "text-transform: uppercase; letter-spacing: 0.02em; padding: 24px 0 8px 16px; margin: 0;";
const IOS_SEPARATOR: &str = "border-bottom: 0.5px solid var(--bulma-border-weak); margin-left: 52px;";
const ROW_STYLE: &str = "padding: 12px 16px; display: flex; align-items: center; gap: 12px; color: inherit; text-decoration: none;";

/// Pick the localized string for the current language (reactive: reads the lang signal).
fn tr(l: &Loc) -> String {
    match i18n::get_lang() {
        i18n::Lang::En => l.en.clone(),
        i18n::Lang::Ru => l.ru.clone(),
    }
}

#[component]
pub fn StoryPage() -> impl IntoView {
    // Opening the hub acknowledges completed tasks (clears the nav-icon dot).
    spawn_local(async move {
        story::ack_done_tasks().await;
        story::refresh_attention();
    });

    // Reactive engine snapshot, rebuilt whenever any sensor source changes.
    let snap = create_rw_signal(None::<story_dsl::EngineSnapshot>);
    let reload = move || spawn_local(async move { snap.set(Some(story::engine_snapshot().await)); });

    let story_ver = db::version("story");
    let weight_ver = db::version("weight_entries");
    let steps_ver = db::version("step_entries");
    let diary_ver = db::version("diary");
    let goals_ver = db::version("goals");
    let summaries_ver = db::version("summaries");
    create_effect(move |_| {
        story_ver.get();
        weight_ver.get();
        steps_ver.get();
        diary_ver.get();
        goals_ver.get();
        summaries_ver.get();
        reload();
    });
    // Subscription gates chapter 2; refresh it live, then rebuild the snapshot.
    spawn_local(async move {
        let _ = subscription::status().await;
        reload();
    });

    // Section routes the user has already opened — drives the per-row "new" dot.
    let seen = create_rw_signal(std::collections::HashSet::<String>::new());
    create_effect(move |_| {
        story_ver.get();
        spawn_local(async move { seen.set(story::seen_routes().await); });
    });

    // Report to the backend (next to the persona) which chapters are AVAILABLE in the UI. The
    // first chapter unlocking = the user «entered the system» — the Mini App's access signal.
    // Deduped per chapter per device inside `report_chapter_available`.
    create_effect(move |_| {
        let Some(s) = snap.get() else { return };
        let st = story_dsl::story();
        let e = Engine::new(st, &s);
        for ch in &st.chapters {
            if e.chapter_open(ch) {
                crate::services::auth::report_chapter_available(&ch.id);
            }
        }
    });

    let new_dot = || view! {
        <span attr:data-testid="story-section-new-dot"
            style="width: 8px; height: 8px; border-radius: 50%; background: var(--bulma-danger); flex: none;"></span>
    };

    view! {
        <div style=IOS_BG>
            <h1 class="is-size-1 has-text-weight-bold" style="margin: 0 0 8px 0;">{move || i18n::t("story.title")}</h1>

            {move || {
                let Some(s) = snap.get() else { return ().into_view() };
                let st = story_dsl::story();
                let e = Engine::new(st, &s);
                let seen_set = seen.get();

                // ── Single active-task list ──
                let active: Vec<String> = e
                    .active_tasks()
                    .iter()
                    .map(|t| match (t.short.as_ref(), e.task_counter(&t.id)) {
                        // Streak/counter tasks get a compact "<label>: X/N дней" form
                        // on the hub plate, distinct from the full task title.
                        (Some(short), Some((cur, tgt))) => tr(short)
                            .replace("{x}", &cur.to_string())
                            .replace("{n}", &tgt.to_string()),
                        _ => story::fill_task_target(&t.id, tr(&t.title), &e.snap.progress),
                    })
                    .collect();
                let total = st.tasks.len();
                let done = st.tasks.iter().filter(|t| e.task_closed(&t.id)).count();
                let active_view = (!active.is_empty()).then(|| {
                    let items = active.into_iter().map(|title| view! {
                        <li class="is-size-6" style="margin-bottom: 4px;">{title}</li>
                    }).collect_view();
                    view! {
                        <p class="is-size-7 has-text-grey-light" style=IOS_SECTION_LABEL>{i18n::t("story.active_tasks")}</p>
                        <div style=format!("{} padding: 12px 16px;", IOS_CARD)>
                            <ul style="margin: 0; padding-left: 1.1rem; list-style: disc;">{items}</ul>
                        </div>
                    }
                });

                // ── Chapters ──
                let chapters = st.chapters.iter().map(|ch| {
                    let open = e.chapter_open(ch);
                    let lock = if open { "" } else { "\u{1f512}" };
                    let head = format!("{} \u{00b7} {} {}", tr_chapter_label(&ch.id), tr(&ch.title), lock);

                    let rows = ch.sections.iter().enumerate().map(|(i, sec)| {
                        let unlocked = e.section_unlocked(ch, i);
                        let route = sec.legacy_route.clone()
                            .unwrap_or_else(|| format!("/story/{}", sec.id));
                        let title = tr(&sec.title);
                        let icon = sec.icon.clone();
                        let icon_span = (!icon.is_empty()).then(|| view! {
                            <span style="font-size: 20px; width: 26px; text-align: center; flex-shrink: 0;">{icon}</span>
                        });
                        let is_new = unlocked && !seen_set.contains(&route);
                        let sep = (i > 0).then(|| view! { <div style=IOS_SEPARATOR></div> });
                        let row = if unlocked {
                            view! {
                                <a href=route style=format!("{}cursor: pointer;", ROW_STYLE)>
                                    {icon_span}
                                    <span class="is-size-6" style="flex: 1;">{title}</span>
                                    {is_new.then(new_dot)}
                                    <span style="color: var(--bulma-text-weak); font-size: 18px;">"\u{203a}"</span>
                                </a>
                            }.into_view()
                        } else {
                            view! {
                                <div style=format!("{}opacity: 0.45;", ROW_STYLE)>
                                    {icon_span}
                                    <span class="is-size-6" style="flex: 1;">{title}</span>
                                    <span style="font-size: 15px;">"\u{1f512}"</span>
                                </div>
                            }.into_view()
                        };
                        view! { {sep}{row} }
                    }).collect_view();

                    view! {
                        <p class="is-size-7 has-text-grey-light" style=IOS_SECTION_LABEL>{head}</p>
                        <div style=IOS_CARD>{rows}</div>
                    }
                }).collect_view();

                view! {
                    {active_view}
                    {chapters}
                    <div style="padding: 12px 16px 0 16px;">
                        <p class="is-size-7 has-text-grey-light" style="margin: 0;">
                            {format!("{}: {}/{}", i18n::t("story.tasks_done"), done, total)}
                        </p>
                    </div>
                    <div style="height: 40px;"></div>
                }.into_view()
            }}
        </div>
    }
}

/// "Chapter N" label by chapter id (purely cosmetic numbering for the header).
fn tr_chapter_label(id: &str) -> String {
    // Numbering follows the "ch<N>" id convention.
    let n = id.strip_prefix("ch").and_then(|s| s.parse::<u32>().ok()).unwrap_or(0);
    format!("{} {}", i18n::t("story.chapter"), n)
}
