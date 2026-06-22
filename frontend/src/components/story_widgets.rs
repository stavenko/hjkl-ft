//! Named, self-contained widgets the Story DSL embeds in section content blocks
//! (`{widget: {id: ...}}`). Each is a real `#[component]` with a stable reactive
//! scope and manages its own state, so the generic section page can rebuild its
//! body on section change without remounting / resetting a widget mid-stream.

use leptos::*;
use leptos_router::*;

use crate::services::story_dsl::{self, Engine, EngineSnapshot, Loc};
use crate::services::{db, i18n, i18n::t, local, profile, story, summary};

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
                    <div style="display: flex; justify-content: center;">
                        <img src="/progress-poses.jpg" alt="" style="display: block; width: 100%; max-width: 320px; height: auto;" />
                    </div>
                    <div style="display: flex; justify-content: space-around; max-width: 300px; margin: 4px auto 0 auto;">
                        <span class="is-size-7 has-text-grey">{move || t("progress.pose_front")}</span>
                        <span class="is-size-7 has-text-grey">{move || t("progress.pose_back")}</span>
                        <span class="is-size-7 has-text-grey">{move || t("progress.pose_side")}</span>
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

/// Hidden-goal status card (Protein / Calorie planka). The goal value is set by
/// the section's `on_open` action; this shows it once present, or a "need …"
/// prompt + CTA when the precondition (a weight / some diary days) isn't met yet.
#[component]
pub fn GoalStatus(
    nutrient: String,
    unit: String,
    title: String,
    set: String,
    need: String,
    route: String,
    label: String,
) -> impl IntoView {
    let goals_ver = db::version("goals");
    let amount = create_rw_signal(None::<f64>);
    {
        let nutrient = nutrient.clone();
        create_effect(move |_| {
            goals_ver.get();
            let nutrient = nutrient.clone();
            spawn_local(async move {
                let v = local::list_goals().await.into_iter()
                    .find(|g| g.nutrient == nutrient && g.amount > 0.0)
                    .map(|g| g.amount);
                amount.set(v);
            });
        });
    }
    // Stored (Copy) so each reactive closure can read its own clone.
    let unit = store_value(unit);
    let title = store_value(title);
    let set = store_value(set);
    let need = store_value(need);
    let route = store_value(route);
    let label = store_value(label);
    view! {
        <div style="margin: 16px 0 0 0;">
            <p class="is-size-7 has-text-grey-light" style="text-transform: uppercase; letter-spacing: 0.02em; margin: 0 0 8px 4px;">
                {move || t(&title.get_value())}
            </p>
            <div style=CARD>
                <div style="display: flex; align-items: center; justify-content: center; padding: 18px 16px; text-align: center;">
                    {move || match amount.get() {
                        Some(v) => view! {
                            <span class="is-size-3 has-text-weight-bold">{format!("{} {}", v.round() as i64, t(&unit.get_value()))}</span>
                        }.into_view(),
                        None => view! { <span class="is-size-6 has-text-grey">{t(&need.get_value())}</span> }.into_view(),
                    }}
                </div>
            </div>
            {move || match amount.get() {
                Some(_) => view! {
                    <p class="is-size-6 has-text-weight-semibold has-text-success" style="margin-top: 16px;">{t(&set.get_value())}</p>
                }.into_view(),
                None => view! {
                    <div style="padding: 16px 0 0 0;">
                        <A href=route.get_value() class="button is-link is-fullwidth is-medium">{t(&label.get_value())}</A>
                    </div>
                }.into_view(),
            }}
        </div>
    }
}

/// Chapter-2 vegetables/fruit: yesterday's logged grams vs the sex-specific
/// target (600 g women / 800 g men).
#[component]
pub fn VegTarget() -> impl IntoView {
    let target = match profile::get_sex() {
        Some(profile::Sex::Female) => 600.0_f64,
        _ => 800.0,
    };
    let summaries_ver = db::version("summaries");
    let grams = create_rw_signal(None::<f64>);
    create_effect(move |_| {
        summaries_ver.get();
        spawn_local(async move {
            let y = local::yesterday();
            let v = match summary::get_day(&y).await {
                Some(s) => summary::parse_day(&s.text).and_then(|d| d.facts.map(|f| f.veg_fruit_grams)),
                None => None,
            };
            grams.set(v);
        });
    });
    view! {
        <div style="margin: 16px 0 0 0;">
            <p class="is-size-7 has-text-grey-light" style="text-transform: uppercase; letter-spacing: 0.02em; margin: 0 0 8px 4px;">
                {move || t("story.ch2.veg.target_label")}
            </p>
            <div style=CARD>
                <div style="display: flex; align-items: center; justify-content: center; padding: 18px 16px;">
                    {move || match grams.get() {
                        Some(g) => view! {
                            <span class="is-size-3 has-text-weight-bold">
                                {format!("{} / {} {}", g.round() as i64, target.round() as i64, t("common.unit.g"))}
                            </span>
                        }.into_view(),
                        None => view! { <span class="is-size-6 has-text-grey">{move || t("story.ch2.veg.no_data")}</span> }.into_view(),
                    }}
                </div>
            </div>
        </div>
    }
}

