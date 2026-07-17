//! The Dashboard — the app's default screen and first nav tab.
//!
//! Layout: an 8-COLUMN square-cell grid. A unit is 1×1 cell; widgets occupy a
//! rectangle of units (the weight/steps widgets will be 4×3). Widgets are revealed
//! progressively.
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
use crate::components::day_bars::DayBars;
use crate::components::gauge::Gauge;
use crate::components::info_hint::InfoHint;
use crate::components::progress_widget::{self, ProgressWidget};
use crate::components::steps_panel::StepsPanel;
use crate::components::steps_widget::StepsWidget;
use crate::components::weight_panel::WeightPanel;
use crate::components::weight_widget::WeightWidget;
use std::cell::RefCell;

use api_types::{StepEntry, WeightEntry};

use crate::services::errors::AppError;
use crate::services::i18n::t;
use crate::services::profile::{self, CourseGoal, Sex};
use crate::services::indicators::{self, IndicatorState};
use crate::services::sticky::sticky;
use crate::services::weight_trend::{self, BalanceState};
use crate::services::{db, local, net};

// ── Expanded-view helpers (gauge labels / colours + "?" explanations) ─────────

/// Everything the expanded view needs once the planka is set.
#[derive(Clone)]
struct DetailData {
    planka: Option<f64>,
    eaten: f64,
    gauges: Vec<indicators::DailyGauge>,
    series: Vec<indicators::IndicatorSeries>,
    calorie_hint: String,
    protein_hint: String,
    veg_hint: String,
}

fn gauge_label(key: &str) -> &'static str {
    match key {
        "protein" => "Белок",
        "veg_fruit" => "Фр/овощи",
        "calcium" => "Кальций",
        "iron" => "Железо",
        _ => "Клетчатка",
    }
}

/// The calorie "?" text — depends on the course goal AND the current weight trend,
/// and shows the actual average intake + resulting planka.
async fn calorie_hint_text(planka: Option<f64>) -> String {
    let avg = local::avg_daily_kcal(7).await;
    let entries = local::list_weight_entries().await;
    let balance = weight_trend::weight_trend(&entries, weight_trend::DEFAULT_WINDOW_DAYS).balance();
    let goal = match profile::get_goal() {
        CourseGoal::Lose => "похудение",
        CourseGoal::Maintain => "удержание веса",
        CourseGoal::Gain => "набор веса",
    };
    // The planka keeps the average when you're ALREADY in a deficit; otherwise it's
    // 5% below (mirrors `local::calorie_planka`).
    let (trend, minus) = match balance {
        BalanceState::Deficit => ("вы уже в дефиците — вес снижается", ""),
        BalanceState::Surplus => ("мы заметили, что вы в профиците — вес растёт", " − 5%"),
        BalanceState::Maintenance => {
            ("мы заметили, что вы в балансе калорий — вес стоит на месте", " − 5%")
        }
    };
    let avg_s = avg.map(|a| format!("{a:.0}")).unwrap_or_else(|| "—".to_string());
    let planka_s = planka.map(|p| format!("{p:.0}")).unwrap_or_else(|| "—".to_string());
    format!(
        "Поскольку ваша цель — {goal}, а {trend}, мы устанавливаем планку в ваше среднее \
         потребление за 7 дней{minus}.\n\nВаше среднее потребление калорий: {avg_s} ккал.\n\
         И поэтому ваша планка: {planka_s} ккал."
    )
}

/// The protein "?" text — the fat-free-mass derivation, with the user's numbers.
async fn protein_hint_text() -> String {
    let Some(w) = local::list_weight_entries().await.last().map(|e| e.weight_kg) else {
        return "Заполните вес и профиль, чтобы рассчитать норму белка.".to_string();
    };
    let target = profile::protein_target_from_profile(w);
    if target == 0 {
        return "Заполните профиль (рост, вес, возраст, пол), чтобы рассчитать норму белка."
            .to_string();
    }
    let ffm = target as f64 / 1.6;
    let h = profile::get_height_cm().map(|h| format!("{h:.0} см")).unwrap_or_default();
    let age = profile::get_age_years().map(|a| format!("{a}")).unwrap_or_default();
    format!(
        "Норму белка считаем от безжировой массы тела. По вашим данным (рост {h}, возраст \
         {age}, вес {w:.0} кг) безжировая масса ≈ {ffm:.0} кг, а планка — 1,6 г белка на кг.\n\n\
         Ваша планка по белку: {target} г."
    )
}

