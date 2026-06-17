use leptos::*;
use api_types::*;

use api_types::NutrientSpec;

use crate::components::diary_add_modal::DiaryAddModal;
use crate::components::food_weight_modal::FoodWeightModal;
use crate::components::weight_widget::WeightWidget;
use crate::components::weight_chart_modal::WeightChartModal;
use crate::components::steps_widget::StepsWidget;
use crate::components::steps_chart_modal::StepsChartModal;
use crate::components::summary_block::SummaryBlock;
use crate::components::food_edit_modal::FoodEditModal;
use crate::services::{local, sync, db, story};
use crate::services::i18n::t;

/// Button reset so a native <button> can wrap a widget card transparently
/// (iOS Safari fires clicks reliably on buttons, not on bare <div>s).
const WIDGET_BTN: &str = "min-width: 0; cursor: pointer; appearance: none; -webkit-appearance: none; border: none; background: none; padding: 0; margin: 0; font: inherit; color: inherit; text-align: left; display: block;";

fn format_date_relative(date_str: &str) -> String {
    use chrono::Datelike;
    let today = chrono::Local::now().date_naive();
    let date = match chrono::NaiveDate::parse_from_str(date_str, "%Y-%m-%d") {
        Ok(d) => d,
        Err(_) => return date_str.to_string(),
    };
    let diff = (today - date).num_days();

    match diff {
        d if d < 0 => date_str.to_string(),
        0 => t("diary.today").to_string(),
        1 => t("diary.yesterday").to_string(),
        2 => t("diary.day_before").to_string(),
        3..=7 => {
            match date.weekday() {
                chrono::Weekday::Mon => t("diary.weekday.mon"),
                chrono::Weekday::Tue => t("diary.weekday.tue"),
                chrono::Weekday::Wed => t("diary.weekday.wed"),
                chrono::Weekday::Thu => t("diary.weekday.thu"),
                chrono::Weekday::Fri => t("diary.weekday.fri"),
                chrono::Weekday::Sat => t("diary.weekday.sat"),
                chrono::Weekday::Sun => t("diary.weekday.sun"),
            }
            .to_string()
        }
        _ => {
            let month = match date.month() {
                1 => t("diary.month.1"),
                2 => t("diary.month.2"),
                3 => t("diary.month.3"),
                4 => t("diary.month.4"),
                5 => t("diary.month.5"),
                6 => t("diary.month.6"),
                7 => t("diary.month.7"),
                8 => t("diary.month.8"),
                9 => t("diary.month.9"),
                10 => t("diary.month.10"),
                11 => t("diary.month.11"),
                12 => t("diary.month.12"),
                _ => "",
            };
            format!("{} {} {}", date.day(), month, date.year())
        }
    }
}

fn format_date_past_prefix(date_str: &str) -> String {
    use chrono::Datelike;
    let today = chrono::Local::now().date_naive();
    let date = match chrono::NaiveDate::parse_from_str(date_str, "%Y-%m-%d") {
        Ok(d) => d,
        Err(_) => return date_str.to_string(),
    };
    let diff = (today - date).num_days();

    match diff {
        1 => t("diary.yesterday").to_string(),
        2 => t("diary.day_before").to_string(),
        3..=7 => {
            match date.weekday() {
                chrono::Weekday::Mon => t("diary.weekday_prep.mon"),
                chrono::Weekday::Tue => t("diary.weekday_prep.tue"),
                chrono::Weekday::Wed => t("diary.weekday_prep.wed"),
                chrono::Weekday::Thu => t("diary.weekday_prep.thu"),
                chrono::Weekday::Fri => t("diary.weekday_prep.fri"),
                chrono::Weekday::Sat => t("diary.weekday_prep.sat"),
                chrono::Weekday::Sun => t("diary.weekday_prep.sun"),
            }
            .to_string()
        }
        _ => {
            let month = match date.month() {
                1 => t("diary.month.1"),
                2 => t("diary.month.2"),
                3 => t("diary.month.3"),
                4 => t("diary.month.4"),
                5 => t("diary.month.5"),
                6 => t("diary.month.6"),
                7 => t("diary.month.7"),
                8 => t("diary.month.8"),
                9 => t("diary.month.9"),
                10 => t("diary.month.10"),
                11 => t("diary.month.11"),
                12 => t("diary.month.12"),
                _ => "",
            };
            format!("{} {} {}", date.day(), month, date.year())
        }
    }
}

/// Best-effort haptic tick. Works on Android (Vibration API); iOS Safari/PWA has
/// no `navigator.vibrate` AT ALL, so we MUST feature-detect — calling the absent
/// method throws, which previously aborted the long-press callback before the
/// menu opened. Feature-detected → silent no-op on iOS.
fn haptic(ms: u32) {
    let Some(w) = web_sys::window() else { return };
    let nav = w.navigator();
    if let Ok(f) = js_sys::Reflect::get(&nav, &wasm_bindgen::JsValue::from_str("vibrate")) {
        if f.is_function() {
            let _ = nav.vibrate_with_duration(ms);
        }
    }
}

