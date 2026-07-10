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

use crate::components::cycle_widget::{CycleLine, CyclePanel};
use crate::components::notify_panel::NotifyPanel;
use crate::components::progress_widget::ProgressWidget;
use crate::components::steps_chart_modal::StepsChartModal;
use crate::components::steps_widget::StepsWidget;
use crate::components::weight_chart_modal::WeightChartModal;
use crate::components::weight_widget::WeightWidget;
use crate::services::i18n::t;
use crate::services::profile::{self, CourseGoal, Sex};
use crate::services::{db, local, story};

// Bare 4×3 tile wrapper: the weight/steps widgets bring their own card, so this
// button is transparent and just fills the grid area to open the chart modal.
const WIDGET_TILE: &str = "appearance: none; -webkit-appearance: none; border: none; background: none; \
    padding: 0; margin: 0; cursor: pointer; font: inherit; color: inherit; text-align: left; display: block;";

/// Which widget's editor is open over the grid (None = just the grid).
#[derive(Clone, Copy, PartialEq)]
enum Overlay {
    None,
    Persona,
    Notifications,
    Cycle,
    Errors,
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
    // The cycle widget is female-only.
    let is_female = move || {
        bump.get();
        profile::get_sex() == Some(Sex::Female)
    };

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

    // Weight & steps widgets (moved here from the diary). Resources refresh when
    // their stores change; the tiles open the same chart modals.
    let weight_ver = db::version("weight_entries");
    let weight_res = create_resource(move || weight_ver.get(), |_| async { local::list_weight_entries().await });
    let weight_entries = move || weight_res.get().unwrap_or_default();
    let steps_ver = db::version("step_entries");
    let steps_res = create_resource(move || steps_ver.get(), |_| async { local::list_step_entries().await });
    let steps_entries = move || steps_res.get().unwrap_or_default();
    let show_weight_modal = create_rw_signal(false);
    let show_steps_modal = create_rw_signal(false);