/// The veg/fruit "?" text — sex-specific.
fn veg_hint_text() -> String {
    let who = match profile::get_sex() {
        Some(Sex::Female) => "Женщинам",
        _ => "Мужчинам",
    };
    format!(
        "{who} рекомендуется употреблять 400–600 г овощей и фруктов в день. Они могут быть в \
         готовом или сыром виде."
    )
}

/// The "?" explanation for a daily indicator's colour — how many of the last 7 days
/// missed the target, the green/orange/red rule, and why it matters.
fn indicator_reason(key: &str, state: IndicatorState, missed: u32) -> String {
    use IndicatorState::*;
    let metric = match key {
        "protein" => "белка",
        "veg_fruit" => "овощей и фруктов",
        _ => "нормы",
    };
    let head = match state {
        Green => format!("Зелёный: за последнюю неделю вы каждый день добирали норму {metric}."),
        Orange => format!(
            "Оранжевый: за последнюю неделю вы {missed} из 7 дней не добрали норму {metric} \
             (1–3 дня → оранжевый)."
        ),
        Red => format!(
            "Красный: за последнюю неделю вы {missed} из 7 дней не добрали норму {metric} \
             (4 и более → красный)."
        ),
        Unknown => return "Пока недостаточно данных, чтобы оценить.".to_string(),
    };
    let tail = match (key, state) {
        ("protein", Orange) | ("protein", Red) => {
            " Регулярный недобор белка грозит потерей мышц и сказывается на здоровье в долгую."
        }
        ("veg_fruit", Orange) | ("veg_fruit", Red) => {
            " Нехватка овощей и фруктов — это дефицит клетчатки и витаминов."
        }
        _ => "",
    };
    format!("{head}{tail}")
}

/// Current network problems as error-log entries, so they appear in the SAME list
/// as background errors (and drive the ⚠ tile) instead of a banner over the UI.
/// Reads the `net` signals, so it auto-clears when connectivity is restored — no
/// mutation of the persistent error log. `None` (unprobed) is not a problem.
fn net_problem_entries() -> Vec<AppError> {
    let mut v = Vec::new();
    if net::is_online().get() == Some(false) {
        v.push(AppError {
            context: t("net.offline_title").to_string(),
            message: t("net.offline_body_vpn").to_string(),
        });
    }
    let down = net::degraded().get();
    if !down.is_empty() {
        let names = down.iter().map(|w| t(w.label_key())).collect::<Vec<_>>().join(", ");
        v.push(AppError {
            context: t("net.degraded_title").to_string(),
            message: format!("{} {}", t("net.degraded_body"), names),
        });
    }
    v
}

