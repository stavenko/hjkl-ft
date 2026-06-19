use leptos::*;
use leptos_router::*;

use crate::services::story_dsl::{self, Block, Engine, EngineSnapshot, Loc, Section};
use crate::services::{db, i18n, i18n::t, profile, story};

const PAGE_BG: &str = "background: var(--bulma-background); min-height: 100vh; padding: 0; margin: -0.75rem;";
const CARD: &str = "background: var(--bulma-scheme-main); border-radius: 12px; overflow: hidden;";

fn tr(l: &Loc) -> String {
    match i18n::get_lang() {
        i18n::Lang::En => l.en.clone(),
        i18n::Lang::Ru => l.ru.clone(),
    }
}

fn para(text: String) -> View {
    view! { <p class="is-size-6" style="line-height: 1.55; margin: 0 0 14px 0;">{text}</p> }.into_view()
}

fn task_row(done: bool, title: String) -> View {
    let icon = if done { "\u{2705}" } else { "\u{23f3}" };
    view! {
        <div style="display: flex; align-items: flex-start; gap: 12px; padding: 14px 16px;">
            <span style="font-size: 22px; width: 22px; text-align: center;">{icon}</span>
            <span class="is-size-6" style="flex: 1; line-height: 1.4;">{title}</span>
        </div>
    }
    .into_view()
}

/// Generic story-section page (`/story/:id`): renders the section's content
/// blocks (sex/lang-filtered prose, headings, lists, the task list, and named
/// widgets) from the DSL, runs its `on_open` actions, and shows a back link.
#[component]
pub fn StorySectionPage() -> impl IntoView {
    let params = use_params_map();
    let id = move || params.with(|p| p.get("id").cloned().unwrap_or_default());

    // Engine snapshot for task done-states / section completeness.
    let snap = create_rw_signal(None::<EngineSnapshot>);
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

    // Run the section's on_open actions when the section changes.
    create_effect(move |_| {
        let sid = id();
        if let Some((_, sec)) = story_dsl::find_section(&sid) {
            for a in &sec.on_open {
                let a = a.clone();
                spawn_local(async move { story::run_action(&a).await; });
            }
        }
    });

    let sex = profile::get_sex().map(|s| match s {
        profile::Sex::Male => "male",
        profile::Sex::Female => "female",
    });

    view! {
        <div style=PAGE_BG>
            <div style="display: flex; align-items: center; padding: 12px 16px;">
                <A href="/" class="is-size-5 has-text-link"
                    attr:style="padding: 4px; text-decoration: none;">
                    {move || t("common.back")}
                </A>
            </div>

            {move || {
                let sid = id();
                let Some((_, sec)) = story_dsl::find_section(&sid) else {
                    return view! { <p style="padding: 16px;">{"\u{2014}"}</p> }.into_view();
                };
                let snapshot = snap.get();
                let engine = snapshot.as_ref().map(|s| Engine::new(story_dsl::story(), s));

                let body = sec
                    .blocks
                    .iter()
                    .filter(|b| match (&b.sex, sex) {
                        (Some(want), Some(cur)) => want == cur,
                        (Some(_), None) => false,
                        (None, _) => true,
                    })
                    .map(|b| render_block(b, sec, engine.as_ref()))
                    .collect_view();

                view! {
                    <h1 class="is-size-1 has-text-weight-bold" style="margin: 0 16px 16px 16px;">{tr(&sec.title)}</h1>
                    <div style="padding: 0 16px 8px 16px;">{body}</div>
                    <div style="height: 40px;"></div>
                }.into_view()
            }}
        </div>
    }
}

fn render_block(b: &Block, sec: &'static Section, engine: Option<&Engine>) -> View {
    if let Some(key) = &b.text_key {
        return para(t(key).to_string());
    }
    if let Some(loc) = &b.text {
        return para(tr(loc));
    }
    if let Some(key) = &b.heading {
        return view! {
            <p class="is-size-6 has-text-weight-semibold" style="line-height: 1.55; margin: 0 0 8px 0;">{t(key)}</p>
        }
        .into_view();
    }
    if let Some(items) = &b.list {
        let lis = items
            .iter()
            .map(|k| view! { <li class="is-size-6" style="margin-bottom: 8px; line-height: 1.5;">{t(k)}</li> })
            .collect_view();
        return view! { <ol style="margin: 0 0 14px 0; padding-left: 22px;">{lis}</ol> }.into_view();
    }
    if b.tasks {
        let rows = sec
            .tasks
            .iter()
            .map(|tid| {
                let done = engine.map(|e| e.task_closed(tid)).unwrap_or(false);
                let title = engine.and_then(|e| e.task(tid)).map(|t| tr(&t.title)).unwrap_or_default();
                task_row(done, title)
            })
            .collect_view();
        let complete = engine.map(|e| e.section_complete(sec)).unwrap_or(false);
        return view! {
            <div style="margin: 16px 0 0 0;">
                <p class="is-size-7 has-text-grey-light" style="text-transform: uppercase; letter-spacing: 0.02em; margin: 0 0 8px 4px;">
                    {move || t("story.section_task_label")}
                </p>
                <div style=CARD>{rows}</div>
                {complete.then(|| view! {
                    <p class="is-size-6 has-text-weight-semibold has-text-success" style="margin-top: 16px;">
                        {move || t("story.section_done")}
                    </p>
                })}
            </div>
        }
        .into_view();
    }
    if let Some(w) = &b.widget {
        return render_widget(w);
    }
    ().into_view()
}

/// Widget registry (named, parameterized). Grows as sections migrate.
fn render_widget(w: &story_dsl::WidgetRef) -> View {
    match w.id.as_str() {
        "cta" => {
            let route = w.param("route").unwrap_or("/").to_string();
            let label = w.param("label").unwrap_or("common.back").to_string();
            view! {
                <div style="padding: 16px 16px 0 16px;">
                    <A href=route class="button is-link is-fullwidth is-medium">
                        {move || t(&label)}
                    </A>
                </div>
            }
            .into_view()
        }
        other => {
            leptos::logging::warn!("story: unknown widget '{other}'");
            ().into_view()
        }
    }
}