    // Background-error log: the ⚠ tile appears (left of the bell) only when there
    // are errors; tapping it opens the full list.
    let errs = crate::services::errors::signal();
    let has_errors = move || !errs.get().is_empty();

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
            } else if overlay.get() == Overlay::Cycle {
                view! {
                    <div style=EDITOR>
                        <EditorHead title="cycle.title"
                            show_done=Signal::derive(|| true)
                            on_done=move || overlay.set(Overlay::None)/>
                        <CyclePanel/>
                    </div>
                }.into_view()
            } else if overlay.get() == Overlay::Errors {
                view! {
                    <div style=EDITOR>
                        <EditorHead title="errors.title"
                            show_done=Signal::derive(|| true)
                            on_done=move || overlay.set(Overlay::None)/>
                        <ErrorsPanel/>
                    </div>
                }.into_view()
            } else {
                // Collapsed grid: persona 1×1 + notifications bell 1×1.
                view! {
                    <div style="display: flex; flex-direction: column; gap: 12px;">
                        <div style=GRID>
                            <button style=format!("{TILE} grid-column: 1 / 2; grid-row: span 1;")
                                on:click=move |_| overlay.set(Overlay::Persona)>
                                {icon_user()}
                            </button>
                            // Error tile (⚠, orange) — left of the bell (col 7), shown
                            // only when the background queue recorded errors.
                            {move || has_errors().then(|| view! {
                                <button style=format!("{TILE} grid-column: 7 / 8; grid-row: span 1;")
                                    attr:data-testid="dash-errors-widget"
                                    on:click=move |_| overlay.set(Overlay::Errors)>
                                    {icon_alert()}
                                </button>
                            })}

                            // Notifications bell lives in the FAR-RIGHT cell (col 8).
                            // It jiggles only until notifications are configured, and
                            // is drawn crossed-out (bell-off) while the kill-switch is on.
                            <button style=format!("{TILE} grid-column: 8 / 9; grid-row: span 1;")
                                on:click=move |_| overlay.set(Overlay::Notifications)>
                                <span class=move || if notif_configured.get() || notif_disabled.get() { "" } else { "dash-bell-jiggle" }
                                    style="display: inline-flex; transform-origin: 50% 10%;">
                                    {move || if notif_disabled.get() { icon_bell_off().into_view() } else { icon_bell().into_view() }}
                                </span>
                            </button>

                            // Weight & steps widgets: 4×3 tiles side by side under the top row.
                            <button style=format!("{WIDGET_TILE} grid-column: 1 / 5; grid-row: 2 / 5;")
                                attr:data-testid="dash-weight-widget"
                                on:click=move |_| show_weight_modal.set(true)>
                                <WeightWidget entries=Signal::derive(weight_entries)/>
                            </button>
                            <button style=format!("{WIDGET_TILE} grid-column: 5 / 9; grid-row: 2 / 5;")
                                attr:data-testid="dash-steps-widget"
                                on:click=move |_| show_steps_modal.set(true)>
                                <StepsWidget entries=Signal::derive(steps_entries)/>
                            </button>

                        </div>

                        // Progress widget + cycle flow BELOW the grid (not inside it),
                        // so the card grows to fit its content — the indicators row
                        // sits at the very bottom instead of being clipped.
                        <ProgressWidget/>

                        // Cycle widget (female only): full-width single line.
                        {move || is_female().then(|| view! {
                            <button style=WIDGET_TILE
                                on:click=move |_| overlay.set(Overlay::Cycle)>
                                <CycleLine/>
                            </button>
                        })}
                    </div>
                }.into_view()
            }
        }}

        // Chart modals (shared with what the diary used to show), on top of everything.
        {move || show_weight_modal.get().then(|| view! {
            <WeightChartModal entries=Signal::derive(weight_entries)
                on_close=Callback::new(move |_| show_weight_modal.set(false))/>
        })}
        {move || show_steps_modal.get().then(|| view! {
            <StepsChartModal entries=Signal::derive(steps_entries)
                on_close=Callback::new(move |_| show_steps_modal.set(false))/>
        })}
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
    // Initial values captured once. We deliberately DON'T reactively control the
    // <select> value: a reactive `prop:value` fought the native selection and
    // reverted the shown option even though the value was already saved. The editor
    // is recreated every time it opens, so a one-time `selected` is enough.
    let sex0 = profile::get_sex();
    let goal0 = profile::get_goal();
    let pick_sex = move |s: Sex| {
        profile::set_sex(s);
        bump.update(|v| *v += 1);
        // Keep the story setup-section tasks working (Settings used to set these).
        spawn_local(async { story::set_flag(story::SEX_SELECTED, true).await; });
    };
    let pick_goal = move |g: CourseGoal| {
        profile::set_goal(g);
        bump.update(|v| *v += 1);
    };

    // Right-aligned number field on its row.
    let field = "background: var(--bulma-scheme-main); border: none; border-radius: 10px; \
                 padding: 10px 12px; width: 110px; text-align: right; color: var(--bulma-text); font: inherit;";
    // Compact native select for the goal.
    let select = "background: var(--bulma-scheme-main); border: none; border-radius: 10px; \
                  padding: 9px 10px; color: var(--bulma-text); font: inherit;";
    // Each field is one row: label on the left, control on the right.
    let row = "display: flex; align-items: center; justify-content: space-between; gap: 12px; min-height: 44px;";
    let label = "margin: 0;";

    view! {
        <div style="display: flex; flex-direction: column; gap: 8px;">
            <div style=row>
                <span class="is-size-6" style=label>{move || t("dashboard.sex")}</span>
                <select style=select
                    on:change=move |ev| {
                        match event_target_value(&ev).as_str() {
                            "male" => pick_sex(Sex::Male),
                            "female" => pick_sex(Sex::Female),
                            _ => {}
                        }
                    }>
                    // Empty placeholder until a sex is chosen (keeps the profile incomplete).
                    <option value="" selected=sex0.is_none() disabled hidden></option>
                    <option value="male" selected=sex0 == Some(Sex::Male)>{move || t("dashboard.sex_male")}</option>
                    <option value="female" selected=sex0 == Some(Sex::Female)>{move || t("dashboard.sex_female")}</option>
                </select>
            </div>

            <div style=row>
                <span class="is-size-6" style=label>{move || t("dashboard.height")}</span>
                <input type="number" inputmode="numeric" min="80" max="250" style=field
                    prop:value=move || { bump.get(); profile::get_height_cm().map(|h| (h as i64).to_string()).unwrap_or_default() }
                    on:change=move |ev| {
                        if let Ok(v) = event_target_value(&ev).trim().parse::<f64>() {
                            if v > 0.0 {
                                profile::set_height_cm(v);
                                bump.update(|x| *x += 1);
                                spawn_local(async { story::set_flag(story::HEIGHT_SET, true).await; });
                            }
                        }
                    }/>
            </div>

            <div style=row>
                <span class="is-size-6" style=label>{move || t("dashboard.birth_year")}</span>
                <input type="number" inputmode="numeric" min="1900" max="2025" style=field
                    prop:value=move || { bump.get(); profile::get_birth_year().map(|y| y.to_string()).unwrap_or_default() }
                    on:change=move |ev| {
                        if let Ok(v) = event_target_value(&ev).trim().parse::<i32>() {
                            if (1900..=2026).contains(&v) {
                                profile::set_birth_year(v);
                                bump.update(|x| *x += 1);
                                spawn_local(async { story::set_flag(story::BIRTH_YEAR_SET, true).await; });
                            }
                        }
                    }/>
            </div>

            <div style=row>
                <span class="is-size-6" style=label>{move || t("dashboard.goal")}</span>
                <select style=select
                    on:change=move |ev| {
                        let g = match event_target_value(&ev).as_str() {
                            "gain" => CourseGoal::Gain,
                            "maintain" => CourseGoal::Maintain,
                            _ => CourseGoal::Lose,
                        };
                        pick_goal(g);
                    }>
                    <option value="lose" selected=goal0 == CourseGoal::Lose>{move || t("dashboard.goal_lose")}</option>
                    <option value="gain" selected=goal0 == CourseGoal::Gain>{move || t("dashboard.goal_gain")}</option>
                    <option value="maintain" selected=goal0 == CourseGoal::Maintain>{move || t("dashboard.goal_maintain")}</option>
                </select>
            </div>
        </div>
    }
}