// Process-lifetime caches for the async widget data, so re-navigating to the
// dashboard paints the widgets with the last-known values on the FIRST frame
// instead of flashing an empty/placeholder state (see `services::sticky`).
thread_local! {
    static WEIGHT_CACHE: RefCell<Option<Vec<WeightEntry>>> = const { RefCell::new(None) };
    static STEPS_CACHE: RefCell<Option<Vec<StepEntry>>> = const { RefCell::new(None) };
}

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
    Progress,
    Weight,
    Steps,
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
    // received → stop jiggling; owned by the push service) and `disabled` (the
    // master kill-switch → cross the bell out; re-read when the schedule changes).
    let meta_ver = db::version("_sync_meta");
    let notif_configured = crate::services::push::received_signal();
    let notif_disabled = create_rw_signal(false);
    create_effect(move |_| {
        meta_ver.get();
        spawn_local(async move {
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
    // `_data` is `None` only before the first-ever load (→ render nothing); after
    // that it's the fresh-or-last-known Vec, so switching panels shows data at once.
    let weight_data = move || sticky(&WEIGHT_CACHE, weight_res.get());
    let weight_entries = move || weight_data().unwrap_or_default();
    let steps_ver = db::version("step_entries");
    let steps_res = create_resource(move || steps_ver.get(), |_| async { local::list_step_entries().await });
    let steps_data = move || sticky(&STEPS_CACHE, steps_res.get());
    let steps_entries = move || steps_data().unwrap_or_default();

    // The expanded «Калории» view (opened by tapping the widget): the same gauges as
    // the widget + a breakdown of the daily indicators + the "?" explanations.
    // Fetched ONLY while the overlay is open, refreshed when the diary / foods /
    // weight / goals change.
    let diary_ver = db::version("diary");
    let foods_ver = db::version("foods");
    let goals_ver = db::version("goals");
    let detail_res = create_local_resource(
        move || {
            (
                overlay.get() == Overlay::Progress,
                diary_ver.get(),
                foods_ver.get(),
                weight_ver.get(),
                goals_ver.get(),
            )
        },
        |(open, _, _, _, _)| async move {
            if !open {
                return None;
            }
            let today = chrono::Local::now().format("%Y-%m-%d").to_string();
            let planka = local::calorie_goal_amount().await;
            Some(DetailData {
                planka,
                eaten: local::kcal_on(&today).await,
                gauges: indicators::daily_gauges().await,
                series: indicators::unlocked_indicator_series().await,
                calorie_hint: calorie_hint_text(planka).await,
                protein_hint: protein_hint_text().await,
                veg_hint: veg_hint_text(),
            })
        },
    );

    // Problems tile: the ⚠ tile (left of the bell) appears when there are
    // background errors OR a network problem. Network problems are shown in the
    // SAME list (ErrorsPanel) — no banner covering the interface.
    let errs = crate::services::errors::signal();
    let has_errors = move || !errs.get().is_empty() || !net_problem_entries().is_empty();

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
            } else if overlay.get() == Overlay::Progress {
                view! {
                    <div style=EDITOR>
                        <EditorHead title="dashboard.calories_title"
                            show_done=Signal::derive(|| true)
                            on_done=move || overlay.set(Overlay::None)/>
                        {move || {
                            // Wait for the detail data. Still loading → spinner.
                            let Some(d) = detail_res.get().flatten() else {
                                return view! {
                                    <div style="display: flex; justify-content: center; padding: 2rem;">
                                        <div class="ft-spinner"></div>
                                    </div>
                                }.into_view();
                            };
                            let Some(target) = d.planka else {
                                // No planka yet (still collecting the week).
                                return view! {
                                    <p class="is-size-7 has-text-grey" style="padding: 1.5rem 0.5rem; text-align: center;">
                                        "Планка ещё не рассчитана — ведите дневник неделю."
                                    </p>
                                }.into_view();
                            };
                            // Planka set → the same gauges as the widget (each with a "?"
                            // explaining where its target came from) + an advisory breakdown
                            // of the daily indicators. Order the hint by gauge key.
                            let cal_color =
                                if d.eaten > target { "#e0304f" } else { "#1fa463" }.to_string();
                            let (protein_hint, veg_hint) = (d.protein_hint.clone(), d.veg_hint.clone());
                            let daily = d.gauges.iter().map(|g| {
                                let hint = if g.key == "protein" { protein_hint.clone() } else { veg_hint.clone() };
                                // At-least goals: neutral until met, green when met.
                                let (bar, val) = crate::components::gauge::at_least_colors(g.value, g.target);
                                view! {
                                    <Gauge value=g.value target=g.target
                                        label=gauge_label(g.key).to_string()
                                        unit=g.unit.to_string()
                                        color=bar.to_string()
                                        hint=hint
                                        value_color=val.map(String::from)/>
                                }
                            }).collect_view();
                            let detail = d.series.iter().map(|s| {
                                let (paths, name) = progress_widget::icon_for(s.key);
                                let (stroke, tint) = progress_widget::state_colors(s.state);
                                let reason = indicator_reason(s.key, s.state, s.missed);
                                let days = s.days.clone();
                                // Each indicator on its own bordered panel: a header row
                                // [icon] [name] … [?] naming which indicator this is, with
                                // the histogram full-width below.
                                view! {
                                    <div style="display: flex; flex-direction: column; gap: 6px; \
                                            border: 0.5px solid var(--bulma-border-weak); border-radius: 12px; \
                                            padding: 8px 10px; background: var(--bulma-scheme-main-bis);">
                                        <div style="display: flex; align-items: center; gap: 8px;">
                                            <div style=format!("width: 28px; height: 28px; min-width: 28px; border-radius: 50%; \
                                                    background: {tint}; display: flex; align-items: center; justify-content: center;")>
                                                <svg xmlns="http://www.w3.org/2000/svg" width="16" height="16" viewBox="0 0 24 24"
                                                    fill="none" stroke=stroke stroke-width="2" stroke-linecap="round"
                                                    stroke-linejoin="round" inner_html=paths></svg>
                                            </div>
                                            <span class="has-text-weight-semibold" style="flex: 1; min-width: 0;">{name}</span>
                                            <InfoHint text=reason/>
                                        </div>
                                        <DayBars series=Signal::derive(move || days.clone())
                                            unit="г".to_string()
                                            miss_color=stroke.to_string()/>
                                    </div>
                                }
                            }).collect_view();
                            view! {
                                <div style="display: flex; flex-direction: column; gap: 18px; padding: 4px 2px;">
                                    <Gauge value=d.eaten target=target
                                        label="Калории".to_string() unit="ккал".to_string()
                                        color=cal_color height=12.0
                                        hint=d.calorie_hint.clone()
                                        value_color={(d.eaten > target).then(|| "#e0304f".to_string())}/>
                                    {daily}
                                    <div>
                                        <div style="border-top: 0.5px solid var(--bulma-border-weak); margin-bottom: 14px;"></div>
                                        <div style="display: flex; flex-direction: column; gap: 12px;">
                                            {detail}
                                        </div>
                                    </div>
                                </div>
                            }.into_view()
                        }}
                    </div>
                }.into_view()
            } else if overlay.get() == Overlay::Weight {
                view! {
                    <div style=EDITOR>
                        <EditorHead title="weight.widget_title"
                            show_done=Signal::derive(|| true)
                            on_done=move || overlay.set(Overlay::None)/>
                        <WeightPanel entries=Signal::derive(weight_entries)
                            on_close=Callback::new(move |_| overlay.set(Overlay::None))/>
                    </div>
                }.into_view()
            } else if overlay.get() == Overlay::Steps {
                view! {
                    <div style=EDITOR>
                        <EditorHead title="steps.title"
                            show_done=Signal::derive(|| true)
                            on_done=move || overlay.set(Overlay::None)/>
                        <StepsPanel entries=Signal::derive(steps_entries)
                            on_close=Callback::new(move |_| overlay.set(Overlay::None))/>
                    </div>
                }.into_view()
            } else {
                // Collapsed grid: persona 1×1 + notifications bell 1×1.
                view! {
                    <div style="display: flex; flex-direction: column; gap: 12px;">
                        <crate::components::story_tray::StoryTray/>
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
                                on:click=move |_| overlay.set(Overlay::Weight)>
                                // Empty until the first data load (then sticky keeps it
                                // filled across navigations) — no placeholder flash.
                                {move || weight_data().map(|_| view! { <WeightWidget entries=Signal::derive(weight_entries)/> })}
                            </button>
                            <button style=format!("{WIDGET_TILE} grid-column: 5 / 9; grid-row: 2 / 5;")
                                attr:data-testid="dash-steps-widget"
                                on:click=move |_| overlay.set(Overlay::Steps)>
                                {move || steps_data().map(|_| view! { <StepsWidget entries=Signal::derive(steps_entries)/> })}
                            </button>

                        </div>

                        // Progress widget + cycle flow BELOW the grid (not inside it),
                        // so the card grows to fit its content — the indicators row
                        // sits at the very bottom instead of being clipped.
                        //
                        // Tapping the card opens the expanded «Калории» view. Uses
                        // `pointerup` (native, fires on iOS unlike a delegated <div>
                        // click); the help link/buttons inside stop propagation so
                        // they keep their own action.
                        <div on:pointerup=move |_| overlay.set(Overlay::Progress)>
                            <ProgressWidget/>
                        </div>

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
                // Network problems first (live, from `net`), then the recorded
                // background errors — one combined list.
                let mut list = net_problem_entries();
                list.extend(errs.get());
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