fn is_standard_nutrient(name: &str) -> bool {
    matches!(name, "Calories" | "Protein" | "Fat" | "Carbs")
}

fn week_dates(date_str: &str) -> Vec<String> {
    use chrono::Datelike;
    let date = chrono::NaiveDate::parse_from_str(date_str, "%Y-%m-%d")
        .unwrap_or_else(|_| chrono::Local::now().date_naive());
    let weekday = date.weekday().num_days_from_monday();
    let monday = date - chrono::Duration::days(weekday as i64);
    (0..7)
        .map(|i| (monday + chrono::Duration::days(i)).format("%Y-%m-%d").to_string())
        .collect()
}

fn weekday_label(date_str: &str) -> &'static str {
    use chrono::Datelike;
    let date = chrono::NaiveDate::parse_from_str(date_str, "%Y-%m-%d").unwrap_or_default();
    match date.weekday() {
        chrono::Weekday::Mon => t("diary.weekday_short.mon"),
        chrono::Weekday::Tue => t("diary.weekday_short.tue"),
        chrono::Weekday::Wed => t("diary.weekday_short.wed"),
        chrono::Weekday::Thu => t("diary.weekday_short.thu"),
        chrono::Weekday::Fri => t("diary.weekday_short.fri"),
        chrono::Weekday::Sat => t("diary.weekday_short.sat"),
        chrono::Weekday::Sun => t("diary.weekday_short.sun"),
    }
}

