//! The Dashboard — the app's new default screen (replaces the story «История» on
//! the first nav tab; the story lives on at `/history`).
//!
//! Layout: an 8-COLUMN square-cell grid. A unit is 1×1 cell; widgets occupy a
//! rectangle of units (the weight/steps widgets will be 4×3). Widgets are revealed
//! progressively — like the story, but simpler: the Persona widget comes first, and
//! once the profile is filled it collapses to 1×1 and the Notifications widget's
//! bell starts jiggling. Tapping a collapsed widget opens its editor over the grid.
//!
//! This first increment ships the framework + the Persona and Notifications widgets.
//! Weight/steps tiles (4×3) will slot into the same grid next.

use leptos::*;

use crate::components::notify_panel::NotifyPanel;
use crate::services::i18n::t;
use crate::services::profile::{self, CourseGoal, Sex};

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
    display: flex; flex-direction: column; padding: 10px;";

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

    view! {
        <div style="display: flex; flex-direction: column; gap: 12px;">
            <h1 class="is-size-4 has-text-weight-bold" style="margin: 4px 4px 0;">
                {move || t("nav.dashboard")}
            </h1>

            <div style=GRID>
                // ── Persona widget ──
                {move || {
                    if persona_complete() {
                        // Collapsed 1×1 summary; tap to reconfigure.
                        view! {
                            <button style=format!("{TILE} grid-column: span 1; grid-row: span 1; align-items: center; justify-content: center;")
                                on:click=move |_| overlay.set(Overlay::Persona)>
                                <span style="font-size: 1.5rem;">"👤"</span>
                            </button>
                        }.into_view()
                    } else {
                        // Setup prompt: full width, two rows tall.
                        view! {
                            <button style=format!("{TILE} grid-column: span 8; grid-row: span 2; justify-content: center; gap: 4px;")
                                on:click=move |_| overlay.set(Overlay::Persona)>
                                <span style="font-size: 1.6rem;">"👤"</span>
                                <span class="has-text-weight-semibold">{move || t("dashboard.persona_setup_title")}</span>
                                <span class="is-size-7 has-text-grey">{move || t("dashboard.persona_setup_hint")}</span>
                            </button>
                        }.into_view()
                    }
                }}

                // ── Notifications widget (1×1) ──
                <button style=format!("{TILE} grid-column: span 1; grid-row: span 1; align-items: center; justify-content: center;")
                    on:click=move |_| overlay.set(Overlay::Notifications)>
                    <span class=move || if persona_complete() { "dash-bell-jiggle" } else { "" }
                        style="font-size: 1.5rem; display: inline-block; transform-origin: 50% 10%;">
                        "🔔"
                    </span>
                </button>
            </div>
        </div>

        // ── Editor overlay (over all widgets) ──
        {move || match overlay.get() {
            Overlay::None => ().into_view(),
            Overlay::Persona => view! {
                <WidgetOverlay title="dashboard.persona_title" on_close=move || overlay.set(Overlay::None)>
                    <PersonaEditor bump/>
                </WidgetOverlay>
            }.into_view(),
            Overlay::Notifications => view! {
                <WidgetOverlay title="dashboard.notifications_title" on_close=move || overlay.set(Overlay::None)>
                    <NotifyPanel hide_check_after_received=true/>
                </WidgetOverlay>
            }.into_view(),
        }}
    }
}

/// Full-screen overlay host: a scrim + a rounded sheet with a title, a Done button,
/// and the widget's editor as children.
#[component]
fn WidgetOverlay(
    title: &'static str,
    on_close: impl Fn() + 'static + Copy,
    children: Children,
) -> impl IntoView {
    view! {
        <div style="position: fixed; inset: 0; z-index: 60; display: flex; flex-direction: column; justify-content: flex-end;">
            // Scrim is a SIBLING behind the sheet (not a parent), so taps on the
            // sheet can't bubble to it — Leptos delegated on:click doesn't reliably
            // honour an inner stop_propagation, so we avoid relying on it.
            <div style="position: absolute; inset: 0; background: rgba(0,0,0,0.35);"
                on:click=move |_| on_close()></div>
            <div style="position: relative; z-index: 1; background: var(--bulma-background); \
                        border-radius: 20px 20px 0 0; max-height: 88vh; overflow-y: auto; \
                        -webkit-overflow-scrolling: touch; \
                        padding: 12px 16px calc(16px + env(safe-area-inset-bottom));"
                data-ios-scroll="1">
                <div style="display: flex; align-items: center; justify-content: space-between; margin-bottom: 8px;">
                    <h2 class="is-size-5 has-text-weight-bold" style="margin: 0;">{move || t(title)}</h2>
                    <button class="button is-small is-light" on:click=move |_| on_close()>
                        {move || t("dashboard.close")}
                    </button>
                </div>
                {children()}
            </div>
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
        <div style="display: flex; flex-direction: column; gap: 14px; padding-bottom: 8px;">
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
