use leptos::*;
use api_types::*;

use api_types::NutrientSpec;

use crate::components::diary_add_modal::DiaryAddModal;
use crate::components::food_weight_modal::FoodWeightModal;
use crate::services::{local, sync};
use crate::services::i18n::t;

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
    let editing = create_rw_signal(None::<(String, Food, f64)>);
    let menu_open = create_rw_signal(None::<String>);

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

    let foods = move || foods_res.get().unwrap_or_default();
    let goals = move || goals_res.get().unwrap_or_default();
    let entries = move || entries_res.get().unwrap_or_default();
    let today_entries = move || today_entries_res.get().unwrap_or_default();
    let week_entries = move || week_entries_res.get().unwrap_or_default();

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

    let nutrient_sum = move |nutrient: &str, es: &[DiaryEntry], fs: &[Food]| -> f64 {
        es.iter().map(|e| {
            let food = fs.iter().find(|f| f.id == e.food_id);
            food.map(|f| {
                let factor = e.grams / 100.0;
                match nutrient {
                    "Calories" => f.kcal * factor,
                    "Protein" => f.protein * factor,
                    "Fat" => f.fat * factor,
                    "Carbs" => f.carbs * factor,
                    custom => f.nutrients.get(custom).copied().unwrap_or(0.0) * factor,
                }
            }).unwrap_or(0.0)
        }).sum()
    };

    view! {
        <div style="display: flex; flex-direction: column; height: calc(100vh - 7rem);">
        // Sticky header: date + goals
        <div style="flex-shrink: 0;">
            // Date navigation: [←] [Вчера] [→]
            // - Label shows relative date: Сегодня / Вчера / Позавчера / day-of-week (3-7 days) / "4 июня 2026"
            // - Tap on label opens native date picker via hidden <input type="date"> + showPicker()
            // - Hidden input must stay in DOM and in viewport (1x1px, opacity 0) — otherwise showPicker() fails
            // - Forward button disabled when date == today (no future dates)
            // - max on date input also prevents selecting future dates in the picker
            <div style="display: flex; align-items: center; justify-content: space-between; margin-bottom: 1rem;">
                <button
                    class="button is-light is-rounded"
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
                        class="button is-white is-size-5 has-text-weight-semibold"
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
                    class="button is-light is-rounded"
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

                    gs.iter().map(|goal| {
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
                                if current >= target { "#48c78e" } else { "#b5b5b5" }
                            } else {
                                if current > target { "#f14668" } else { "#48c78e" }
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
                                    <div style="height: 6px; background: #f0f0f0; border-radius: 3px; overflow: hidden;">
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
                                    let factor = e.grams / 100.0;
                                    match name.as_str() {
                                        "Calories" => f.kcal * factor,
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
                            let bar_color = if on_track { "#48c78e" } else { "#f14668" };

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
                                        <div style="height: 6px; background: #f0f0f0; border-radius: 3px; overflow: hidden;">
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
                                                {format!(" / {target:.1} {unit} {}", t("diary.per_week"))}
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

        </div>
            // Entries list — scrollable, padding-bottom for FAB
            <div style="flex: 1; overflow-y: auto; padding-bottom: 5rem;">
                {move || entries().into_iter().map(|entry| {
                    let entry_id = entry.id.clone();
                    let entry_id2 = entry.id.clone();
                    let entry_id3 = entry.id.clone();
                    let entry_id4 = entry.id.clone();
                    let fid = entry.food_id.clone();
                    let fid2 = entry.food_id.clone();
                    let fid3 = entry.food_id.clone();
                    let fid4 = entry.food_id.clone();
                    let g = entry.grams;
                    view! {
                            <div style="display: flex; align-items: center; padding: 0.5rem 0; border-bottom: 1px solid #f0f0f0;">
                                <div style="flex: 1; min-width: 0; overflow-wrap: break-word;">
                                    <span class="is-size-6 has-text-weight-medium">
                                        {move || food_name(&fid)}
                                    </span>
                                    <div class="tags mt-1" style="margin-bottom: 0;">
                                        {move || {
                                            let fs = foods();
                                            let food = fs.iter().find(|f| f.id == fid2);
                                            let factor = g / 100.0;
                                            let mut badges = Vec::new();
                                            use crate::services::i18n;
                                            if let Some(f) = food {
                                                badges.push((i18n::nutrient_badge("Calories"), f.kcal * factor, i18n::unit_label("kcal")));
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
                                                <span class="tag is-light is-small">
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
                                            let eid2 = entry_id2.clone();
                                            let eid3 = entry_id3.clone();
                                            let eid4 = entry_id4.clone();
                                            let fid_e = fid3.clone();
                                            view! {
                                                <button
                                                    class="button is-ghost is-small has-text-link"
                                                    style="height: auto; text-decoration: none;"
                                                    on:click=move |_| {
                                                        if let Some(food) = foods().into_iter().find(|f| f.id == fid_e) {
                                                            editing.set(Some((eid.clone(), food, g)));
                                                        }
                                                    }
                                                >
                                                    <span class="is-size-7">{format!("{:.0}{}", g, t("common.unit.g"))}</span>
                                                </button>
                                                <div style="position: relative;">
                                                    <button
                                                        class="button is-ghost has-text-grey-light"
                                                        style="height: 2.5rem; width: 2.5rem; padding: 0; text-decoration: none;"
                                                        on:click=move |_| {
                                                            menu_open.update(|m| {
                                                                if m.as_deref() == Some(&eid2) { *m = None; }
                                                                else { *m = Some(eid2.clone()); }
                                                            });
                                                        }
                                                    >
                                                        <svg xmlns="http://www.w3.org/2000/svg" width="20" height="20" viewBox="0 0 20 20" fill="currentColor">
                                                            <path fill-rule="evenodd" d="M9 2a1 1 0 00-.894.553L7.382 4H4a1 1 0 000 2v10a2 2 0 002 2h8a2 2 0 002-2V6a1 1 0 100-2h-3.382l-.724-1.447A1 1 0 0011 2H9zM7 8a1 1 0 012 0v6a1 1 0 11-2 0V8zm5-1a1 1 0 00-1 1v6a1 1 0 102 0V8a1 1 0 00-1-1z" clip-rule="evenodd" />
                                                        </svg>
                                                    </button>
                                                    <Show when=move || menu_open.get().as_deref() == Some(&eid3)>
                                                        <div style="position: absolute; right: 0; top: 100%; z-index: 10; background: white; border-radius: 6px; box-shadow: 0 2px 12px rgba(0,0,0,0.15); min-width: 8rem; padding: 0.25rem 0;">
                                                            <button
                                                                class="button is-ghost is-small is-fullwidth has-text-danger"
                                                                style="justify-content: flex-start; text-decoration: none;"
                                                                on:click={
                                                                    let id = eid4.clone();
                                                                    move |_| {
                                                                        delete_entry(id.clone());
                                                                        menu_open.set(None);
                                                                    }
                                                                }
                                                            >{t("diary.delete")}</button>
                                                        </div>
                                                    </Show>
                                                </div>
                                            }.into_view()
                                        } else {
                                            let eid = entry_id.clone();
                                            let eid2 = entry_id2.clone();
                                            let fid_c = fid3.clone();
                                            let fid_r = fid4.clone();
                                            // Check IndexedDB state (via resource) — not a stale copy
                                            let already_copied = move || {
                                                today_entries().iter().any(|e| e.food_id == fid_c)
                                            };
                                            view! {
                                                <span class="is-size-7 has-text-grey">{format!("{:.0}{}", g, t("common.unit.g"))}</span>
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
                                                        <div style="position: absolute; right: 0; top: 100%; z-index: 10; background: white; border-radius: 6px; box-shadow: 0 2px 12px rgba(0,0,0,0.15); min-width: 10rem; padding: 0.25rem 0;">
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
                                                                            let _ = local::save_food_to_diary(&food, g).await;
                                                                            invalidate();
                                                                            sync::push_background();
                                                                        }
                                                                    });
                                                                    }
                                                                }
                                                            >{t("diary.repeat_today")}</button>
                                                        </div>
                                                    </Show>
                                                </div>
                                            }.into_view()
                                        }
                                    }}
                                </div>
                            </div>
                    }
                }).collect::<Vec<_>>()}
            </div>

            <Show when=move || entries().is_empty()>
                <p class="has-text-grey is-size-7 has-text-centered mt-4">{t("diary.no_entries")}</p>
            </Show>

            // Floating green + button
            <Show when=move || !show_add.get()>
                <button
                    class="button is-success is-rounded"
                    style="position: fixed; bottom: 5.5rem; right: 1.5rem; z-index: 41; width: 3.5rem; height: 3.5rem; font-size: 1.5rem; box-shadow: 0 4px 12px rgba(0,0,0,0.2); border: none;"
                    on:click=move |_| show_add.set(true)
                >"+"</button>
            </Show>

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
                editing.get().map(|(entry_id, food, current_grams)| {
                    view! {
                        <FoodWeightModal
                            food=food
                            goals=Signal::derive(goals)
                            initial_grams=current_grams
                            submit_label=t("weight.save")
                            on_save=Callback::new({
                                let eid = entry_id.clone();
                                move |new_grams: f64| {
                                    let eid = eid.clone();
                                    spawn_local(async move {
                                        let _ = local::update_diary_entry(&eid, new_grams).await;
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
        </div>
    }
}