#[component]
pub fn DiaryPage() -> impl IntoView {
    let today_str = chrono::Local::now().format("%Y-%m-%d").to_string();
    let today_max = today_str.clone();
    let date = create_rw_signal(today_str);

    // Version counter: bump after any write → all resources re-read from IndexedDB
    let version = create_rw_signal(0u32);

    let show_add = create_rw_signal(false);
    let editing = create_rw_signal(None::<(String, Food, f64, f64, bool)>);
    let menu_open = create_rw_signal(None::<String>);
    // The diary entry whose product is being edited (КБЖУ + name, CoW on save).
    let edit_food = create_rw_signal(None::<(String, Food)>);

    // All data comes from IndexedDB via resources. No manual signal mutation.
    let foods_res = create_resource(
        move || version.get(),
        |_| async { local::list_foods().await },
    );
    let goals_res = create_resource(
        move || version.get(),
        |_| async { local::list_goals().await },
    );
    let entries_res = create_resource(
        move || (date.get(), version.get()),
        |(d, _)| async move { local::list_diary(&d).await },
    );
    let today_entries_res = create_resource(
        move || version.get(),
        |_| async {
            let today = chrono::Local::now().format("%Y-%m-%d").to_string();
            local::list_diary(&today).await
        },
    );

    // Week entries for weekly goals (Mon-Sun of selected date's week)
    let week_entries_res = create_resource(
        move || (date.get(), version.get()),
        |(d, _)| async move {
            let dates = week_dates(&d);
            local::list_diary_range(&dates).await
        },
    );

    let weight_res = create_resource(
        move || version.get(),
        |_| async { local::list_weight_entries().await },
    );

    // The weight chart appears at the same moment as the weigh-in reminder
    // toggle in settings — i.e. once the setup section is done.
    let story_ver = db::version("story");
    let setup_done_res = create_resource(
        move || story_ver.get(),
        |_| async {
            story::get_flag(story::LANGUAGE_CONFIGURED).await
                && story::get_flag(story::NOTIFICATION_RECEIVED).await
        },
    );
    let setup_done = move || setup_done_res.get().unwrap_or(false);
    let show_weight_modal = create_rw_signal(false);

    // Chapter 2 / s6: once the meal-split section has been opened, the day's
    // diary entries are grouped into derived meals. Until then, keep the flat
    // list unchanged (no regression).
    let meal_split_res = create_resource(
        move || story_ver.get(),
        |_| async { story::get_flag(story::MEAL_SPLIT_UNLOCKED).await },
    );
    let meal_split_on = move || meal_split_res.get().unwrap_or(false);

    let steps_res = create_resource(
        move || version.get(),
        |_| async { local::list_step_entries().await },
    );
    let steps_entries = move || steps_res.get().unwrap_or_default();
    let show_steps_modal = create_rw_signal(false);

    let foods = move || foods_res.get().unwrap_or_default();
    let goals = move || goals_res.get().unwrap_or_default();
    let entries = move || entries_res.get().unwrap_or_default();
    let today_entries = move || today_entries_res.get().unwrap_or_default();
    let week_entries = move || week_entries_res.get().unwrap_or_default();
    let weight_entries = move || weight_res.get().unwrap_or_default();

    let custom_nutrients = move || -> Vec<NutrientSpec> {
        goals()
            .into_iter()
            .filter(|g| !matches!(g.nutrient.as_str(), "Calories" | "Protein" | "Fat" | "Carbs"))
            .map(|g| NutrientSpec {
                key: g.key,
                unit_label: g.unit.label().to_string(),
                name: g.nutrient,
            })
            .collect()
    };

    let food_name = move |food_id: &str| -> String {
        foods()
            .iter()
            .find(|f| f.id == food_id)
            .map(|f| f.name.clone())
            .unwrap_or_default()
    };

    let invalidate = move || version.update(|v| *v += 1);

    let delete_entry = move |entry_id: String| {
        spawn_local(async move {
            match local::remove_food_diary(&entry_id).await {
                Ok(()) => {
                    invalidate();
                    sync::push_background();
                }
                Err(e) => leptos::logging::error!("failed to delete diary entry: {}", e),
            }
        });
    };

    let change_date = move |delta: i64| {
        let d = date.get_untracked();
        if let Ok(parsed) = chrono::NaiveDate::parse_from_str(&d, "%Y-%m-%d") {
            let new = parsed + chrono::Duration::days(delta);
            let today = chrono::Local::now().date_naive();
            if new <= today {
                date.set(new.format("%Y-%m-%d").to_string());
            }
        }
    };

    let is_today = move || {
        let today = chrono::Local::now().format("%Y-%m-%d").to_string();
        date.get() == today
    };

    let duplicate_entry = move |entry_id: String| {
        spawn_local(async move {
            local::duplicate_diary_entry(&entry_id).await;
            invalidate();
            sync::push_background();
        });
    };

    let nutrient_sum = move |nutrient: &str, es: &[DiaryEntry], fs: &[Food]| -> f64 {
        es.iter().map(|e| {
            let food = fs.iter().find(|f| f.id == e.food_id);
            food.map(|f| {
                let factor = (e.grams - e.waste_grams).max(0.0) / 100.0;
                match nutrient {
                    "Calories" => f.effective_kcal() * factor,
                    "Protein" => f.protein * factor,
                    "Fat" => f.fat * factor,
                    "Carbs" => f.carbs * factor,
                    custom => f.nutrients.get(custom).copied().unwrap_or(0.0) * factor,
                }
            }).unwrap_or(0.0)
        }).sum()
    };

    view! {
        // Document-scroll page (NOT a fixed shell + inner overflow): on iOS the
        // inner accelerated overflow layer loses its touch-scroll region when the
        // compositor is disrupted (resume, rotate, a Plex PiP overlay), freezing
        // the pan. Scrolling the document itself is robust. Only the date row is
        // `position: sticky` (below); the FAB stays `position: fixed`.
        <div style="background: var(--bulma-background); min-height: 100dvh;">
        // Tap-away backdrop: closes the open row menu when tapping outside it.
        // pointerdown, not click: iOS Safari doesn't fire `click` on a bare <div>
        // on tap, so the tap-away close never worked on iPhone.
        {move || menu_open.get().is_some().then(|| view! {
            <div style="position: fixed; inset: 0; z-index: 9;"
                on:pointerdown=move |_| menu_open.set(None)></div>
        })}
        // Date row + goal gauges + widgets flow DIRECTLY in the page container (no
        // wrapper): a sticky element pins only within its parent's box, so the date
        // row must be a child of the full-height page — otherwise it detaches at the
        // bottom of a short header wrapper.
            // Date navigation: [←] [Вчера] [→]
            // - Label shows relative date: Сегодня / Вчера / Позавчера / day-of-week (3-7 days) / "4 июня 2026"
            // - Tap on label opens native date picker via hidden <input type="date"> + showPicker()
            // - Hidden input must stay in DOM and in viewport (1x1px, opacity 0) — otherwise showPicker() fails
            // - Forward button disabled when date == today (no future dates)
            // - max on date input also prevents selecting future dates in the picker
            <div style="position: sticky; top: 0; z-index: 8; background: var(--bulma-background); padding-top: env(safe-area-inset-top); display: flex; align-items: center; justify-content: space-between; margin-bottom: 1rem;">
                <button
                    attr:data-testid="diary-btn-prev-date"
                    class="button is-rounded"
                    style="width: 3rem; height: 3rem; font-size: 1.2rem;"
                    on:click=move |_| change_date(-1)
                >"\u{2190}"</button>

                <div style="position: relative; min-width: 8rem; text-align: center;">
                    <input
                        type="date"
                        max=today_max
                        id="diary-date-picker"
                        style="position: absolute; top: 0; left: 0; width: 1px; height: 1px; opacity: 0; pointer-events: none;"
                        prop:value=move || date.get()
                        on:change=move |ev| {
                            let v = event_target_value(&ev);
                            if !v.is_empty() {
                                date.set(v);
                            }
                        }
                    />
                    <button
                        attr:data-testid="diary-btn-date"
                        class="button is-size-5 has-text-weight-semibold"
                        on:click=move |_| {
                            let doc = web_sys::window().unwrap().document().unwrap();
                            let el = doc.get_element_by_id("diary-date-picker").unwrap();
                            use wasm_bindgen::JsCast;
                            let input: &web_sys::HtmlInputElement = el.unchecked_ref();
                            let _ = input.show_picker();
                        }
                    >
                        {move || format_date_relative(&date.get())}
                    </button>
                </div>

                <button
                    attr:data-testid="diary-btn-next-date"
                    class="button is-rounded"
                    style="width: 3rem; height: 3rem; font-size: 1.2rem;"
                    disabled=move || is_today()
                    on:click=move |_| change_date(1)
                >"\u{2192}"</button>
            </div>

            // Goals gauges
            <div style="margin-bottom: 0.75rem;">
                {move || {
                    let fs = foods();
                    let es = entries();
                    let gs = goals();
                    let we = week_entries();
                    let sel_date = date.get();
                    let dates = week_dates(&sel_date);
                    let today_str = chrono::Local::now().format("%Y-%m-%d").to_string();

                    gs.iter().filter(|g| g.amount > 0.0).map(|goal| {
                        let name = if is_standard_nutrient(&goal.nutrient) {
                            crate::services::i18n::nutrient_name(&goal.nutrient).to_string()
                        } else {
                            goal.nutrient.clone()
                        };
                        let target = goal.amount;
                        let unit = crate::services::i18n::unit_label(goal.unit.label());
                        let is_at_least = goal.direction == GoalDirection::AtLeast;

                        if goal.period == GoalPeriod::Day {
                            // Day goal: single gauge bar
                            let current = nutrient_sum(&goal.nutrient, &es, &fs);
                            let pct = if target > 0.0 { ((current / target) * 100.0).min(100.0) } else { 0.0 };
                            let bar_color = if is_at_least {
                                if current >= target { "var(--bulma-success)" } else { "var(--bulma-text-weak)" }
                            } else {
                                if current > target { "var(--bulma-danger)" } else { "var(--bulma-success)" }
                            };
                            let text_color = if is_at_least {
                                if current >= target { "has-text-success" } else { "" }
                            } else {
                                if current > target { "has-text-danger" } else { "" }
                            };
                            view! {
                                <div style="margin-bottom: 0.5rem;">
                                    <div style="display: flex; align-items: baseline; justify-content: space-between; margin-bottom: 0.15rem;">
                                        <span class="is-size-7 has-text-grey">{name.clone()}</span>
                                        <span class=format!("is-size-7 has-text-weight-semibold {text_color}")>
                                            {format!("{:.0}", current.abs())}
                                            <span class="has-text-grey-light has-text-weight-normal">
                                                {format!(" / {target:.0} {unit}")}
                                            </span>
                                        </span>
                                    </div>
                                    <div style="height: 6px; background: var(--bulma-border-weak); border-radius: 3px; overflow: hidden;">
                                        <div style=format!(
                                            "height: 100%; width: {pct:.0}%; background: {bar_color}; border-radius: 3px; transition: width 0.3s;"
                                        )></div>
                                    </div>
                                </div>
                            }.into_view()
                        } else {
                            // Week goal: cumulative fill spread across 7 bars
                            // ratio = eaten / target, days_filled = ratio * 7
                            // Full bars for completed days, partial for current fill day
                            // Today marker shows expected linear pace
                            let week_total: f64 = we.iter().map(|e| {
                                let food = fs.iter().find(|f| f.id == e.food_id);
                                food.map(|f| {
                                    let factor = (e.grams - e.waste_grams).max(0.0) / 100.0;
                                    match name.as_str() {
                                        "Calories" => f.effective_kcal() * factor,
                                        "Protein" => f.protein * factor,
                                        "Fat" => f.fat * factor,
                                        "Carbs" => f.carbs * factor,
                                        custom => f.nutrients.get(custom).copied().unwrap_or(0.0) * factor,
                                    }
                                }).unwrap_or(0.0)
                            }).sum();

                            let ratio = if target > 0.0 { (week_total / target).min(1.0) } else { 0.0 };
                            let days_filled = ratio * 7.0;
                            let full_days = days_filled.floor() as usize;
                            let partial = days_filled - days_filled.floor();

                            // Today's expected position (day index 0-based from Monday)
                            let today_index = {
                                use chrono::Datelike;
                                let today_d = chrono::Local::now().date_naive();
                                today_d.weekday().num_days_from_monday() as usize
                            };
                            let expected_by_today = target * (today_index + 1) as f64 / 7.0;
                            let on_track = if is_at_least { week_total >= expected_by_today } else { week_total <= expected_by_today };
                            let bar_color = if on_track { "var(--bulma-success)" } else { "var(--bulma-danger)" };

                            let bars: Vec<_> = dates.iter().enumerate().map(|(i, d)| {
                                let fill_pct = if i < full_days {
                                    100.0
                                } else if i == full_days {
                                    partial * 100.0
                                } else {
                                    0.0
                                };
                                let is_future = *d > today_str;
                                let opacity = if is_future { "0.3" } else { "1" };
                                view! {
                                    <div style=format!("flex: 1; opacity: {opacity};")>
                                        <div style="height: 6px; background: var(--bulma-border-weak); border-radius: 3px; overflow: hidden;">
                                            <div style=format!(
                                                "height: 100%; width: {fill_pct:.0}%; background: {bar_color}; border-radius: 3px; transition: width 0.3s;"
                                            )></div>
                                        </div>
                                    </div>
                                }
                            }).collect();

                            let text_color = if on_track { "" } else { "has-text-warning-dark" };

                            view! {
                                <div style="margin-bottom: 0.5rem;">
                                    <div style="display: flex; align-items: baseline; justify-content: space-between; margin-bottom: 0.15rem;">
                                        <span class="is-size-7 has-text-grey">{name.clone()}</span>
                                        <span class=format!("is-size-7 has-text-weight-semibold {text_color}")>
                                            {format!("{:.1}", week_total.abs())}
                                            <span class="has-text-grey-light has-text-weight-normal">
                                                {move || format!(" / {target:.1} {unit} {}", t("diary.per_week"))}
                                            </span>
                                        </span>
                                    </div>
                                    <div style="display: flex; gap: 2px;">
                                        {bars}
                                    </div>
                                </div>
                            }.into_view()
                        }
                    }).collect::<Vec<_>>()
                }}
            </div>

            {move || (is_today() && setup_done()).then(|| view! {
                <div style="display: flex; gap: 0.75rem; align-items: stretch; margin-bottom: 0.75rem;">
                    // Native <button> wrappers: iOS Safari doesn't reliably fire
                    // delegated click events on non-interactive <div>s.
                    <button type="button" attr:data-testid="diary-weight-widget" style=WIDGET_BTN style:flex="1" on:click=move |_| show_weight_modal.set(true)>
                        <WeightWidget entries=Signal::derive(weight_entries) />
                    </button>
                    <button type="button" style=WIDGET_BTN style:flex="1" on:click=move |_| show_steps_modal.set(true)>
                        <StepsWidget entries=Signal::derive(steps_entries) />
                    </button>
                </div>
            })}

            {move || show_weight_modal.get().then(|| view! {
                <WeightChartModal
                    entries=Signal::derive(weight_entries)
                    on_close=Callback::new(move |_| show_weight_modal.set(false))
                />
            })}
            {move || show_steps_modal.get().then(|| view! {
                <StepsChartModal
                    entries=Signal::derive(steps_entries)
                    on_close=Callback::new(move |_| show_steps_modal.set(false))
                />
            })}

            {move || if entries().is_empty() {
                if is_today() {
                    // Today empty: invitation to add first entry
                    view! {
                        <div style="display: flex; flex-direction: column; align-items: center; justify-content: center; padding: 4rem 24px;">
                            <p style="font-size: 17px; color: var(--bulma-text-weak); margin: 0 0 8px 0; text-align: center; line-height: 1.5;">
                                {move || t("diary.empty_today_1")}
                            </p>
                            <p style="font-size: 17px; color: var(--bulma-text-weak); margin: 0 0 24px 0; text-align: center; line-height: 1.5;">
                                {move || t("diary.empty_today_2")}
                            </p>
                            <Show when=move || !show_add.get()>
                                <button
                                    attr:data-testid="diary-btn-add"
                                    class="button is-success is-rounded"
                                    style="width: 3.5rem; height: 3.5rem; font-size: 1.5rem; box-shadow: 0 4px 12px rgba(0,0,0,0.2); border: none;"
                                    on:click=move |_| show_add.set(true)
                                >"+"</button>
                            </Show>
                        </div>
                    }.into_view()
                } else {
                    // Past day empty: no add button, but the weekly report is
                    // still available (the day summary renders nothing if empty).
                    view! {
                        <div style="padding: 0 16px 5rem 16px;">
                            <div style="display: flex; flex-direction: column; align-items: center; justify-content: center; padding: 48px 8px 0 8px;">
                                <p style="font-size: 17px; color: var(--bulma-text-weak); margin: 0; text-align: center; line-height: 1.5;">
                                    {move || format!("{} {}", format_date_past_prefix(&date.get()), t("diary.empty_past"))}
                                </p>
                            </div>
                            <SummaryBlock date=Signal::derive(move || date.get()) />
                        </div>
                    }.into_view()
                }
            } else {
                // Entries list — scrollable. The bottom padding MUST keep the last
                // list item ABOVE the floating "+" FAB so they never overlap: the
                // FAB sits at bottom: 5.5rem and is 3.5rem tall (its top is at 9rem
                // from the viewport bottom), so padding-bottom must exceed that.
                // 10rem = FAB top (9rem) + ~1rem gap. Keep in sync with the FAB
                // position below if it ever changes.
                view! {
                    <div style="padding-bottom: 10rem;">
                        {move || {
                          // Single diary row. Identical regardless of grouping:
                          // the meal-split path interleaves headers between calls
                          // to this, the flat path just maps over it directly.
                          let render_row = move |entry: DiaryEntry| -> View {
                            let entry_id = entry.id.clone();
                            let entry_id2 = entry.id.clone();
                            let fid = entry.food_id.clone();
                            let fid2 = entry.food_id.clone();
                            let fid3 = entry.food_id.clone();
                            let fid4 = entry.food_id.clone();
                            let fid5 = entry.food_id.clone();
                            let g = entry.grams;
                            let w = entry.waste_grams;
                            view! {
                                    <div style="display: flex; align-items: center; padding: 0.5rem 0; border-bottom: 1px solid var(--bulma-border-weak);">
                                        <div style="flex: 1; min-width: 0; overflow-wrap: break-word;">
                                            <span class="is-size-6 has-text-weight-medium"
                                                style=move || if foods().iter().any(|f| f.id == fid5 && f.is_restaurant) { crate::components::food_list_item::RESTAURANT_NAME_STYLE } else { "" }>
                                                {move || food_name(&fid)}
                                            </span>
                                            <div class="tags mt-1" style="margin-bottom: 0;">
                                                {move || {
                                                    let fs = foods();
                                                    let food = fs.iter().find(|f| f.id == fid2);
                                                    let factor = (g - w).max(0.0) / 100.0;
                                                    let mut badges = Vec::new();
                                                    use crate::services::i18n;
                                                    if let Some(f) = food {
                                                        badges.push((i18n::nutrient_badge("Calories"), f.effective_kcal() * factor, i18n::unit_label("kcal")));
                                                        badges.push((i18n::nutrient_badge("Protein"), f.protein * factor, i18n::unit_label("g")));
                                                        badges.push((i18n::nutrient_badge("Fat"), f.fat * factor, i18n::unit_label("g")));
                                                        badges.push((i18n::nutrient_badge("Carbs"), f.carbs * factor, i18n::unit_label("g")));
                                                    }
                                                    let gs = goals();
                                                    let custom_badges: Vec<_> = gs.iter()
                                                        .filter(|goal| goal.period == GoalPeriod::Day)
                                                        .filter(|goal| !matches!(goal.nutrient.as_str(), "Calories" | "Protein" | "Fat" | "Carbs"))
                                                        .map(|goal| {
                                                            let val = food
                                                                .and_then(|f| f.nutrients.get(&goal.nutrient).copied())
                                                                .map(|v| v * factor)
                                                                .unwrap_or(0.0);
                                                            let label: String = goal.nutrient.chars().take(3).collect();
                                                            (label, val, i18n::unit_label(goal.unit.label()).to_string())
                                                        })
                                                        .collect();
                                                    let badge_view = |(l, v, u): &(&str, f64, &str)| view! {
                                                        <span class="tag is-small">
                                                            {format!("{} {:.0}", l, v)}
                                                            " "
                                                            <span class="has-text-grey-light">{u.to_string()}</span>
                                                        </span>
                                                    }.into_view();
                                                    badges.iter()
                                                        .map(|b| badge_view(b))
                                                        .chain(custom_badges.iter().map(|(l, v, u)| {
                                                            let b: (&str, f64, &str) = (l.as_str(), *v, u.as_str());
                                                            badge_view(&b)
                                                        }))
                                                        .collect::<Vec<_>>()
                                                }}
                                            </div>
                                        </div>
                                        // Right side
                                        <div style="flex-shrink: 0; margin-left: 1rem; display: flex; align-items: center; gap: 0.75rem;">
                                            {move || {
                                                if is_today() {
                                                    let eid = entry_id.clone();
                                                    let eid_t = entry_id.clone();
                                                    let eid_s = entry_id.clone();
                                                    let eid_d = entry_id.clone();
                                                    let eid_e = entry_id.clone();
                                                    let eid_del = entry_id.clone();
                                                    let fid_e = fid3.clone();
                                                    let fid_ed = fid3.clone();
                                                    view! {
                                                        <button
                                                            class="button is-ghost is-small has-text-link"
                                                            style="height: auto; text-decoration: none;"
                                                            on:click=move |_| {
                                                                if let Some(food) = foods().into_iter().find(|f| f.id == fid_e) {
                                                                    let r = food.is_restaurant;
                                                                    editing.set(Some((eid.clone(), food, g, w, r)));
                                                                }
                                                            }
                                                        >
                                                            <span class="is-size-7">{move || format!("{:.0}{}", g, t("common.unit.g"))}</span>
                                                        </button>
                                                        // Menu trigger (kebab "⋮" icon). Toggles the action menu,
                                                        // which is anchored directly under this button.
                                                        <div style="position: relative;">
                                                            <button
                                                                class="button is-ghost has-text-grey-light"
                                                                style="height: 2.5rem; width: 2.5rem; padding: 0; text-decoration: none;"
                                                                on:click=move |_| {
                                                                    haptic(15);
                                                                    menu_open.update(|m| {
                                                                        if m.as_deref() == Some(&eid_t) { *m = None; }
                                                                        else { *m = Some(eid_t.clone()); }
                                                                    });
                                                                }
                                                            >
                                                                <svg xmlns="http://www.w3.org/2000/svg" width="20" height="20" viewBox="0 0 20 20" fill="currentColor">
                                                                    <circle cx="10" cy="4" r="1.6"/>
                                                                    <circle cx="10" cy="10" r="1.6"/>
                                                                    <circle cx="10" cy="16" r="1.6"/>
                                                                </svg>
                                                            </button>
                                                            <Show when=move || menu_open.get().as_deref() == Some(&eid_s)>
                                                                <div style="position: absolute; right: 0; top: 100%; z-index: 10; background: var(--bulma-scheme-main); border-radius: 6px; box-shadow: 0 2px 12px rgba(0,0,0,0.15); min-width: 10rem; padding: 0.25rem 0;">
                                                                    <button
                                                                        class="button is-ghost is-small is-fullwidth"
                                                                        style="justify-content: flex-start; text-decoration: none;"
                                                                        on:click={
                                                                            let id = eid_d.clone();
                                                                            move |_| { duplicate_entry(id.clone()); menu_open.set(None); }
                                                                        }
                                                                    >{move || t("diary.duplicate")}</button>
                                                                    <button
                                                                        class="button is-ghost is-small is-fullwidth"
                                                                        style="justify-content: flex-start; text-decoration: none;"
                                                                        on:click={
                                                                            let id = eid_e.clone();
                                                                            let fid_edit = fid_ed.clone();
                                                                            move |_| {
                                                                                if let Some(food) = foods().into_iter().find(|f| f.id == fid_edit) {
                                                                                    edit_food.set(Some((id.clone(), food)));
                                                                                }
                                                                                menu_open.set(None);
                                                                            }
                                                                        }
                                                                    >{move || t("diary.edit")}</button>
                                                                    <button
                                                                        class="button is-ghost is-small is-fullwidth has-text-danger"
                                                                        style="justify-content: flex-start; text-decoration: none;"
                                                                        on:click={
                                                                            let id = eid_del.clone();
                                                                            move |_| { delete_entry(id.clone()); menu_open.set(None); }
                                                                        }
                                                                    >{move || t("diary.delete")}</button>
                                                                </div>
                                                            </Show>
                                                        </div>
                                                    }.into_view()
                                                } else {
                                                    let eid = entry_id.clone();
                                                    let eid2 = entry_id2.clone();
                                                    let fid_c = fid3.clone();
                                                    let fid_r = fid4.clone();
                                                    let already_copied = move || {
                                                        today_entries().iter().any(|e| e.food_id == fid_c)
                                                    };
                                                    view! {
                                                        <span class="is-size-7 has-text-grey">{move || format!("{:.0}{}", g, t("common.unit.g"))}</span>
                                                        <div style="position: relative;">
                                                            <button
                                                                class="button is-ghost"
                                                                style="height: 2.5rem; width: 2.5rem; padding: 0; text-decoration: none;"
                                                                disabled=move || already_copied()
                                                                on:click=move |_| {
                                                                    menu_open.update(|m| {
                                                                        if m.as_deref() == Some(&eid) { *m = None; }
                                                                        else { *m = Some(eid.clone()); }
                                                                    });
                                                                }
                                                            >
                                                                <svg xmlns="http://www.w3.org/2000/svg" width="20" height="20" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">
                                                                    <polyline points="17 1 21 5 17 9"/>
                                                                    <path d="M3 11V9a4 4 0 0 1 4-4h14"/>
                                                                    <polyline points="7 23 3 19 7 15"/>
                                                                    <path d="M21 13v2a4 4 0 0 1-4 4H3"/>
                                                                </svg>
                                                            </button>
                                                            <Show when=move || menu_open.get().as_deref() == Some(&eid2)>
                                                                <div style="position: absolute; right: 0; top: 100%; z-index: 10; background: var(--bulma-scheme-main); border-radius: 6px; box-shadow: 0 2px 12px rgba(0,0,0,0.15); min-width: 10rem; padding: 0.25rem 0;">
                                                                    <button
                                                                        class="button is-ghost is-small is-fullwidth"
                                                                        style="justify-content: flex-start; text-decoration: none;"
                                                                        on:click={
                                                                            let fid = fid_r.clone();
                                                                            move |_| {
                                                                            let fid = fid.clone();
                                                                            menu_open.set(None);
                                                                            spawn_local(async move {
                                                                                if let Some(food) = local::list_foods().await.into_iter().find(|f| f.id == fid) {
                                                                                    let _ = local::save_food_to_diary(&food, g, w, food.is_restaurant).await;
                                                                                    invalidate();
                                                                                    sync::push_background();
                                                                                }
                                                                            });
                                                                            }
                                                                        }
                                                                    >{move || t("diary.repeat_today")}</button>
                                                                </div>
                                                            </Show>
                                                        </div>
                                                    }.into_view()
                                                }
                                            }}
                                        </div>
                                    </div>
                            }.into_view()
                          };

                          if meal_split_on() {
                              // Grouped by derived meal. A header per non-empty
                              // group (name + playful subtitle for the 3 mains),
                              // followed by that group's rows.
                              use crate::services::meal_split::{group_by_meal, MealType};
                              let es = entries();
                              group_by_meal(&es).into_iter().map(|grp| {
                                  let name_key = grp.meal.i18n_key();
                                  let sub_key = match grp.meal {
                                      MealType::Breakfast => Some("meal.breakfast_sub"),
                                      MealType::Lunch => Some("meal.lunch_sub"),
                                      MealType::Dinner => Some("meal.dinner_sub"),
                                      _ => None,
                                  };
                                  let rows = grp.entries.into_iter().map(render_row).collect::<Vec<_>>();
                                  view! {
                                      <div style="padding: 1rem 0 0.25rem 0;">
                                          <span class="is-size-6 has-text-weight-bold">{move || t(name_key)}</span>
                                          {sub_key.map(|sk| view! {
                                              <span class="is-size-7 has-text-grey-light" style="margin-left: 0.5rem;">
                                                  "« " {move || t(sk)} " »"
                                              </span>
                                          })}
                                      </div>
                                      {rows}
                                  }.into_view()
                              }).collect::<Vec<_>>()
                          } else {
                              entries().into_iter().map(render_row).collect::<Vec<_>>()
                          }
                        }}

                        // Daily AI summary + weekly report — past days only.
                        {move || (!is_today()).then(|| view! {
                            <SummaryBlock date=Signal::derive(move || date.get()) />
                        })}
                    </div>

                    // Floating green "+" FAB. MUST be drawn STRICTLY for "today":
                    // you can only log food into the current day. On past days we
                    // show only the day assessment (the SummaryBlock above) and NO
                    // add button — `is_today()` gates it here. (Bug fixed: this used
                    // to render on every day with entries, not just today.)
                    <Show when=move || is_today() && !show_add.get()>
                        <button
                            attr:data-testid="diary-btn-add"
                            class="button is-success is-rounded"
                            style="position: fixed; bottom: 5.5rem; right: 1.5rem; z-index: 41; width: 3.5rem; height: 3.5rem; font-size: 1.5rem; box-shadow: 0 4px 12px rgba(0,0,0,0.2); border: none;"
                            on:click=move |_| show_add.set(true)
                        >"+"</button>
                    </Show>
                }.into_view()
            }}
        // Close the page container. Dialogs below stay SIBLINGS (not nested) so
        // their z-index (50) sits in the root stacking context and beats the nav
        // bar (z-40) — kept as-is from when the shell was position:fixed.
        </div>

            <Show when=move || show_add.get()>
                <DiaryAddModal
                    foods=Signal::derive(foods)
                    goals=Signal::derive(goals)
                    today_entries=Signal::derive(today_entries)
                    custom_nutrients=Signal::derive(custom_nutrients)
                    date=date.get_untracked()
                    on_added=Callback::new(move |_entry: DiaryEntry| {
                        invalidate();
                    })
                    on_food_created=Callback::new(move |_food: Food| {
                        invalidate();
                    })
                    on_close=Callback::new(move |_| show_add.set(false))
                />
            </Show>

            {move || {
                editing.get().map(|(entry_id, food, current_grams, current_waste, current_restaurant)| {
                    view! {
                        <FoodWeightModal
                            food=food
                            goals=Signal::derive(goals)
                            initial_grams=current_grams
                            initial_waste=current_waste
                            initial_restaurant=current_restaurant
                            submit_label=t("weight.save")
                            on_save=Callback::new({
                                let eid = entry_id.clone();
                                move |(new_grams, new_waste, new_restaurant): (f64, f64, bool)| {
                                    let eid = eid.clone();
                                    spawn_local(async move {
                                        let _ = local::update_diary_entry(&eid, new_grams, new_waste, new_restaurant).await;
                                        invalidate();
                                        sync::push_background();
                                    });
                                    editing.set(None);
                                }
                            })
                            on_close=Callback::new(move |_| editing.set(None))
                        />
                    }
                })
            }}

            // "Изменить" from the row long-press: edit the product's КБЖУ + name
            // (copy-on-write on save).
            {move || {
                edit_food.get().map(|(entry_id, food)| view! {
                    <FoodEditModal
                        food=food
                        entry_id=entry_id
                        on_saved=Callback::new(move |_| { invalidate(); sync::push_background(); })
                        on_close=Callback::new(move |_| edit_food.set(None))
                    />
                })
            }}
    }
}
