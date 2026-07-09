//! The Dashboard — the app's new default screen (replaces the story «История» on
//! the first nav tab; the story lives on at `/history`).
//!
//! Layout: an 8-COLUMN square-cell grid. A unit is 1×1 cell; widgets occupy a
//! rectangle of units (the weight/steps widgets will be 4×3). Widgets are revealed
//! progressively — like the story, but simpler.
//!
//! The Persona widget comes FIRST and is OPEN by default: while the profile is
//! incomplete its editor fills the whole dashboard (above the nav, in-flow — never a
//! bottom sheet that fights the menu), and the other widgets (the notifications bell)
//! are hidden behind it. Once every field is filled it collapses to a 1×1 tile and
//! the bell appears and jiggles; tapping the tile re-opens the full-screen editor.
//!
//! This first increment ships the framework + the Persona and Notifications widgets.
//! Weight/steps tiles (4×3) will slot into the same grid next.

use leptos::*;

use crate::components::notify_panel::NotifyPanel;
use crate::services::i18n::t;
use crate::services::profile::{self, CourseGoal, Sex};
use crate::services::{db, story};

/// Which widget's editor is open over the grid (None = just the grid).
#[derive(Clone, Copy, PartialEq)]
enum Overlay {
    None,
    Persona,
    Notifications,
}

// 8 columns; each cell is a square whose side `--u` is derived from the viewport
// width minus the app-shell's 0.75rem side padding and the inter-cell gaps.
const GRID: &str = "--gap: 6px; --u: calc((100vw - 1.5rem - 7 * var(--gap)) / 8); \
    display: grid; grid-template-columns: repeat(8, 1fr); grid-auto-rows: var(--u); gap: var(--gap);";

const TILE: &str = "appearance: none; -webkit-appearance: none; border: none; font: inherit; \
    color: inherit; text-align: left; cursor: pointer; background: var(--bulma-scheme-main); \
    border-radius: 16px; box-shadow: 0 2px 10px rgba(0,0,0,0.06); overflow: hidden; \
    display: flex; flex-direction: column; align-items: center; justify-content: center; padding: 10px;";

// An open editor fills the dashboard area; min-height keeps it "full-screen" while
// still sitting inside the scroll container (so the bottom nav stays clear).
const EDITOR: &str = "display: flex; flex-direction: column; gap: 14px; min-height: calc(100dvh - 5.5rem);";

#[component]
pub fn DashboardPage() -> impl IntoView {
    // Profile reads are synchronous (cached); bump this to re-read after an edit.
    let bump = create_rw_signal(0u32);
    let persona_complete = move || {
        bump.get();
        profile::get_height_cm().is_some()
            && profile::get_birth_year().is_some()
            && profile::get_sex().is_some()
    };

    let overlay = create_rw_signal(Overlay::None);
    // Persona takes over the whole screen while it's incomplete OR re-opened.
    let persona_full = move || !persona_complete() || overlay.get() == Overlay::Persona;

    // Notifications state for the bell: `configured` (a test notification was
    // received → stop jiggling) and `disabled` (the master kill-switch → cross the
    // bell out). Both re-read when the story flags or the schedule record change.
    let meta_ver = db::version("_sync_meta");
    let story_ver = db::version("story");
    let notif_configured = create_rw_signal(false);
    let notif_disabled = create_rw_signal(false);
    create_effect(move |_| {
        meta_ver.get();
        story_ver.get();
        spawn_local(async move {
            notif_configured.set(story::get_flag(story::NOTIFICATION_RECEIVED).await);
            let d = db::get::<serde_json::Value>("_sync_meta", "notification_schedule")
                .await
                .and_then(|v| v.get("disabled").and_then(|x| x.as_bool()))
                .unwrap_or(false);
            notif_disabled.set(d);
        });
    });

    view! {
        {move || {
            if persona_full() {
                view! {
                    <div style=EDITOR>
                        <EditorHead title="dashboard.persona_title"
                            show_done=Signal::derive(persona_complete)
                            on_done=move || overlay.set(Overlay::None)/>
                        <PersonaEditor bump/>
                    </div>
                }.into_view()
            } else if overlay.get() == Overlay::Notifications {
                view! {
                    <div style=EDITOR>
                        <EditorHead title="dashboard.notifications_title"
                            show_done=Signal::derive(|| true)
                            on_done=move || overlay.set(Overlay::None)/>
                        <NotifyPanel hide_check_after_received=true/>
                    </div>
                }.into_view()
            } else {
                // Collapsed grid: persona 1×1 + notifications bell 1×1.
                view! {
                    <div style="display: flex; flex-direction: column; gap: 12px;">
                        <h1 class="is-size-4 has-text-weight-bold" style="margin: 4px 4px 0;">
                            {move || t("nav.dashboard")}
                        </h1>
                        <div style=GRID>
                            <button style=format!("{TILE} grid-column: 1 / 2; grid-row: span 1;")
                                on:click=move |_| overlay.set(Overlay::Persona)>
                                <span style="font-size: 1.5rem;">"👤"</span>
                            </button>
                            // Notifications bell lives in the FAR-RIGHT cell (col 8).
                            // It jiggles only until notifications are configured, and
                            // is drawn crossed-out (🔕) while the kill-switch is on.
                            <button style=format!("{TILE} grid-column: 8 / 9; grid-row: span 1;")
                                on:click=move |_| overlay.set(Overlay::Notifications)>
                                <span class=move || if notif_configured.get() || notif_disabled.get() { "" } else { "dash-bell-jiggle" }
                                    style="font-size: 1.5rem; display: inline-block; transform-origin: 50% 10%;">
                                    {move || if notif_disabled.get() { "🔕" } else { "🔔" }}
                                </span>
                            </button>
                        </div>
                    </div>
                }.into_view()
            }
        }}
    }
}