/// Full-panel list of background errors. Each row is tappable to copy its text to
/// the clipboard; a «clear» button empties the log.
#[component]
fn ErrorsPanel() -> impl IntoView {
    let errs = crate::services::errors::signal();
    let copied = create_rw_signal(None::<usize>);
    view! {
        <p class="is-size-7 has-text-grey" style="margin: 0 0 10px;">{move || t("errors.hint")}</p>
        <div style="display: flex; flex-direction: column; gap: 8px;">
            {move || {
                let list = errs.get();
                if list.is_empty() {
                    return view! { <p class="is-size-6 has-text-grey">{move || t("errors.none")}</p> }.into_view();
                }
                list.into_iter().enumerate().map(|(i, e)| {
                    let text = e.as_text();
                    // A real <button> — a <div on:click> is dead on iOS (Leptos
                    // delegates clicks and iOS only bubbles them from interactive
                    // elements). Tint the row green briefly to confirm the copy.
                    view! {
                        <button
                            style=move || format!(
                                "display: block; width: 100%; text-align: left; height: auto; \
                                 white-space: normal; border: none; border-radius: 12px; padding: 12px 14px; \
                                 cursor: pointer; font: inherit; color: inherit; transition: background 0.15s; \
                                 background: {};",
                                if copied.get() == Some(i) { "var(--bulma-success-soft)" } else { "var(--bulma-scheme-main)" }
                            )
                            on:click=move |_| {
                                // Call clipboard.writeText SYNCHRONOUSLY inside the gesture
                                // — iOS Safari drops it if deferred (spawn_local).
                                copy_to_clipboard(&text);
                                copied.set(Some(i));
                            }>
                            <p class="is-size-6 has-text-weight-semibold">{e.context.clone()}</p>
                            <p class="is-size-7 has-text-grey" style="white-space: pre-wrap; word-break: break-word; margin-top: 2px;">
                                {e.message.clone()}
                            </p>
                            {move || (copied.get() == Some(i)).then(|| view! {
                                <p class="is-size-7 has-text-weight-bold has-text-success" style="margin-top: 4px;">
                                    {move || t("errors.copied")}
                                </p>
                            })}
                        </button>
                    }
                }).collect_view()
            }}
        </div>
        <button class="button is-light is-fullwidth is-small" style="margin-top: 14px;"
            on:click=move |_| crate::services::errors::clear()>
            {move || t("errors.clear")}
        </button>
    }
}

