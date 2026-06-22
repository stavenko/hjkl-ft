use leptos::*;
use leptos_router::*;

use crate::components::story_widgets::{
    Cta, GoalStatus, NightFeedback, ProgressPhotos, SetupControls, StoryTaskList, VegTarget,
};
use crate::services::story_dsl::{self, Block, Loc, Section, WidgetRef};
use crate::services::{i18n, i18n::t, profile, story};

const PAGE_BG: &str = "background: var(--bulma-background); min-height: 100vh; padding: 0; margin: -0.75rem;";

fn tr(l: &Loc) -> String {
    match i18n::get_lang() {
        i18n::Lang::En => l.en.clone(),
        i18n::Lang::Ru => l.ru.clone(),
    }
}

/// Generic story-section page (`/story/:id`): renders the section's content
/// blocks from the DSL (sex/lang-filtered prose, headings, lists, the task list,
/// and named widgets), runs its `on_open` actions, with a back link. Stateful
/// widgets are self-contained components, so the body is keyed only on the
/// section id (no remount on data changes).
#[component]
pub fn StorySectionPage() -> impl IntoView {
    let params = use_params_map();
    let id = move || params.with(|p| p.get("id").cloned().unwrap_or_default());

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
                <A href="/" class="is-size-5 has-text-link" attr:style="padding: 4px; text-decoration: none;">
                    {move || t("common.back")}
                </A>
            </div>

            {move || {
                let sid = id();
                let Some((_, sec)) = story_dsl::find_section(&sid) else {
                    return view! { <p style="padding: 16px;">{"\u{2014}"}</p> }.into_view();
                };
                let body = sec
                    .blocks
                    .iter()
                    .filter(|b| match (&b.sex, sex) {
                        (Some(want), Some(cur)) => want == cur,
                        (Some(_), None) => false,
                        (None, _) => true,
                    })
                    .map(|b| render_block(b, sec))
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

/// Render a paragraph string with inline `**bold**` segments.
fn render_rich(s: &str) -> impl IntoView {
    s.split("**")
        .enumerate()
        .map(|(i, seg)| {
            let seg = seg.to_string();
            if i % 2 == 1 {
                view! { <strong>{seg}</strong> }.into_view()
            } else {
                view! { {seg} }.into_view()
            }
        })
        .collect_view()
}

fn render_block(b: &Block, sec: &'static Section) -> View {
    if let Some(key) = &b.text_key {
        return view! { <p class="is-size-6" style="line-height: 1.55; margin: 0 0 14px 0;">{render_rich(t(key))}</p> }.into_view();
    }
    if let Some(loc) = &b.text {
        return view! { <p class="is-size-6" style="line-height: 1.55; margin: 0 0 14px 0;">{render_rich(&tr(loc))}</p> }.into_view();
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
    if let Some(items) = &b.bullets {
        let lis = items
            .iter()
            .map(|k| {
                // "Name — rest" → bold the name (matches the old section lists).
                let item = match t(k).split_once(" \u{2014} ") {
                    Some((name, rest)) => view! { <strong>{name}</strong>" \u{2014} "{rest} }.into_view(),
                    None => view! { {t(k)} }.into_view(),
                };
                view! { <li class="is-size-6" style="margin-bottom: 6px; line-height: 1.5;">{item}</li> }
            })
            .collect_view();
        return view! { <ul style="margin: 0 0 14px 0; padding-left: 22px; list-style: disc;">{lis}</ul> }.into_view();
    }
    if b.tasks {
        return view! { <StoryTaskList section_id=sec.id.to_string() /> }.into_view();
    }
    if let Some(w) = &b.widget {
        return render_widget(w);
    }
    ().into_view()
}

/// Widget registry: map a DSL `{widget: {id: ...}}` to its component. Grows as
/// sections migrate.
fn render_widget(w: &WidgetRef) -> View {
    match w.id.as_str() {
        "cta" => {
            let route = w.param("route").unwrap_or("/").to_string();
            let label = w.param("label").unwrap_or("common.back").to_string();
            view! { <Cta route=route label=label /> }.into_view()
        }
        "progress_photos" => view! { <ProgressPhotos /> }.into_view(),
        "veg_target" => view! { <VegTarget /> }.into_view(),
        "night_feedback" => view! { <NightFeedback /> }.into_view(),
        "setup_controls" => view! { <SetupControls /> }.into_view(),
        "goal_status" => {
            let p = |k: &str| w.param(k).unwrap_or("").to_string();
            view! {
                <GoalStatus nutrient=p("nutrient") unit=p("unit") title=p("title")
                    set=p("set") need=p("need") route=p("route") label=p("label") />
            }
            .into_view()
        }
        "fab" => view! {
            <div style="display: flex; justify-content: center; margin: 0 0 14px 0;">
                <div style="width: 56px; height: 56px; border-radius: 50%; background: var(--bulma-success); color: #fff; display: flex; align-items: center; justify-content: center; font-size: 34px; line-height: 1; box-shadow: 0 4px 12px rgba(0,0,0,0.2);">"+"</div>
            </div>
        }.into_view(),
        other => {
            leptos::logging::warn!("story: unknown widget '{other}'");
            ().into_view()
        }
    }
}