/// Chapter-2 night feedback: today's evening protein (dinner + night) ≥ 30 g.
#[component]
pub fn NightFeedback() -> impl IntoView {
    let diary_ver = db::version("diary");
    let protein = create_rw_signal(0.0_f64);
    create_effect(move |_| {
        diary_ver.get();
        spawn_local(async move {
            let today = chrono::Local::now().format("%Y-%m-%d").to_string();
            protein.set(local::evening_protein_on(&today).await);
        });
    });
    view! {
        <div style="margin: 16px 0 0 0;">
            <p class="is-size-7 has-text-grey-light" style="text-transform: uppercase; letter-spacing: 0.02em; margin: 0 0 8px 4px;">
                {move || t("story.ch2.night.feedback_label")}
            </p>
            <div style=CARD>
                <div style="display: flex; align-items: flex-start; gap: 12px; padding: 14px 16px;">
                    {move || if protein.get() >= 30.0 {
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
    }
}

/// Chapter-1 setup controls: the language checkbox (toggles the task), the test
/// notification status, and the sex-selection status. Opening with `?notif=1`
/// (the test push deep-link) marks the notification task done.
#[component]
pub fn SetupControls() -> impl IntoView {
    let story_ver = db::version("story");
    let lang_done = create_rw_signal(false);
    let notif_done = create_rw_signal(false);
    let sex_done = create_rw_signal(false);
    create_effect(move |_| {
        story_ver.get();
        spawn_local(async move {
            lang_done.set(story::get_flag(story::LANGUAGE_CONFIGURED).await);
            notif_done.set(story::get_flag(story::NOTIFICATION_RECEIVED).await);
            sex_done.set(story::get_flag(story::SEX_SELECTED).await);
        });
    });

    // Test-push deep link: ?notif=1 marks the notification task done.
    let search = web_sys::window()
        .map(|w| w.location())
        .and_then(|l| l.search().ok())
        .unwrap_or_default();
    if search.contains("notif=1") {
        spawn_local(async move { story::set_flag(story::NOTIFICATION_RECEIVED, true).await; });
    }

    let toggle_lang = move |_| {
        let v = !lang_done.get_untracked();
        lang_done.set(v);
        spawn_local(async move { story::set_flag(story::LANGUAGE_CONFIGURED, v).await; });
    };

    view! {
        <div style="margin: 16px 0 0 0;">
            <p class="is-size-7 has-text-grey-light" style="text-transform: uppercase; letter-spacing: 0.02em; margin: 0 0 8px 4px;">
                {move || t("story.setup.task_label")}
            </p>
            <div style=CARD>
                <label style="display: flex; align-items: center; gap: 12px; padding: 14px 16px; cursor: pointer;">
                    <input type="checkbox"
                        attr:data-testid="story-setup-language-configured"
                        style="width: 22px; height: 22px; accent-color: var(--bulma-link);"
                        prop:checked=move || lang_done.get()
                        on:change=toggle_lang
                    />
                    <span class="is-size-6 has-text-weight-semibold">{move || t("story.setup.checkbox_lang")}</span>
                </label>
                <div style="border-bottom: 0.5px solid var(--bulma-border-weak);"></div>
                <div style="display: flex; align-items: center; gap: 12px; padding: 14px 16px;">
                    <span style="font-size: 22px; width: 22px; text-align: center;">{move || if notif_done.get() { "\u{2705}" } else { "\u{23f3}" }}</span>
                    <span class="is-size-6 has-text-weight-semibold" style="flex: 1;">
                        {move || if notif_done.get() { t("story.setup.notif_status_done") } else { t("story.setup.notif_status_pending") }}
                    </span>
                </div>
                <div style="border-bottom: 0.5px solid var(--bulma-border-weak);"></div>
                <div style="display: flex; align-items: center; gap: 12px; padding: 14px 16px;">
                    <span style="font-size: 22px; width: 22px; text-align: center;">{move || if sex_done.get() { "\u{2705}" } else { "\u{23f3}" }}</span>
                    <span class="is-size-6 has-text-weight-semibold" style="flex: 1;">
                        {move || if sex_done.get() { t("story.setup.sex_status_done") } else { t("story.setup.sex_status_pending") }}
                    </span>
                </div>
            </div>
            {move || (lang_done.get() && notif_done.get()).then(|| view! {
                <p class="is-size-6 has-text-weight-semibold has-text-success" style="margin-top: 16px;">
                    {move || t("story.setup.next_unlocked")}
                </p>
            })}
        </div>
    }
}