/// Copy text to the clipboard SYNCHRONOUSLY from the click handler. Uses two
/// mechanisms for reliability on iOS PWAs: the async Clipboard API (fire-and-forget,
/// invoked inside the gesture) AND a legacy hidden-textarea + `execCommand('copy')`
/// fallback, which works in WKWebView/older Safari where the async API is flaky.
fn copy_to_clipboard(text: &str) {
    use wasm_bindgen::JsCast;
    let Some(window) = web_sys::window() else { return };

    // Modern async Clipboard API — the promise settles after the handler returns,
    // but the API is invoked within the gesture, which is what iOS checks.
    let _ = window.navigator().clipboard().write_text(text);

    // Legacy fallback: select a hidden textarea and execCommand('copy').
    let Some(document) = window.document() else { return };
    if let Ok(el) = document.create_element("textarea") {
        let ta: web_sys::HtmlTextAreaElement = el.unchecked_into();
        ta.set_value(text);
        let _ = ta.set_attribute("readonly", "");
        let _ = ta.style().set_property("position", "fixed");
        let _ = ta.style().set_property("top", "0");
        let _ = ta.style().set_property("opacity", "0");
        if let Some(body) = document.body() {
            let _ = body.append_child(&ta);
            ta.select();
            let _ = ta.set_selection_range(0, text.len() as u32);
            if let Ok(html_doc) = document.dyn_into::<web_sys::HtmlDocument>() {
                let _ = html_doc.exec_command("copy");
            }
            let _ = body.remove_child(&ta);
        }
    }
}

// ── Feather/Lucide line icons (24×24, currentColor, 2px round strokes) — the same
// style as the bottom-nav icons, so the widgets stop looking like OS emoji. ──

const IC: &str = "http://www.w3.org/2000/svg";

/// Feather `user`.
fn icon_user() -> impl IntoView {
    view! {
        <svg xmlns=IC width="30" height="30" viewBox="0 0 24 24" fill="none"
            stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">
            <path d="M20 21v-2a4 4 0 0 0-4-4H8a4 4 0 0 0-4 4v2"/>
            <circle cx="12" cy="7" r="4"/>
        </svg>
    }
}

/// Feather `alert-triangle`, drawn orange — the background-errors tile.
fn icon_alert() -> impl IntoView {
    view! {
        <svg xmlns=IC width="28" height="28" viewBox="0 0 24 24" fill="none"
            stroke="#e8850d" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">
            <path d="M10.29 3.86 1.82 18a2 2 0 0 0 1.71 3h16.94a2 2 0 0 0 1.71-3L13.71 3.86a2 2 0 0 0-3.42 0z"/>
            <line x1="12" y1="9" x2="12" y2="13"/>
            <line x1="12" y1="17" x2="12.01" y2="17"/>
        </svg>
    }
}

/// Feather `bell`.
fn icon_bell() -> impl IntoView {
    view! {
        <svg xmlns=IC width="28" height="28" viewBox="0 0 24 24" fill="none"
            stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">
            <path d="M18 8A6 6 0 0 0 6 8c0 7-3 9-3 9h18s-3-2-3-9"/>
            <path d="M13.73 21a2 2 0 0 1-3.46 0"/>
        </svg>
    }
}

/// Feather `bell-off` (the crossed-out bell for the disabled state).
fn icon_bell_off() -> impl IntoView {
    view! {
        <svg xmlns=IC width="28" height="28" viewBox="0 0 24 24" fill="none"
            stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">
            <path d="M13.73 21a2 2 0 0 1-3.46 0"/>
            <path d="M18.63 13A17.89 17.89 0 0 1 18 8"/>
            <path d="M6.26 6.26A5.86 5.86 0 0 0 6 8c0 7-3 9-3 9h14"/>
            <path d="M18 8a6 6 0 0 0-9.33-5"/>
            <line x1="1" y1="1" x2="23" y2="23"/>
        </svg>
    }
}
