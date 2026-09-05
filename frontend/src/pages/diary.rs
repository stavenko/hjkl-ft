use std::cell::RefCell;
use std::collections::HashMap;

use leptos::*;
use leptos_router::*;
use api_types::*;

use crate::components::food_weight_modal::FoodWeightModal;
use crate::components::food_edit_modal::FoodEditModal;
use crate::services::sticky::{sticky, sticky_keyed};
use crate::services::{db, local, sync};
use crate::services::i18n::t;

// Process-lifetime caches so navigating back to the diary paints the last-known
// day + food list on the FIRST frame instead of flashing the empty-day state
// (the centered green «+») before the IndexedDB read resolves (see
// `services::sticky`). Diary entries are keyed by date.
thread_local! {
    static DIARY_CACHE: RefCell<HashMap<String, Vec<DiaryEntry>>> = RefCell::new(HashMap::new());
    static FOODS_CACHE: RefCell<Option<Vec<Food>>> = const { RefCell::new(None) };
}

fn format_date_relative(date_str: &str) -> String {
    crate::services::i18n::relative_date(date_str)
}

fn format_date_past_prefix(date_str: &str) -> String {
    use chrono::Datelike;
    let today = local::today_date();
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
        .unwrap_or_else(|_| local::today_date());
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
    let today_str = local::today();
    let today_max = today_str.clone();
    let date = create_rw_signal(today_str.clone());

    // Follow the real "today" across midnight. `date` is a snapshot from mount, so a
    // PWA kept in memory over midnight would keep showing the OLD day — no "+" button
    // (gated by `is_today()`) and none of the new day's entries. When the app is
    // resumed (visibilitychange / window focus), if the user is still on what WAS
    // today (not a past day they navigated to), snap `date` forward to the new today.
    // Нажали «Дневник» в меню — возвращаемся на сегодня, каким бы днём человек ни
    // листал. Роутер на переход в то же место не отвечает, поэтому слушаем нажатие.
    let diary_taps = crate::services::nav::diary_taps();
    create_effect(move |seen: Option<u32>| {
        let n = diary_taps.get();
        // Первый прогон — подписка, а не нажатие: сбрасывать нечего.
        if seen.is_some() {
            let now = local::today();
            if date.get_untracked() != now {
                date.set(now);
            }
        }
        n
    });

    let known_today = store_value(today_str);
    let resync_today = move || {
        let now = local::today();
        if now != known_today.get_value() {
            if date.get_untracked() == known_today.get_value() {
                date.set(now.clone());
            }
            known_today.set_value(now);
        }
    };
    {
        use std::rc::Rc;
        use wasm_bindgen::closure::Closure;
        use wasm_bindgen::JsCast;
        let cb = Rc::new(Closure::<dyn Fn()>::new(move || resync_today()));
        let _ = document()
            .add_event_listener_with_callback("visibilitychange", (*cb).as_ref().unchecked_ref());
        let _ = window()
            .add_event_listener_with_callback("focus", (*cb).as_ref().unchecked_ref());
        // `cb` (an Rc) is moved into the cleanup closure, keeping the JS callback
        // alive for the page's lifetime and removing both listeners on unmount.
        on_cleanup(move || {
            let _ = document()
                .remove_event_listener_with_callback("visibilitychange", (*cb).as_ref().unchecked_ref());
            let _ = window()
                .remove_event_listener_with_callback("focus", (*cb).as_ref().unchecked_ref());
        });
    }

    // Version counter: bump after any write → all resources re-read from IndexedDB
    let own_writes = create_rw_signal(0u32);

    // ЧУЖИЕ записи в базу — тоже повод перечитать. Своего счётчика мало: разбор
    // ленивой записи заканчивается В ФОНЕ, уже после того как страница отрисована,
    // и человек, оставшийся на дневнике, не увидел бы ни разобравшейся записи, ни
    // сообщения о неудаче до тех пор, пока не уйдёт со страницы и не вернётся. Это
    // ровно тот случай, ради которого возможность и делалась: сфотографировал и
    // занимаешься своим делом. Счётчики базы поднимает `db::put`, кто бы ни писал, —
    // фон, синхронизация или сама страница.
    //
    // `foods` здесь не для порядка: разбор ЗАВОДИТ новую еду, и без него строка
    // обновилась бы, а названия в ней остались бы от старого списка.
    let db_diary = db::version("diary");
    let db_foods = db::version("foods");
    let version = Signal::derive(move || own_writes.get() + db_diary.get() + db_foods.get());

    let editing = create_rw_signal(None::<(String, Food, f64, f64, bool)>);
    let menu_open = create_rw_signal(None::<String>);
    // Entry id whose «Дублировать» dialog (pick target meal) is open.
    let dup_target = create_rw_signal(None::<String>);
    // Перенос в другой приём пищи. Окно выбора то же, что у дублирования, но
    // держим отдельным сигналом: у окна разный заголовок и разное действие, а
    // одним сигналом пришлось бы ещё и хранить, что именно затеяли.
    let move_target = create_rw_signal(None::<String>);
    // The diary entry whose product is being edited (КБЖУ + name, CoW on save).
    let edit_food = create_rw_signal(None::<(String, Food)>);

    // All data comes from IndexedDB via resources. No manual signal mutation.
    let foods_res = create_resource(
        move || version.get(),
        |_| async { local::list_foods().await },
    );
    let entries_res = create_resource(
        move || (date.get(), version.get()),
        |(d, _)| async move { local::list_diary(&d).await },
    );
    let today_entries_res = create_resource(
        move || version.get(),
        |_| async {
            let today = local::today();
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

    // The calorie planka that APPLIED on the selected date: today → the live planka,
    // a completed past day → the planka frozen for that day (so it stays valid after
    // the weekly recompute changes the current planka).
    let cal_planka_res = create_resource(
        move || (date.get(), version.get()),
        |(d, _)| async move { crate::services::indicators::calorie_planka_on(&d).await },
    );


    // `_data` is `None` only before the first-ever load of that key (→ render
    // nothing); after that it's the fresh-or-last-known value, so switching to the
    // diary shows the day at once instead of flashing the empty-day invitation.
    let foods_data = move || sticky(&FOODS_CACHE, foods_res.get());
    let entries_data = move || sticky_keyed(&DIARY_CACHE, &date.get(), entries_res.get());
    let foods = move || foods_data().unwrap_or_default();
    let entries = move || entries_data().unwrap_or_default();
    let today_entries = move || today_entries_res.get().unwrap_or_default();
    let week_entries = move || week_entries_res.get().unwrap_or_default();

    let food_name = move |food_id: &str| -> String {
        foods()
            .iter()
            .find(|f| f.id == food_id)
            .map(|f| f.name.clone())
            .unwrap_or_default()
    };

    let invalidate = move || own_writes.update(|v| *v += 1);

    // Какую ленивую запись сейчас правим. Форма одна на обе формы записи: у
    // нераспознанной нижняя половина просто пуста.
    let editing_lazy = create_rw_signal(Option::<DiaryEntry>::None);

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
            let today = local::today_date();
            if new <= today {
                date.set(new.format("%Y-%m-%d").to_string());
            }
        }
    };

    let is_today = move || {
        let today = local::today();
        date.get() == today
    };

    /// Открыт ли выбранный день для правки. Неделя назад — тот же дневник, что и
    /// сегодняшний: те же панели с «+», то же меню строки. Дальше день прожит и
    /// заморожен, и там остаётся только «повторить сегодня».
    let editable = move || local::is_editable_day(&date.get());


    // Считает `local::entry_nutrient`: форм записи стало три, и складывать их
    // умеет одно место, покрытое тестами. Нераспознанная запись даёт ноль — она не
    // еда с неизвестными нутриентами, а обещание разобраться.
    let nutrient_sum = move |nutrient: &str, es: &[DiaryEntry], fs: &[Food]| -> f64 {
        es.iter().map(|e| local::entry_nutrient(e, fs, nutrient)).sum()
    };

    view! {
        // Document-scroll page (NOT a fixed shell + inner overflow): on iOS the
        // inner accelerated overflow layer loses its touch-scroll region when the
        // compositor is disrupted (resume, rotate, a Plex PiP overlay), freezing
        // the pan. Scrolling the document itself is robust. Only the date row is
        // `position: sticky` (below); the FAB stays `position: fixed`.
        <div style="background: var(--bulma-background); min-height: 100dvh;">
        // Правка ленивой записи — поверх дневника, полноэкранным листом. Форма одна
        // на обе формы записи: у нераспознанной нижняя половина пуста.
        {move || editing_lazy.get().map(|e| view! {
            <div attr:data-testid="lazy-edit-sheet"
                style="position: fixed; inset: 0; z-index: 40; background: var(--bulma-background); overflow-y: auto; padding: 16px;">
                <crate::components::lazy_food_edit::LazyFoodEdit
                    entry=e
                    foods=Signal::derive(foods)
                    on_saved=Callback::new(move |_| { editing_lazy.set(None); invalidate(); })
                    on_cancel=Callback::new(move |_| editing_lazy.set(None))
                />
            </div>
        })}

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

            // Шкала дня: планка по калориям.
            //
            // Раньше здесь перебирался список целей, и что покажется человеку,
            // зависело от того, какие цели у него завелись: калорийная — всегда,
            // кальциевая — с открытия недели кальция, остальные — никогда, потому
            // что экран целей был выключен. Планка — это и есть цель, и она одна.
            <div style="margin-bottom: 0.75rem;">
                {move || {
                    let fs = foods();
                    let es = entries();
                    // Планка ВЫБРАННОГО дня: прошлый день судится своей, а не
                    // сегодняшней — иначе недельный пересчёт перекрашивал бы прошлое.
                    let Some(target) = cal_planka_res.get().flatten().filter(|t| *t > 0.0) else {
                        return ().into_view();
                    };
                    let current = nutrient_sum("Calories", &es, &fs);
                    let pct = ((current / target) * 100.0).min(100.0);
                    // КАЛОРИИ судятся КОРИДОРОМ ±50 ккал — тем же, по которому их
                    // судит индикатор и полоса на дашборде: недобрал больше — серый
                    // (день не закрыт), попал — зелёный, перебрал больше — красный.
                    use crate::components::progress_widget::{calorie_bar_state, CalorieBar};
                    let (bar_color, text_color) = match calorie_bar_state(current, target) {
                        CalorieBar::Over => ("var(--bulma-danger)", "has-text-danger"),
                        CalorieBar::Hit => ("var(--bulma-success)", "has-text-success"),
                        CalorieBar::Under => ("var(--bulma-text-weak)", ""),
                    };
                    let name = crate::services::i18n::nutrient_name("Calories").to_string();
                    let unit = crate::services::i18n::unit_label("kcal");
                    view! {
                        <div style="margin-bottom: 0.5rem;">
                            <div style="display: flex; align-items: baseline; justify-content: space-between; margin-bottom: 0.15rem;">
                                <span class="is-size-7 has-text-grey">{name}</span>
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
                }}
            </div>

            // Weight & steps widgets moved to the dashboard (pages::dashboard).

            {move || if entries_data().is_none() || foods_data().is_none() {
                // Still loading the day / food list for the first time: render
                // nothing rather than the empty-day invitation (the centered green
                // «+»), which would otherwise flash before the entries arrive.
                // Sticky caches make this instant on any later navigation.
                ().into_view()
            } else if !editable() && entries().is_empty() {
                // Past day with no entries: a short message (no panels, no add). Today
                // always falls through to the panel view below, which renders the three
                // empty meal panels (each with its «+»).
                view! {
                    <div style="padding: 0 16px 5rem 16px;">
                        <div style="display: flex; flex-direction: column; align-items: center; justify-content: center; padding: 48px 8px 0 8px;">
                            <p style="font-size: 17px; color: var(--bulma-text-weak); margin: 0; text-align: center; line-height: 1.5;">
                                {move || format!("{} {}", format_date_past_prefix(&date.get()), t("diary.empty_past"))}
                            </p>
                        </div>
                    </div>
                }.into_view()
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
                          // `is_last` suppresses the row divider on the final row so
                          // dividers sit only BETWEEN entries, not after the last one.
                          let render_row = move |entry: DiaryEntry, is_last: bool| -> View {
                            // Ленивые формы записи рисуются своим компонентом и сюда
                            // не идут: обычная строка знает про еду, граммы, отходы,
                            // повтор и копирование, и ни одно из этих понятий к
                            // нераспознанной записи не применимо.
                            if entry.kind != api_types::DiaryEntryKind::Direct {
                                return view! {
                                    <crate::components::lazy_diary_row::LazyDiaryRow
                                        entry=entry.clone()
                                        foods=Signal::derive(foods)
                                        is_last=is_last
                                        on_edit=Callback::new(move |e: DiaryEntry| editing_lazy.set(Some(e)))
                                        on_delete=Callback::new(move |e: DiaryEntry| delete_entry(e.id.clone()))
                                        on_move=Callback::new(move |e: DiaryEntry| move_target.set(Some(e.id.clone())))
                                        on_duplicate=Callback::new(move |e: DiaryEntry| dup_target.set(Some(e.id.clone())))
                                    />
                                }.into_view();
                            }
                            let entry_id = entry.id.clone();
                            let entry_id2 = entry.id.clone();
                            let fid = entry.food_id.clone();
                            let fid2 = entry.food_id.clone();
                            let fid3 = entry.food_id.clone();
                            let fid4 = entry.food_id.clone();
                            let fid5 = entry.food_id.clone();
                            let g = entry.grams;
                            let w = entry.waste_grams;
                            // Repeating a row re-logs the food into the SAME meal it
                            // belongs to (its label, or derived from its time).
                            let meal_key = Some(crate::services::meal_split::meal_key_for(&entry).to_string());
                            view! {
                                    <div style=format!("display: flex; align-items: center; padding: 0.5rem 0;{}", if is_last { "" } else { " border-bottom: 1px solid var(--bulma-border-weak);" })>
                                        <div style="flex: 1; min-width: 0; overflow-wrap: break-word;">
                                            <span class="is-size-6 has-text-weight-medium"
                                                style=move || if foods().iter().any(|f| f.id == fid5 && f.is_restaurant) { crate::components::food_list_item::RESTAURANT_NAME_STYLE } else { "" }>
                                                {move || food_name(&fid)}
                                            </span>
                                            // Single-line КБЖУ: no wrap (Bulma `.tags`
                                            // wraps), tight gap; overflow hidden so a
                                            // rare extra (custom) badge clips instead
                                            // of dropping to a second line.
                                            // Пилюли КБЖУ — из общего места
                                            // (`components::food_badges`): те же самые
                                            // позиции человек видит в правке разобранной
                                            // записи, и выглядеть они обязаны одинаково.
                                            <div style=crate::components::food_badges::BADGE_ROW>
                                                {move || {
                                                    let fs = foods();
                                                    fs.iter().find(|f| f.id == fid2).map(|food| {
                                                        crate::components::food_badges::nutrient_badges(
                                                            food, (g - w).max(0.0) / 100.0)
                                                    })
                                                }}
                                            </div>
                                        </div>
                                        // Right side
                                        <div style="flex-shrink: 0; margin-left: 1rem; display: flex; align-items: center; gap: 0.75rem;">
                                            {move || {
                                                if editable() {
                                                    let eid = entry_id.clone();
                                                    let eid_t = entry_id.clone();
                                                    let eid_s = entry_id.clone();
                                                    let eid_d = entry_id.clone();
                                                    let eid_e = entry_id.clone();
                                                    let eid_del = entry_id.clone();
                                                    let eid_mv = entry_id.clone();
                                                    let fid_e = fid3.clone();
                                                    let fid_ed = fid3.clone();
                                                    // «Повторить сегодня» — то же действие, что было на
                                                    // прошлых днях отдельной кнопкой-стрелкой. Теперь оно
                                                    // просто пункт того же меню: кнопка вызова у всех дней
                                                    // одна и та же.
                                                    let fid_rep = fid3.clone();
                                                    let meal_rep = meal_key.clone();
                                                    let past_day = !is_today();
                                                    // `Copy`-замыкание: разметка строки перерисовывается,
                                                    // и `move ||` с захваченной строкой стал бы `FnOnce`.
                                                    let fid_dup = store_value(fid3.clone());
                                                    let already_today =
                                                        move || today_entries().iter().any(|e| e.food_id == fid_dup.get_value());
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
                                                                attr:data-testid="diary-row-menu"
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
                                                                            move |_| { dup_target.set(Some(id.clone())); menu_open.set(None); }
                                                                        }
                                                                    >{move || t("diary.duplicate")}</button>
                                                                    <button
                                                                        attr:data-testid="diary-menu-move"
                                                                        class="button is-ghost is-small is-fullwidth"
                                                                        style="justify-content: flex-start; text-decoration: none;"
                                                                        on:click={
                                                                            let id = eid_mv.clone();
                                                                            move |_| { move_target.set(Some(id.clone())); menu_open.set(None); }
                                                                        }
                                                                    >{move || t("diary.move")}</button>
                                                                    {past_day.then(|| view! {
                                                                        <button
                                                                            attr:data-testid="diary-menu-repeat"
                                                                            class="button is-ghost is-small is-fullwidth"
                                                                            style="justify-content: flex-start; text-decoration: none;"
                                                                            disabled=move || already_today()
                                                                            on:click={
                                                                                let fid = fid_rep.clone();
                                                                                let mk = meal_rep.clone();
                                                                                move |_| {
                                                                                    let fid = fid.clone();
                                                                                    let mk = mk.clone();
                                                                                    menu_open.set(None);
                                                                                    spawn_local(async move {
                                                                                        if let Some(food) = local::list_foods().await.into_iter().find(|f| f.id == fid) {
                                                                                            let _ = local::save_food_to_diary(
                                                                                                &food, g, w, food.is_restaurant, mk, Some(local::today()),
                                                                                            ).await;
                                                                                            invalidate();
                                                                                            sync::push_background();
                                                                                        }
                                                                                    });
                                                                                }
                                                                            }
                                                                        >{move || t("diary.repeat_today")}</button>
                                                                    })}
                                                                    <button
                                                                        attr:data-testid="diary-menu-edit"
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
                                                    // Per-render clone so the repeat on:click stays `Fn`.
                                                    let meal_key = meal_key.clone();
                                                    // `Copy`-замыкание: см. такое же выше, в открытом дне.
                                                    let fid_copied = store_value(fid_c);
                                                    let already_copied = move || {
                                                        today_entries().iter().any(|e| e.food_id == fid_copied.get_value())
                                                    };
                                                    view! {
                                                        <span class="is-size-7 has-text-grey">{move || format!("{:.0}{}", g, t("common.unit.g"))}</span>
                                                        <div style="position: relative;">
                                                            // Кнопка вызова меню — ТА ЖЕ, что у сегодняшнего дня.
                                                            // Раньше здесь была стрелка-повтор, и строка прошлого
                                                            // дня выглядела другим элементом интерфейса, хотя
                                                            // делает то же: открывает меню действий. Отличается
                                                            // теперь только список внутри.
                                                            <button
                                                                attr:data-testid="diary-row-menu"
                                                                class="button is-ghost has-text-grey-light"
                                                                style="height: 2.5rem; width: 2.5rem; padding: 0; text-decoration: none;"
                                                                on:click=move |_| {
                                                                    haptic(15);
                                                                    menu_open.update(|m| {
                                                                        if m.as_deref() == Some(&eid) { *m = None; }
                                                                        else { *m = Some(eid.clone()); }
                                                                    });
                                                                }
                                                            >
                                                                <svg xmlns="http://www.w3.org/2000/svg" width="20" height="20" viewBox="0 0 20 20" fill="currentColor">
                                                                    <circle cx="10" cy="4" r="1.6"/>
                                                                    <circle cx="10" cy="10" r="1.6"/>
                                                                    <circle cx="10" cy="16" r="1.6"/>
                                                                </svg>
                                                            </button>
                                                            <Show when=move || menu_open.get().as_deref() == Some(&eid2)>
                                                                <div style="position: absolute; right: 0; top: 100%; z-index: 10; background: var(--bulma-scheme-main); border-radius: 6px; box-shadow: 0 2px 12px rgba(0,0,0,0.15); min-width: 10rem; padding: 0.25rem 0;">
                                                                    <button
                                                                        attr:data-testid="diary-menu-repeat"
                                                                        class="button is-ghost is-small is-fullwidth"
                                                                        style="justify-content: flex-start; text-decoration: none;"
                                                                        disabled=move || already_copied()
                                                                        on:click={
                                                                            let fid = fid_r.clone();
                                                                            let mk = meal_key.clone();
                                                                            move |_| {
                                                                            let fid = fid.clone();
                                                                            // Clone before the `async move` so the on:click stays `Fn`.
                                                                            let mk = mk.clone();
                                                                            menu_open.set(None);
                                                                            spawn_local(async move {
                                                                                if let Some(food) = local::list_foods().await.into_iter().find(|f| f.id == fid) {
                                                                                    let _ = local::save_food_to_diary(
                                                                                        &food, g, w, food.is_restaurant, mk, Some(local::today()),
                                                                                    ).await;
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

                          // Three explicit meal panels (breakfast / lunch / dinner),
                          // in order. Today they always show (empty → header + «+»);
                          // past days show only meals that have entries. Each entry is
                          // placed by its `meal_label` (or derived from its time).
                          use crate::services::meal_split::{meal_key_for, MAIN_MEALS};
                          use crate::components::meal_panel::MealPanel;
                          let fs = foods();
                          let es = entries();
                          let today = editable();
                          MAIN_MEALS.iter().filter_map(|meal| {
                              let meal_entries: Vec<DiaryEntry> =
                                  es.iter().filter(|e| meal_key_for(e) == meal.key).cloned().collect();
                              if !today && meal_entries.is_empty() {
                                  return None;
                              }
                              let title = t(meal.i18n_key).to_string();
                              let accent = meal.accent.to_string();
                              let kcal = nutrient_sum("Calories", &meal_entries, &fs);
                              let protein = nutrient_sum("Protein", &meal_entries, &fs);
                              let fat = nutrient_sum("Fat", &meal_entries, &fs);
                              let carbs = nutrient_sum("Carbs", &meal_entries, &fs);
                              let is_empty = meal_entries.is_empty();
                              let n = meal_entries.len();
                              let rows = meal_entries.into_iter().enumerate()
                                  .map(|(i, e)| render_row(e, i + 1 == n)).collect::<Vec<_>>();
                              Some(view! {
                                  <MealPanel title=title accent=accent meal_key=meal.key.to_string()
                                      can_add=today is_empty=is_empty
                                      on_date=(!is_today()).then(|| date.get())
                                      kcal=kcal protein=protein fat=fat carbs=carbs>
                                      {rows}
                                  </MealPanel>
                              }.into_view())
                          }).collect::<Vec<_>>()
                        }}
                    </div>

                    // (The floating «+» FAB was removed: adding now happens from each
                    // meal panel's header / «+».)
                }.into_view()
            }}
        // Close the page container. Dialogs below stay SIBLINGS (not nested) so
        // their z-index (50) sits in the root stacking context and beats the nav
        // bar (z-40) — kept as-is from when the shell was position:fixed.
        </div>

            {move || {
                editing.get().map(|(entry_id, food, current_grams, current_waste, current_restaurant)| {
                    view! {
                        <FoodWeightModal
                            food=food
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

            // «Перенести»: то же окно выбора приёма, что и у дублирования, только
            // запись не копируется, а меняет приём. День не трогаем — для другого
            // числа есть «повторить сегодня».
            {move || {
                move_target.get().map(|eid| {
                    let meal_btns = crate::services::meal_split::MAIN_MEALS.iter().map(|m| {
                        let eid = eid.clone();
                        let key: &'static str = m.key;
                        let accent: &'static str = m.accent;
                        let i18n_key: &'static str = m.i18n_key;
                        view! {
                            <button class="button is-fullwidth"
                                attr:data-testid="diary-move-meal"
                                style=format!("justify-content: flex-start; margin-bottom: 8px; height: 3rem; \
                                    border: 1px solid {accent}; color: {accent}; background: {accent}22; font-weight: 600;")
                                on:click=move |_| {
                                    let eid = eid.clone();
                                    move_target.set(None);
                                    spawn_local(async move {
                                        local::move_diary_entry(&eid, Some(key.to_string())).await;
                                        invalidate();
                                        sync::push_background();
                                    });
                                }
                            >{move || t(i18n_key)}</button>
                        }
                    }).collect_view();
                    view! {
                        <div class="modal is-active" attr:data-testid="diary-move-sheet">
                            <div class="modal-background" on:click=move |_| move_target.set(None)></div>
                            <div class="modal-card" style="max-width: 22rem; width: calc(100% - 2rem);">
                                <section class="modal-card-body" style="border-radius: 12px;">
                                    <div class="is-size-6 has-text-weight-bold" style="margin-bottom: 14px;">
                                        {move || t("diary.move_to")}
                                    </div>
                                    {meal_btns}
                                    <button class="button is-ghost is-fullwidth" style="margin-top: 4px;"
                                        on:click=move |_| move_target.set(None)>{move || t("common.cancel")}</button>
                                </section>
                            </div>
                        </div>
                    }
                })
            }}

            // "Дублировать": bottom-sheet to pick which meal the copy goes into.
            {move || {
                dup_target.get().map(|eid| {
                    let meal_btns = crate::services::meal_split::MAIN_MEALS.iter().map(|m| {
                        let eid = eid.clone();
                        let key: &'static str = m.key;
                        let accent: &'static str = m.accent;
                        let i18n_key: &'static str = m.i18n_key;
                        view! {
                            <button class="button is-fullwidth"
                                style=format!("justify-content: flex-start; margin-bottom: 8px; height: 3rem; \
                                    border: 1px solid {accent}; color: {accent}; background: {accent}22; font-weight: 600;")
                                on:click=move |_| {
                                    let eid = eid.clone();
                                    dup_target.set(None);
                                    spawn_local(async move {
                                        local::duplicate_diary_entry(&eid, Some(key.to_string()), None).await;
                                        invalidate();
                                        sync::push_background();
                                        // Копия НЕРАСПОЗНАННОЙ записи так и осталась
                                        // нераспознанной — её надо поставить в очередь,
                                        // иначе она провисит так навсегда. Разобранной
                                        // копии проход ничего не сделает: ответ у неё
                                        // уже есть.
                                        crate::services::lazy_food::run_queue_background();
                                    });
                                }
                            >{move || t(i18n_key)}</button>
                        }
                    }).collect_view();
                    view! {
                        // Bulma modal — centred, viewport-fixed, matching the app's
                        // other dialogs (a bare position:fixed misplaces inside the shell).
                        <div class="modal is-active">
                            <div class="modal-background" on:click=move |_| dup_target.set(None)></div>
                            <div class="modal-card" style="max-width: 22rem; width: calc(100% - 2rem);">
                                <section class="modal-card-body" style="border-radius: 12px;">
                                    <div class="is-size-6 has-text-weight-bold" style="margin-bottom: 14px;">
                                        {move || t("diary.duplicate_to")}
                                    </div>
                                    {meal_btns}
                                    <button class="button is-ghost is-fullwidth" style="margin-top: 4px;"
                                        on:click=move |_| dup_target.set(None)>{move || t("common.cancel")}</button>
                                </section>
                            </div>
                        </div>
                    }
                })
            }}
    }
}