/// Editor header: the widget title + a "Done" button (shown only when `show_done`).
#[component]
fn EditorHead(
    title: &'static str,
    #[prop(into)] show_done: MaybeSignal<bool>,
    on_done: impl Fn() + 'static + Copy,
) -> impl IntoView {
    view! {
        <div style="display: flex; align-items: center; justify-content: space-between; margin: 4px 4px 0;">
            <h1 class="is-size-4 has-text-weight-bold" style="margin: 0;">{move || t(title)}</h1>
            {move || show_done.get().then(|| view! {
                <button class="button is-small is-light" on:click=move |_| on_done()>
                    {move || t("dashboard.close")}
                </button>
            })}
        </div>
    }
}

/// Persona editor: sex, height, birth year and course goal. Every control writes
/// straight to the profile and bumps the dashboard so completeness re-evaluates.
#[component]
fn PersonaEditor(bump: RwSignal<u32>) -> impl IntoView {
    let sex = move || {
        bump.get();
        profile::get_sex()
    };
    let goal = move || {
        bump.get();
        profile::get_goal()
    };
    let pick_sex = move |s: Sex| {
        profile::set_sex(s);
        bump.update(|v| *v += 1);
    };
    let pick_goal = move |g: CourseGoal| {
        profile::set_goal(g);
        bump.update(|v| *v += 1);
    };

    let seg = |active: bool| -> String {
        format!(
            "flex: 1; padding: 10px; border-radius: 10px; border: none; cursor: pointer; font: inherit; \
             background: {}; color: {};",
            if active { "var(--bulma-link)" } else { "var(--bulma-scheme-main)" },
            if active { "#fff" } else { "var(--bulma-text)" },
        )
    };
    let field = "background: var(--bulma-scheme-main); border: none; border-radius: 10px; \
                 padding: 10px 12px; width: 100%; color: var(--bulma-text); font: inherit;";

    view! {
        <div style="display: flex; flex-direction: column; gap: 14px;">
            <div>
                <p class="is-size-7 has-text-grey" style="margin: 0 0 6px;">{move || t("dashboard.sex")}</p>
                <div style="display: flex; gap: 8px;">
                    <button style=move || seg(sex() == Some(Sex::Male)) on:click=move |_| pick_sex(Sex::Male)>
                        {move || t("dashboard.sex_male")}
                    </button>
                    <button style=move || seg(sex() == Some(Sex::Female)) on:click=move |_| pick_sex(Sex::Female)>
                        {move || t("dashboard.sex_female")}
                    </button>
                </div>
            </div>

            <div>
                <p class="is-size-7 has-text-grey" style="margin: 0 0 6px;">{move || t("dashboard.height")}</p>
                <input type="number" inputmode="numeric" min="80" max="250" style=field
                    prop:value=move || { bump.get(); profile::get_height_cm().map(|h| (h as i64).to_string()).unwrap_or_default() }
                    on:change=move |ev| {
                        if let Ok(v) = event_target_value(&ev).trim().parse::<f64>() {
                            profile::set_height_cm(v);
                            bump.update(|x| *x += 1);
                        }
                    }/>
            </div>

            <div>
                <p class="is-size-7 has-text-grey" style="margin: 0 0 6px;">{move || t("dashboard.birth_year")}</p>
                <input type="number" inputmode="numeric" min="1900" max="2025" style=field
                    prop:value=move || { bump.get(); profile::get_birth_year().map(|y| y.to_string()).unwrap_or_default() }
                    on:change=move |ev| {
                        if let Ok(v) = event_target_value(&ev).trim().parse::<i32>() {
                            profile::set_birth_year(v);
                            bump.update(|x| *x += 1);
                        }
                    }/>
            </div>

            <div>
                <p class="is-size-7 has-text-grey" style="margin: 0 0 6px;">{move || t("dashboard.goal")}</p>
                <div style="display: flex; gap: 8px;">
                    <button style=move || seg(goal() == CourseGoal::Lose) on:click=move |_| pick_goal(CourseGoal::Lose)>
                        {move || t("dashboard.goal_lose")}
                    </button>
                    <button style=move || seg(goal() == CourseGoal::Gain) on:click=move |_| pick_goal(CourseGoal::Gain)>
                        {move || t("dashboard.goal_gain")}
                    </button>
                    <button style=move || seg(goal() == CourseGoal::Maintain) on:click=move |_| pick_goal(CourseGoal::Maintain)>
                        {move || t("dashboard.goal_maintain")}
                    </button>
                </div>
            </div>
        </div>
    }
}
