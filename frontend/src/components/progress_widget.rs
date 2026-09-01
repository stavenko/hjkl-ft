//! Dashboard "progress" widget: appears once the persona is set (alongside the
//! notifications bell). It nudges the user to log a full week of food / weight /
//! steps, shows «X/7» counters, and — once all three reach 7 — offers a button
//! that runs the same first-planka algorithm the story used
//! (`local::calorie_planka_suggestion` → `local::set_calorie_goal`).

use std::cell::RefCell;

use leptos::*;
use leptos_router::use_navigate;

use crate::services::i18n::t;
use crate::services::indicators::{self, IndicatorState};
use crate::services::profile::{self, CourseGoal};
use crate::services::sticky::sticky;
use crate::services::{db, local, sync};

// Process-lifetime caches so re-navigating to the dashboard paints the widget's
// real state on the first frame instead of the 0/7 / "add food" placeholder that
// then snaps to the loaded state (see `services::sticky`).
thread_local! {
    static PLANKA_CACHE: RefCell<Option<Option<f64>>> = const { RefCell::new(None) };
    static HASFOOD_CACHE: RefCell<Option<bool>> = const { RefCell::new(None) };
    static COUNTS_CACHE: RefCell<Option<(u32, u32, u32)>> = const { RefCell::new(None) };
    static INDS_CACHE: RefCell<Option<Vec<(&'static str, IndicatorState)>>> = const { RefCell::new(None) };
    static GAUGES_CACHE: RefCell<Option<Vec<indicators::DailyGauge>>> = const { RefCell::new(None) };
    static GATE_CACHE: RefCell<Option<u32>> = const { RefCell::new(None) };
    static STEPS_GATE_CACHE: RefCell<Option<u32>> = const { RefCell::new(None) };
    static CALCIUM_GATE_CACHE: RefCell<Option<u32>> = const { RefCell::new(None) };
}

const CARD: &str = "background: var(--bulma-scheme-main); border-radius: 16px; \
    padding: 16px; box-sizing: border-box; \
    display: flex; flex-direction: column; gap: 12px;";

/// Длина недели наблюдений в днях — окно, за которое письмо о первой планке
/// считает калорийность и изменение веса. То же окно, что у самой планки
/// (`calorie_planka_suggestion`).
const OBSERVATION_DAYS: i64 = 7;

/// Насколько вес должен измениться, чтобы об этом стоило говорить, кг.
///
/// Бытовые весы и суточные колебания воды дают до полукилограмма разницы на ровном
/// месте. Ниже этого порога честный ответ — «практически не изменился», а не
/// «вырос»: человек сверит письмо со своими весами, и приписанный рост подорвёт
/// доверие ко всему остальному в нём.
const WEIGHT_NOISE_KG: f64 = 0.3;

/// Письмо о первой планке.
///
/// Числа собираются заново, а не берутся из виджета: письмо переживёт человека,
/// закрывшего приложение, и должно говорить то же, что и расчёт.
async fn first_planka_letter(planka_kcal: f64) -> String {
    let mut body = String::from(
        "Неделя наблюдений завершена, вы хорошо поработали. Теперь мы знаем примерную \
         калорийность вашей еды.",
    );
    if let Some((avg, max, min)) = local::daily_kcal_stats(OBSERVATION_DAYS).await {
        body.push_str(&format!(
            " Средняя калорийность составила {avg:.0} ккал. Максимальная калорийность: \
             {max:.0} ккал, минимальная калорийность была {min:.0} ккал.",
        ));
    }

    // Про вес — только когда есть с чем сравнивать: пары замеров за неделю может и
    // не быть, и выдумывать направление в этом случае нечем.
    if let Some(delta) = local::weight_change_over(OBSERVATION_DAYS).await {
        let phrase = if delta > WEIGHT_NOISE_KG {
            "ваш вес вырос"
        } else if delta < -WEIGHT_NOISE_KG {
            "ваш вес снизился"
        } else {
            "ваш вес практически не изменился"
        };
        body.push_str(&format!("\n\nПо результатам недели {phrase}."));
    }

    body.push_str(&format!(
        "\n\nЭто означает, что мы должны назначить вам первую планку по калориям в размере: \
         {planka_kcal:.0} ккал.",
    ));

    // Планка по белку считается от калорийной, поэтому спрашивается ПОСЛЕ её
    // установки. Ноль значит незаполненный профиль (рост, возраст, пол) — тогда
    // строки про белок в письме просто нет, обещать нечего.
    let weight_kg = local::list_weight_entries().await.last().map(|e| e.weight_kg);
    if let Some(w) = weight_kg {
        let protein = profile::protein_target_from_profile(w).await;
        if protein > 0 {
            body.push_str(&format!("\n\nТакже мы выдаём вам планку по белку: {protein} г."));
        }
    }

    body.push_str(
        "\n\nЧерез неделю посмотрим, какие у вас результаты, и, если потребуется, сделаем \
         перерасчёт планок.",
    );
    body
}

// ── Nutrition indicators ─────────────────────────────────────────────────────
// Seven line icons (Lucide, inlined — same line style as the nav) showing how the
// user's food/drink is doing, coloured green / orange / red (grey = no data yet) by
// `services::indicators`. Shown only once there's ≥1 week of diary history.
const IC_BONE: &str = r#"<path d="M17 10c.7-.7 1.69 0 2.5 0a2.5 2.5 0 1 0 0-5 .5.5 0 0 1-.5-.5 2.5 2.5 0 1 0-5 0c0 .81.7 1.8 0 2.5l-7 7c-.7.7-1.69 0-2.5 0a2.5 2.5 0 0 0 0 5c.28 0 .5.22.5.5a2.5 2.5 0 1 0 5 0c0-.81-.7-1.8 0-2.5Z"/>"#;
const IC_FISH: &str = r#"<path d="M6.5 12c.94-3.46 4.94-6 8.5-6 3.56 0 6.06 2.54 7 6-.94 3.47-3.44 6-7 6s-7.56-2.53-8.5-6Z"/><path d="M18 12v.5"/><path d="M16 17.93a9.77 9.77 0 0 1 0-11.86"/><path d="M7 10.67C7 8 5.58 5.97 2.73 5.5c-1 1.5-1 5 .23 6.5-1.24 1.5-1.24 5-.23 6.5C5.58 18.03 7 16 7 13.33"/><path d="M10.46 7.26C10.2 5.88 9.17 4.24 8 3h5.8a2 2 0 0 1 1.98 1.67l.23 1.4"/><path d="m16.01 17.93-.23 1.4A2 2 0 0 1 13.8 21H9.5a5.96 5.96 0 0 0 1.49-3.98"/>"#;
const IC_EGG: &str = r#"<path d="M12 2C8 2 4 8 4 14a8 8 0 0 0 16 0c0-6-4-12-8-12"/>"#;
const IC_DROPLET: &str = r#"<path d="M12 22a7 7 0 0 0 7-7c0-2-1-3.9-3-5.5s-3.5-4-4-6.5c-.5 2.5-2 4.9-4 6.5C6 11.1 5 13 5 15a7 7 0 0 0 7 7z"/>"#;
const IC_HAM: &str = r#"<path d="M13.144 21.144A7.274 10.445 45 1 0 2.856 10.856"/><path d="M13.144 21.144A7.274 4.365 45 0 0 2.856 10.856a7.274 4.365 45 0 0 10.288 10.288"/><path d="M16.565 10.435 18.6 8.4a2.501 2.501 0 1 0 1.65-4.65 2.5 2.5 0 1 0-4.66 1.66l-2.024 2.025"/><path d="m8.5 16.5-1-1"/>"#;
const IC_APPLE: &str = r#"<path d="M12 6.528V3a1 1 0 0 1 1-1"/><path d="M18.237 21A15 15 0 0 0 22 11a6 6 0 0 0-10-4.472A6 6 0 0 0 2 11a15.1 15.1 0 0 0 3.763 10 3 3 0 0 0 3.648.648 5.5 5.5 0 0 1 5.178 0A3 3 0 0 0 18.237 21"/>"#;
const IC_FLAME: &str = r#"<path d="M8.5 14.5A2.5 2.5 0 0 0 11 12c0-1.38-.5-2-1-3-1.072-2.143-.224-4.054 2-6 .5 2.5 2 4.9 4 6.5 2 1.6 3 3.5 3 5.5a7 7 0 1 1-14 0c0-1.153.433-2.294 1-3a2.5 2.5 0 0 0 2.5 2.5z"/>"#;
const IC_WHEAT: &str = r#"<path d="M2 22 16 8"/><path d="M3.47 12.53 5 11l1.53 1.53a3.5 3.5 0 0 1 0 4.94L5 19l-1.53-1.53a3.5 3.5 0 0 1 0-4.94Z"/><path d="M7.47 8.53 9 7l1.53 1.53a3.5 3.5 0 0 1 0 4.94L9 15l-1.53-1.53a3.5 3.5 0 0 1 0-4.94Z"/><path d="M11.47 4.53 13 3l1.53 1.53a3.5 3.5 0 0 1 0 4.94L13 11l-1.53-1.53a3.5 3.5 0 0 1 0-4.94Z"/><path d="M20 2h2v2a4 4 0 0 1-4 4h-2V6a4 4 0 0 1 4-4Z"/><path d="M11.47 17.47 13 19l-1.53 1.53a3.5 3.5 0 0 1-4.94 0L5 19l1.53-1.53a3.5 3.5 0 0 1 4.94 0Z"/><path d="M15.47 13.47 17 15l-1.53 1.53a3.5 3.5 0 0 1-4.94 0L9 15l1.53-1.53a3.5 3.5 0 0 1 4.94 0Z"/><path d="M19.47 9.47 21 11l-1.53 1.53a3.5 3.5 0 0 1-4.94 0L13 11l1.53-1.53a3.5 3.5 0 0 1 4.94 0Z"/>"#;
// Lucide "beef" — the protein indicator.
const IC_BEEF: &str = r#"<circle cx="12.5" cy="8.5" r="2.5"/><path d="M12.5 2a6.5 6.5 0 0 0-6.22 4.6c-1.1 3.13-.78 3.9-3.18 6.08A3 3 0 0 0 5 18c4 0 8.4-1.8 11.4-4.3A6.5 6.5 0 0 0 12.5 2Z"/><path d="m18.5 6 2.19 4.5a6.48 6.48 0 0 1 .31 2 6.49 6.49 0 0 1-2.6 5.2C15.4 20.2 11 22 7 22a3 3 0 0 1-2.68-1.66L2.4 16.5"/>"#;
// Lucide "footprints" — the steps (activity) indicator.
const IC_STEPS: &str = r#"<path d="M4 16v-2.38C4 11.5 2.97 10.5 3 8c.03-2.72 1.49-6 4.5-6C9.37 2 10 3.8 10 5.5c0 3.11-2 5.66-2 8.68V16a2 2 0 1 1-4 0Z"/><path d="M20 20v-2.38c0-2.12 1.03-3.12 1-5.62-.03-2.72-1.49-6-4.5-6C14.63 6 14 7.8 14 9.5c0 3.11 2 5.66 2 8.68V20a2 2 0 1 0 4 0Z"/><path d="M16 17h4"/><path d="M4 13h4"/>"#;
/// Капля масла — качество жира: отношение (МНЖК+ПНЖК)/НЖК.
const IC_OIL: &str = r#"<path d="M12 2v6"/><path d="M9 5h6"/><path d="M12 8c-3 3-5 5.5-5 8a5 5 0 0 0 10 0c0-2.5-2-5-5-8z"/>"#;
/// Кусок мяса с прожилкой — красное мясо. Нарисован здесь, а не взят из набора:
/// готовые мясные значки там уже заняты белком и гемом.
const IC_STEAK: &str = r#"<path d="M4 10a6 6 0 0 1 6-6h4a6 6 0 0 1 6 6v1a9 9 0 0 1-9 9h-1a6 6 0 0 1-6-6z"/><path d="M9 8c1.8 1.2 2.6 3 2.4 5.4"/>"#;
/// Сосиска — мясо глубокой переработки. Из набора gastronomy: контур там уже
/// превращён в залитую фигуру, поэтому значок рисуется заливкой (см. `glyph`).
const IC_SAUSAGE: &str = r##"<path d="M115.7,94.39l-8.421.9a15.907,15.907,0,0,0-9.026-11.734C73.31,72.1,55.616,54.406,44.164,29.459a15.811,15.811,0,0,0-10.626-8.8l.982-9.278a5.03,5.03,0,0,0-8.5-4.141L23.6,9.585a9.163,9.163,0,0,1-7.926,2.45l-3.328-.57a5.027,5.027,0,0,0-4.68,8.209l7.855,9.238a15.886,15.886,0,0,0-.285,13.828,141.627,141.627,0,0,0,28.44,41.3,141.612,141.612,0,0,0,41.3,28.439,15.815,15.815,0,0,0,12.5.314l9.938,8.451a5.031,5.031,0,0,0,8.208-4.69l-.57-3.321a9.192,9.192,0,0,1,2.453-7.927l2.339-2.419A5.028,5.028,0,0,0,115.7,94.39ZM10.333,17.407a1.525,1.525,0,0,1,1.422-2.492l3.328.569A12.673,12.673,0,0,0,26.037,12.1l2.42-2.341a1.53,1.53,0,0,1,2.583,1.259L30.067,20.2a15.767,15.767,0,0,0-12.535,5.675Zm76.1,91.891A138.145,138.145,0,0,1,46.151,81.565,138.144,138.144,0,0,1,18.419,41.28,12.414,12.414,0,1,1,40.982,30.92c.614,1.336,1.255,2.639,1.9,3.934l-7.332,4.218A1.75,1.75,0,0,0,37.3,42.106l7.207-4.146a114.309,114.309,0,0,0,10.317,16l-7.657,6.4a1.75,1.75,0,0,0,2.244,2.686l7.583-6.335a104.865,104.865,0,0,0,14,14l-6.537,7.833a1.75,1.75,0,0,0,2.687,2.243l6.6-7.906A114.128,114.128,0,0,0,89.737,83.2L85.6,90.4a1.75,1.75,0,0,0,3.035,1.744l4.212-7.326c1.3.651,2.612,1.3,3.954,1.912A12.414,12.414,0,0,1,86.437,109.3Zm30.895-8.844-2.34,2.419a12.71,12.71,0,0,0-3.387,10.953l.571,3.321a1.529,1.529,0,0,1-2.492,1.43l-8.9-7.567A15.8,15.8,0,0,0,107.5,98.783l8.57-.913A1.528,1.528,0,0,1,117.332,100.454Z"/>"##;

/// (stroke, tint background) for an indicator state.
pub fn state_colors(s: IndicatorState) -> (&'static str, &'static str) {
    match s {
        IndicatorState::Green => ("#1fa463", "rgba(31,164,99,0.15)"),
        IndicatorState::Orange => ("#e8850d", "rgba(232,133,13,0.15)"),
        IndicatorState::Red => ("#e0304f", "rgba(224,48,79,0.15)"),
        IndicatorState::Unknown => ("#9aa0a6", "rgba(154,160,166,0.14)"),
    }
}

/// Значок индикатора: фигуры, подпись и то, КАК их рисовать.
#[derive(Clone, Copy)]
pub struct Icon {
    pub paths: &'static str,
    pub label: &'static str,
    /// Своя система координат у каждого набора: у Lucide 24, у gastronomy 128.
    pub view_box: &'static str,
    /// `true` — фигура ЗАЛИТАЯ (контур уже превращён в форму), `false` — линия,
    /// которую надо обвести.
    pub filled: bool,
}

/// Значок-линия из Lucide — тем же способом, что рисуется вся навигация.
const fn stroked(paths: &'static str, label: &'static str) -> Icon {
    Icon { paths, label, view_box: "0 0 24 24", filled: false }
}

/// Значок из набора gastronomy: тот же контурный вид, но фигура залитая.
const fn glyph(paths: &'static str, label: &'static str) -> Icon {
    Icon { paths, label, view_box: "0 0 128 128", filled: true }
}

/// Значок и подпись для ключа индикатора.
pub fn icon_for(k: &str) -> Icon {
    match k {
        "calories" => stroked(IC_FLAME, "Калории"),
        "protein" => stroked(IC_BEEF, "Белок"),
        "calcium" => stroked(IC_BONE, "Кальций"),
        // Два жировых: EPA+DHA — рыба (единственный их источник), баланс — капля масла.
        "epa_dha" => stroked(IC_FISH, "Омега-3"),
        "fat_ratio" => stroked(IC_OIL, "Баланс"),
        "iron" => stroked(IC_DROPLET, "Железо"),
        // Не капля, как у железа: два одинаковых значка рядом читаются как один
        // индикатор, продублированный по ошибке. Гем — про сам продукт, отсюда мясо.
        "heme" => stroked(IC_HAM, "Гем"),
        // Мясные ограничения. Гем рядом рисуется окороком, поэтому этим двум нужны
        // СВОИ силуэты: три похожих мясных значка в ряд не различить.
        "red_meat" => stroked(IC_STEAK, "Кр. мясо"),
        "processed_meat" => glyph(IC_SAUSAGE, "Колбасы"),
        // Яйцо — свой силуэт, значок уже есть в наборе.
        "egg" => stroked(IC_EGG, "Яйца"),
        "veg_fruit" => stroked(IC_APPLE, "Фр/овощи"),
        "steps" => stroked(IC_STEPS, "Шаги"),
        "fiber" => stroked(IC_WHEAT, "Клетчатка"),
        // No silent fallback: an unmapped key is a bug (e.g. a new indicator added
        // without an icon), so fail loudly instead of mislabeling it as fiber.
        _ => panic!("icon_for: no icon/label for indicator key {k:?}"),
    }
}

fn indicator(icon: Icon, state: IndicatorState) -> impl IntoView {
    let (color, tint) = state_colors(state);
    let label = icon.label;
    // Два разных способа нарисовать одну и ту же по виду линию. Наши значки —
    // ОБВОДКА пути (Lucide, 24×24, толщина 2). Значки из набора gastronomy —
    // ЗАЛИВКА контура (128×128): линия там уже превращена в замкнутую фигуру, и
    // обводить её нельзя, иначе получится двойной контур. Отсюда и развилка.
    let (fill, stroke, width) = if icon.filled {
        (color, "none", "0")
    } else {
        ("none", color, "2")
    };
    view! {
        <div attr:data-ind=label style="display: flex; flex-direction: column; align-items: center; gap: 3px; flex: 1; min-width: 0;">
            <div style=format!("width: 38px; height: 38px; border-radius: 50%; background: {tint}; \
                    display: flex; align-items: center; justify-content: center;")>
                <svg xmlns="http://www.w3.org/2000/svg" width="21" height="21" viewBox=icon.view_box
                    fill=fill stroke=stroke stroke-width=width
                    stroke-linecap="round" stroke-linejoin="round"
                    inner_html=icon.paths></svg>
            </div>
            <span style="font-size: 0.55rem; line-height: 1.1; text-align: center; color: var(--bulma-text-weak);">{label}</span>
        </div>
    }
}

/// ОДИН ряд, не больше семи значков — сколько влезает по ширине телефона.
///
/// Индикаторов больше, чем мест, и это не проблема, а условие задачи: показывать надо
/// не все подряд, а те, что требуют внимания. Порядок задан сортировкой у вызывающего:
/// сначала красные, потом оранжевые, дальше зелёные. Если краснеть нечему, ряд просто
/// заполняется зелёными — и как только что-то испортится, оно тут же встанет первым.
///
/// Перенос на вторую строку пробовался и оказался хуже: ряд разъезжался на две
/// половины, из которых вторая ничего не значила.
fn indicators_row(states: Vec<(&'static str, IndicatorState)>) -> impl IntoView {
    view! {
        <div style="display: flex; gap: 4px; justify-content: space-between;">
            {states.into_iter().map(|(k, st)| {
                indicator(icon_for(k), st)
            }).collect_view()}
        </div>
    }
}

/// Short label under a daily gauge.
fn gauge_label(key: &str) -> &'static str {
    match key {
        "protein" => "Белок",
        "veg_fruit" => "Фр/овощи",
        "steps" => "Шаги",
        "calcium" => "Кальций",
        "fiber" => "Клетчатка",
        _ => panic!("gauge_label: no label for gauge key {key:?}"),
    }
}

/// Grid of daily-nutrient bars (protein, veg/fruit, calcium, iron, fiber), two
/// per row so they fit vertically (calories stay full-width above). Each fills
/// toward its per-day target; the bar is the indicator's colour, or grey while
/// the metric has no data yet.
fn daily_gauges_grid(
    gauges: Vec<indicators::DailyGauge>,
    iron: Option<crate::services::iron::WeeklyIron>,
    heme: Option<crate::services::heme::WeeklyHeme>,
    fats: Option<crate::services::fats::WeeklyFats>,
    red_meat: Option<crate::services::red_meat::WeeklyRedMeat>,
    egg: Option<crate::services::egg::WeeklyEggs>,
) -> impl IntoView {
    // ДНЕВНЫЕ — В ОДНУ СТРОКУ. Их всего три (белок, фрукты с овощами, кальций), и
    // они про один и тот же сегодняшний день: строка держит их вместе и отделяет
    // от недельных, которые идут ниже парами. Колонок ровно столько, сколько шкал —
    // пока кальций не открыт, их две, и пустого места справа не остаётся.
    let daily_cols = gauges.len().max(1);
    view! {
        <div style=format!("display: grid; grid-template-columns: repeat({daily_cols}, 1fr); \
                            gap: 12px 14px; margin-bottom: 12px;")>
            {gauges.into_iter().map(|g| {
                // At-least goals: neutral until met, green when met (bar + value).
                let (bar, val) = crate::components::gauge::at_least_colors(g.value, g.target);
                view! {
                    <crate::components::gauge::Gauge
                        value=g.value target=g.target
                        label=gauge_label(g.key).to_string()
                        unit=g.unit.to_string()
                        color=bar.to_string()
                        // Числа внутри полосы: втроём в ряд они иначе не помещаются.
                        value_inside=true
                        value_color=val.map(String::from)/>
                }
            }).collect_view()}
        </div>
        <div style="display: grid; grid-template-columns: repeat(2, 1fr); gap: 12px 14px;">
            // Недельное железо — первой ячейкой второй сетки.
            {iron.map(weekly_iron_gauge)}
            // Порции гемовых продуктов — следующей ячейкой, рядом с железом:
            // это две стороны одного разговора, и врозь они читаются хуже.
            {heme.map(weekly_heme_gauge)}
            // Жиры — три ячейки подряд, тоже вместе: «сколько морских омега-3»,
            // «сколько растительных» и «каков жир в целом».
            {fats.map(weekly_fat_gauges)}
            // Красное мясо — единственная шкала-ОГРАНИЧЕНИЕ: полная означает не
            // достижение, а перебор.
            {red_meat.map(weekly_red_meat_gauge)}
            // Яйца — снова шкала-ЦЕЛЬ: семь штук за неделю набирают, а не удерживают
            // под ними. Стоит следом за мясом, потому что и открывается следом.
            {egg.map(weekly_egg_gauge)}
        </div>
    }
}

/// Недельные яйца — ячейка той же сетки. Планка здесь МИНИМУМ: полная полоса
/// означает набранную неделю, а не перебор, — поэтому цвет берётся из самой недели,
/// которая знает и про темп: одно яйцо к пятнице значит отставание, а к вторнику
/// нет.
///
/// ГРАММЫ: счёт идёт в них, по 50 г на яйцо (`egg::GRAMS_PER_EGG`), и шкала
/// показывает то же, что считает. Штуки остаются в тексте задания — «наберите за
/// неделю семь яиц», — но мерить дробные штуки на шкале не за чем.
fn weekly_egg_gauge(w: crate::services::egg::WeeklyEggs) -> impl IntoView {
    use indicators::IndicatorState;
    let (bar, val) = match w.state() {
        IndicatorState::Orange => ("#f5a524", Some("#f5a524")),
        _ => ("#20c997", None),
    };
    let pace = crate::components::gauge::GaugePace {
        segments: 7,
        passed: w.day_of_week.saturating_sub(1),
        at_most: false,
    };
    view! {
        <crate::components::gauge::Gauge
            value=w.grams target=w.target
            label="Яйца/нед".to_string()
            unit="г".to_string()
            color=bar.to_string()
            height=12.0
            decimals=1
            pace=Some(pace)
            value_color=val.map(String::from)/>
    }
}

/// Недельный gauge по железу — ЯЧЕЙКА той же сетки, что и дневные полосы, поэтому
/// он встаёт напротив кальция и занимает ровно половину ширины.
///
/// Шесть точек делят полосу на семь суточных отрезков; горят те, чьи дни уже
/// прошли. День 3 → два дня позади → две горящие точки, и норма «на сейчас» —
/// 2/7 недельной. На первом дне не горит ничего: должок ещё не набежал. Номер дня
/// отдельной подписью не выводим — его показывают сами точки.
fn weekly_iron_gauge(w: crate::services::iron::WeeklyIron) -> impl IntoView {
    let (bar, val) = crate::components::gauge::at_least_colors(w.absorbed_mg, w.target_mg);
    let pace = crate::components::gauge::GaugePace {
        segments: 7,
        passed: w.day_of_week.saturating_sub(1),
        at_most: false,
    };
    view! {
        <crate::components::gauge::Gauge
            value=w.absorbed_mg target=w.target_mg
            label="Железо/нед".to_string()
            unit="мг".to_string()
            color=bar.to_string()
            height=12.0
            decimals=1
            pace=Some(pace)
            value_color=val.map(String::from)/>
    }
}

/// Недельные порции печени, красного мяса и моллюсков — ячейка той же сетки.
///
/// Значение дробное намеренно: порция считается по белку, а не по числу приёмов,
/// и «2,08 из 3» честнее округлённого «2» — иначе непонятно, что кусок поменьше
/// засчитался не полностью.
fn weekly_heme_gauge(w: crate::services::heme::WeeklyHeme) -> impl IntoView {
    let (bar, val) = crate::components::gauge::at_least_colors(w.portions, w.target);
    let pace = crate::components::gauge::GaugePace {
        segments: 7,
        passed: w.day_of_week.saturating_sub(1),
        at_most: false,
    };
    view! {
        <crate::components::gauge::Gauge
            value=w.portions target=w.target
            label="Гем/нед".to_string()
            // Без единицы: «2.08 / 3.00» и так читается как счёт порций, а «порц.»
            // рядом с дробью только загромождает подпись.
            unit=String::new()
            color=bar.to_string()
            height=12.0
            decimals=2
            pace=Some(pace)
            value_color=val.map(String::from)/>
    }
}

/// Недельная шкала красного мяса — ячейка той же сетки, но с ОБРАТНЫМ смыслом.
///
/// У всех соседних шкал полная полоса значит «получилось». Здесь наоборот: полная
/// значит, что планка выбрана и дальше идёт перебор. Поэтому цвет берётся не из
/// `at_least_colors`, а из самой недели — она знает и про темп: 350 г к третьему
/// дню тревожны, а те же 350 г к воскресенью — нет.
///
/// Точки, как и у соседей, делят неделю на семь отрезков — здесь они показывают
/// ровно тот темп, с которым планки хватит до конца недели.
fn weekly_red_meat_gauge(w: crate::services::red_meat::WeeklyRedMeat) -> impl IntoView {
    use indicators::IndicatorState;
    let (bar, val) = match w.state() {
        IndicatorState::Red => ("#e0304f", Some("#e0304f")),
        IndicatorState::Orange => ("#f5a524", Some("#f5a524")),
        _ => ("#20c997", None),
    };
    // ПОТОЛОК, а не цель: пустая шкала здесь — лучший исход, и обводка обязана быть
    // зелёной, пока человек идёт медленнее равномерного темпа.
    let pace = crate::components::gauge::GaugePace {
        segments: 7,
        passed: w.day_of_week.saturating_sub(1),
        at_most: true,
    };
    view! {
        <crate::components::gauge::Gauge
            value=w.grams target=w.limit
            label="Кр. мясо/нед".to_string()
            unit="г".to_string()
            color=bar.to_string()
            height=12.0
            decimals=0
            pace=Some(pace)
            value_color=val.map(String::from)/>
    }
}

/// Три недельные шкалы по жирам — ячейки той же сетки.
///
/// Отношение (МНЖК+ПНЖК)/НЖК рисуется шкалой «не меньше двух»: значение — само
/// отношение, цель — двойка. Пока насыщенных не съедено, отношения нет — шкала
/// показывает ноль, а не «бесконечно хорошо».
fn weekly_fat_gauges(w: crate::services::fats::WeeklyFats) -> impl IntoView {
    let pace = crate::components::gauge::GaugePace {
        segments: 7,
        passed: w.day_of_week.saturating_sub(1),
        at_most: false,
    };
    // Точки стоят у обеих шкал, но означают разное: у граммов — темп набора («где
    // вы были бы, набирая ровно»), у баланса — просто ход недели, догонять там
    // нечего.
    let cell = |value: f64, target: f64, label: &'static str, unit: &'static str,
                decimals: usize| {
        let (bar, val) = crate::components::gauge::at_least_colors(value, target);
        let pace = Some(pace);
        view! {
            <crate::components::gauge::Gauge
                value=value target=target
                label=label.to_string()
                unit=unit.to_string()
                color=bar.to_string()
                height=12.0
                decimals=decimals
                pace=pace
                value_color=val.map(String::from)/>
        }
    };
    view! {
        {cell(w.acids.epa_dha_g, w.epa_dha_target, "Омега-3/нед", "г", 2)}
        // «Баланс», а не «Жиры»: это отношение, а не количество, и название должно
        // читаться так же, как о нём говорит история — «насколько вы в балансе».
        // Шкала у него РАСХОДЯЩАЯСЯ: см. `BalanceGauge`.
        <crate::components::gauge::BalanceGauge
            value=w.ratio()
            target=crate::services::fats::Fat::Ratio.target()
            label="Баланс жира".to_string()
            height=12.0
    />
    }
}

/// Название главы, которая откроется за этим заданием.
///
/// Падеж зависит от места: в задании оно дополнение («…, чтобы открыть неделю
/// кальция»), в сроке закрытой планки — подлежащее («Неделя жиров откроется через
/// 5 дней»). Обе формы лежат в словаре, здесь выбирается нужная.
///
/// Пусто там, где открывать нечего: у красного мяса следующей главы пока нет, и в
/// его текстах места под подстановку тоже нет.
/// Всё, от чего зависит ЗАДАНИЕ НЕДЕЛИ. Собрано в одну запись, чтобы расчёт
/// подписи не тащил за собой десяток отдельных значений.
struct TaskInput {
    /// Зелёные дни трёх гейтов: белка с овощами, шагов, кальция.
    green: u32,
    steps_green: u32,
    calcium_green: u32,
    /// Ход недельных глав — каждая знает свой день недели и свои величины.
    iron: Option<crate::services::iron::WeeklyIron>,
    fat: Option<crate::services::fats::WeeklyFats>,
    red_meat: Option<crate::services::red_meat::WeeklyRedMeat>,
    egg: Option<crate::services::egg::WeeklyEggs>,
    /// Ход недели клетчатки — только для подписи: своей шкалы у неё нет.
    fiber: Option<crate::services::fiber::WeeklyFiber>,
    /// Пройден ли гейт своей главы: у мяса, яиц и клетчатки счётчик после этого
    /// встаёт на ноль.
    red_meat_gate: bool,
    egg_gate: bool,
    fiber_gate: bool,
}

/// Состояние полосы калорий: недобор, попадание в коридор, перебор.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum CalorieBar {
    /// Недобрал больше коридора — день ещё не закрыт, судить нечего.
    Under,
    /// В пределах ±[`CALORIE_BAND_KCAL`] от планки — выполнено.
    Hit,
    /// Перебрал больше коридора.
    Over,
}

impl CalorieBar {
    fn color(self) -> &'static str {
        match self {
            CalorieBar::Under => "#9aa0a6",
            CalorieBar::Hit => "#1fa463",
            CalorieBar::Over => "#e0304f",
        }
    }
}

/// Цвет полосы калорий — ПО ТОМУ ЖЕ КОРИДОРУ, по которому индикатор калорий судит
/// день (`indicators::CALORIE_BAND_KCAL`, ±50 ккал).
///
/// Прежде полоса краснела от любого превышения, и перебор в семь ккал выглядел
/// провалом, хотя индикатор тот же день считал зелёным. Полоса и значок обязаны
/// говорить одно и то же, поэтому границы здесь ровно те же: ровно ±50 — уже мимо.
pub(crate) fn calorie_bar_state(eaten: f64, planka: f64) -> CalorieBar {
    let band = crate::services::indicators::CALORIE_BAND_KCAL;
    if eaten - planka >= band {
        CalorieBar::Over
    } else if planka - eaten > band {
        CalorieBar::Under
    } else {
        CalorieBar::Hit
    }
}

/// ЗАДАНИЕ ТЕКУЩЕЙ НЕДЕЛИ: что держать, ради чего и сколько осталось.
///
/// Стоит в шапке карточки — там, где раньше была надпись «Ваша дневная планка».
/// Планка и так нарисована шкалой прямо под ним, а задание — единственное, что
/// человеку нужно делать сегодня, и место ему первое.
fn task_caption(input: &TaskInput) -> Option<View> {
    let TaskInput { green, steps_green, calcium_green, .. } = *input;
    // Подпись показывает гейт ТЕКУЩЕЙ главы — той, что ещё не открыта. Каждый
    // гейт держится ровно до момента, когда открылась следующая глава, и после
    // этого не возвращается, даже если его счётчик зелёных дней потом упал.
    //
    // Иначе выходило так: неделя железа давно открыта, а подпись зовёт «держите
    // индикатор планки по шагам зелёным 7 дней» — шаговый гейт своё отработал, но
    // индикатор позеленел в оранжевый, счётчик сбросился, и гейт вылез заново.
    // Гейты монотонны: пройденное не отменяется.
    let active: Option<(&'static str, u32)> =
        if !indicators::activity_unlocked() && green < indicators::GREEN_GATE_DAYS {
            Some(("dashboard.progress.gate_title", green))
        } else if indicators::activity_unlocked()
            && !indicators::calcium_unlocked()
            && steps_green < indicators::GREEN_GATE_DAYS
        {
            Some(("dashboard.progress.steps_gate_title", steps_green))
        } else if indicators::calcium_unlocked()
            && !crate::services::iron::unlocked()
            && calcium_green < indicators::GREEN_GATE_DAYS
        {
            Some(("dashboard.progress.calcium_gate_title", calcium_green))
        } else if let Some(w) = input.fiber.as_ref() {
            // Клетчатка — последняя открытая глава, и подпись держится всю её:
            // следующей за ней пока нет. Планка ПРЯМАЯ, её набирают.
            let done = if input.fiber_gate { indicators::GREEN_GATE_DAYS } else { w.day_of_week - 1 };
            Some(("dashboard.progress.fiber_gate_title", done))
        } else if let Some(w) =
            input.egg.as_ref().filter(|_| !crate::services::fiber::unlocked())
        {
            // Яйца — последняя открытая глава, и подпись держится всю её: следующей
            // за ней пока нет, ждать нечего, кроме конца своей недели.
            //
            // Планка ПРЯМАЯ, её набирают. Набрал — счётчик уже ни к чему не ведёт и
            // встаёт на ноль, как было у мяса до появления этой главы.
            let done = if input.egg_gate { indicators::GREEN_GATE_DAYS } else { w.day_of_week - 1 };
            Some(("dashboard.progress.egg_gate_title", done))
        } else if let Some(w) =
            input.red_meat.as_ref().filter(|_| !crate::services::egg::unlocked())
        {
            // Планка тут ОБРАТНАЯ — её не выполняют, а превышают. Перебрал — говорим
            // прямо и зовём на следующую неделю; с её началом счётчик встаёт в
            // первый день, съеденное обнуляется, и подпись возвращается к первой.
            if w.grams > w.limit {
                Some(("dashboard.progress.red_meat_over_title", w.day_of_week - 1))
            } else if input.red_meat_gate {
                Some((
                    "dashboard.progress.red_meat_gate_title",
                    indicators::GREEN_GATE_DAYS,
                ))
            } else {
                Some(("dashboard.progress.red_meat_gate_title", w.day_of_week - 1))
            }
        } else if let Some(w) =
            input.fat.as_ref().filter(|_| !crate::services::red_meat::unlocked())
        {
            // Жиры: гейт закрывается по морским омега-3, поэтому и заголовок про них.
            // Набранная норма, как у железа, не гасит подпись, а меняет её.
            let key = if w.acids.epa_dha_g >= w.epa_dha_target {
                "dashboard.progress.fat_done_title"
            } else {
                "dashboard.progress.fat_gate_title"
            };
            Some((key, w.day_of_week - 1))
        } else if let Some(w) =
            input.iron.as_ref().filter(|_| !crate::services::fats::unlocked())
        {
            let key = if w.absorbed_mg < w.target_mg {
                "dashboard.progress.iron_gate_title"
            } else {
                "dashboard.progress.iron_done_title"
            };
            Some((key, w.day_of_week - 1))
        } else {
            None
        };

    let grams_target = input.fiber.as_ref().map(|w| w.target);
    active.map(|(title_key, done)| {
        // Показываем ОСТАТОК дней, а не «5/7»: дробь читается двояко — то ли
        // сделано, то ли осталось.
        let left = indicators::GREEN_GATE_DAYS.saturating_sub(done);
        // Вторая строка — срок. Считаются в ней разные вещи: у недельных глав дни
        // до конца недели, у гейтов — недостающие ЗЕЛЁНЫЕ дни (семь в скользящем
        // окне восьми суток), отчего число может стоять на месте или расти обратно.
        // Человеку они называются просто днями.
        //
        // У закрытой планки своя строка: молчание читалось бы как «не засчитано». У
        // перебранного мяса срока нет вовсе — неделя уже потеряна, и обратный отсчёт
        // по ней был бы счётчиком до конца наказания.
        let progress_key = match title_key {
            "dashboard.progress.iron_done_title" | "dashboard.progress.fat_done_title" => {
                Some("dashboard.progress.iron_done_progress")
            }
            "dashboard.progress.red_meat_over_title" => None,
            // У КЛЕТЧАТКИ обратного отсчёта нет: индикатор необязательный, мы его
            // предлагаем. Держать надо то, на чём стоит похудение — калории, белок,
            // шаги, фрукты и овощи; клетчатка идёт довеском, и вторая строка говорит
            // ровно это, называя её недельную величину.
            "dashboard.progress.fiber_gate_title" => {
                Some("dashboard.progress.fiber_optional_progress")
            }
            _ => Some("dashboard.progress.days_left_progress"),
        };
        let word = match crate::services::i18n::get_lang() {
            crate::services::i18n::Lang::En => {
                if left == 1 {
                    "day"
                } else {
                    "days"
                }
            }
            crate::services::i18n::Lang::Ru => {
                let (d10, d100) = (left % 10, left % 100);
                if d10 == 1 && d100 != 11 {
                    "день"
                } else if (2..=4).contains(&d10) && !(12..=14).contains(&d100) {
                    "дня"
                } else {
                    "дней"
                }
            }
        };
        let progress = progress_key.map(|key| {
            let line = t(key)
                .replace("{n}", &left.to_string())
                .replace("{w}", word)
                .replace("{week}", next_week_name(title_key))
                .replace("{g}", &grams_target.map(|g| format!("{g:.0}")).unwrap_or_default());
            view! { <span class="is-size-7 has-text-grey">{line}</span> }
        });
        // {g} — недельная планка клетчатки: своей шкалы у неё нет, и число живёт
        // только в этой строке.
        let title = t(title_key)
            .replace("{week}", next_week_name(title_key))
            .replace("{g}", &grams_target.map(|g| format!("{g:.0}")).unwrap_or_default());
        view! {
            <div style="display: flex; flex-direction: column; gap: 2px;">
                <span class="is-size-7 has-text-weight-semibold">{title}</span>
                {progress}
            </div>
        }
        .into_view()
    })
}

fn next_week_name(title_key: &str) -> &'static str {
    match title_key {
        "dashboard.progress.gate_title" => t("dashboard.progress.week_steps"),
        "dashboard.progress.steps_gate_title" => t("dashboard.progress.week_calcium"),
        "dashboard.progress.calcium_gate_title" => t("dashboard.progress.week_iron"),
        "dashboard.progress.iron_gate_title" => t("dashboard.progress.week_fat"),
        "dashboard.progress.fat_gate_title" => t("dashboard.progress.week_red_meat"),
        "dashboard.progress.red_meat_gate_title" => t("dashboard.progress.week_egg"),
        "dashboard.progress.egg_gate_title" => t("dashboard.progress.week_fiber"),
        "dashboard.progress.iron_done_title" => t("dashboard.progress.week_fat_nom"),
        "dashboard.progress.fat_done_title" => t("dashboard.progress.week_red_meat_nom"),
        _ => "",
    }
}

#[component]
pub fn ProgressWidget() -> impl IntoView {
    // «X/7» counters refresh when any of the three stores change.
    let food_ver = db::version("diary");
    let weight_ver = db::version("weight_entries");
    let steps_ver = db::version("step_entries");
    let counts = create_resource(
        move || (food_ver.get(), weight_ver.get(), steps_ver.get()),
        |_| async { local::progress_week_counts().await },
    );

    // Before the very first food entry we show how to add food instead of counters.
    let has_food = create_resource(
        move || food_ver.get(),
        |_| async { !local::list_diary_dates().await.is_empty() },
    );

    // The planka, once set, flips the widget to its "done" state.
    //
    // Слушаем сигнал ПЛАНОК, а не версию `goals`. Планка живёт в истории и
    // читается из синхронного кэша, а запись в `goals` — зеркало, и обновляется
    // оно РАНЬШЕ кэша: `set_calorie_goal` сперва пишет цель, потом историю.
    // Подписка на `goals` поэтому перечитывала планку до того, как та появилась,
    // и виджет застревал в состоянии «планки ещё нет» до следующего запуска —
    // ровно в тот момент, когда приложение впервые её и посчитало.
    let goals_ver = db::version("goals");
    let plankas_ver = crate::services::plankas::version_signal();
    let planka = create_resource(
        move || (goals_ver.get(), plankas_ver.get()),
        |_| async { local::calorie_goal_amount().await },
    );

    // Calories eaten TODAY (for the done-state gauge). Refreshes on diary edits.
    let today_kcal = create_resource(
        move || food_ver.get(),
        |_| async {
            let today = chrono::Local::now().format("%Y-%m-%d").to_string();
            local::kcal_on(&today).await
        },
    );

    // Sticky views of the resources: `None` only until the first successful load
    // (→ render nothing), then fresh-or-last-known across navigations.
    let planka_s = move || sticky(&PLANKA_CACHE, planka.get());
    let hasfood_s = move || sticky(&HASFOOD_CACHE, has_food.get());
    let counts_s = move || sticky(&COUNTS_CACHE, counts.get());

    // Nutrition indicators (consistency over time): the states for the currently
    // UNLOCKED indicators, read through the per-day cache. Async — `None` until the
    // aggregate resolves, so the row paints grey first, then colours in. Refreshes
    // when the diary, foods (tags/nutrients), weight or GOALS change — планка по
    // белку считается от калорийной планки, а та живёт в goals.
    let foods_ver = db::version("foods");
    let inds = create_local_resource(
        move || (food_ver.get(), foods_ver.get(), weight_ver.get(), goals_ver.get()),
        |_| async { indicators::unlocked_indicator_states().await },
    );
    let inds_s = move || sticky(&INDS_CACHE, inds.get());

    // Daily-nutrient gauges (today's amount vs each per-day target). Depends on the
    // diary, the foods (nutrient values / tags), the latest weight and the calorie
    // planka (protein is a share of it). Grey until data appears per metric.
    let gauges = create_local_resource(
        move || (food_ver.get(), foods_ver.get(), weight_ver.get(), goals_ver.get()),
        |_| async { indicators::daily_gauges().await },
    );
    let gauges_s = move || sticky(&GAUGES_CACHE, gauges.get());

    // "Keep them green" gate: GREEN days accrued toward the required week. Same
    // dependencies as the indicators (they drive the per-day green check).
    let gate = create_local_resource(
        move || (food_ver.get(), foods_ver.get(), weight_ver.get(), goals_ver.get()),
        |_| async { indicators::green_gate_progress().await },
    );
    let gate_s = move || sticky(&GATE_CACHE, gate.get());

    // The activity-week (steps) gate: GREEN steps-days accrued toward its own week.
    // Depends on step logging and the steps planka goal.
    let steps_gate = create_local_resource(
        move || (steps_ver.get(), goals_ver.get()),
        |_| async { indicators::steps_gate_progress().await },
    );
    let steps_gate_s = move || sticky(&STEPS_GATE_CACHE, steps_gate.get());

    // Недельное железо (усвоенные мг за текущую неделю железа против нормы).
    // Зависит от дневника и продуктов — коэффициент усвоения приезжает фоновым
    // проходом по железу.
    let heme_week = create_local_resource(
        move || food_ver.get(),
        |_| async { crate::services::heme::weekly_progress().await },
    );
    let fat_week = create_local_resource(
        move || food_ver.get(),
        |_| async { crate::services::fats::weekly_progress().await },
    );
    let red_meat_week = create_local_resource(
        move || (food_ver.get(), foods_ver.get()),
        |_| async { crate::services::red_meat::weekly_progress().await },
    );
    // Гейт красного мяса: закрыта ли уже хотя бы одна неделя. Условие исполнено —
    // счётчик встаёт на ноль и там остаётся: неделя яиц ещё не сделана, открывать
    // по этому гейту пока нечего.
    let red_meat_gate = create_local_resource(
        move || (food_ver.get(), foods_ver.get()),
        |_| async { crate::services::red_meat::week_closed_since_open().await },
    );
    let egg_week = create_local_resource(
        move || (food_ver.get(), foods_ver.get()),
        |_| async { crate::services::egg::weekly_progress().await },
    );
    // Гейт яиц: закрыта ли уже хотя бы одна неделя. Следующей главы за яйцами пока
    // нет, поэтому счётчик, как было у мяса, встаёт на ноль и там остаётся.
    // Клетчатка: ход недели для подписи и «закрыта ли уже неделя» для счётчика.
    // Шкалы у неё нет — числа нужны только тексту задания.
    let fiber_week = create_local_resource(
        move || (food_ver.get(), foods_ver.get()),
        |_| async { crate::services::fiber::weekly_progress().await },
    );
    let fiber_gate = create_local_resource(
        move || (food_ver.get(), foods_ver.get()),
        |_| async { crate::services::fiber::week_closed_since_open().await },
    );
    let egg_gate = create_local_resource(
        move || (food_ver.get(), foods_ver.get()),
        |_| async { crate::services::egg::week_closed_since_open().await },
    );
    let iron_week = create_local_resource(
        move || (food_ver.get(), foods_ver.get()),
        |_| async { crate::services::iron::weekly_progress().await },
    );

    // The calcium-week gate: GREEN calcium-days accrued toward its own week. Depends
    // on food/nutrient data (calcium comes from enriched foods) and the goals store.
    let calcium_gate = create_local_resource(
        move || (food_ver.get(), foods_ver.get(), goals_ver.get()),
        |_| async { indicators::calcium_gate_progress().await },
    );
    let calcium_gate_s = move || sticky(&CALCIUM_GATE_CACHE, calcium_gate.get());

    // Считается ли планка прямо сейчас — общий замок для обоих расчётов ниже:
    // первого и пересчёта при смене цели. Ручного расчёта нет, кнопку убрали.
    let busy = create_rw_signal(false);

    // AUTO first planka: the moment the observation week completes (7/7/7 across
    // food/weight/steps) and no planka exists yet, compute and set it — no button.
    // Setting the goal bumps the goals version, so the widget flips to the gauge +
    // indicators + first gate by itself. `auto_tried` keeps a failed attempt from
    // looping; the in-flight re-check of the goal guards against a double set.
    let auto_tried = create_rw_signal(false);
    create_effect(move |_| {
        let all_done = matches!(counts.get(), Some((f, w, s)) if f >= 7 && w >= 7 && s >= 7);
        let no_planka = matches!(planka.get(), Some(None));
        if all_done && no_planka && !busy.get_untracked() && !auto_tried.get_untracked() {
            auto_tried.set(true);
            busy.set(true);
            spawn_local(async move {
                if local::calorie_goal_amount().await.is_none() {
                    if let Some(n) = local::calorie_planka_suggestion().await {
                        local::set_calorie_goal(n).await;
                        sync::push_background();
                        // Announce the FIRST planka with an inbox letter — same
                        // channel as the weekly recompute and the curator's
                        // set_planka, so the user learns the number even if they
                        // miss the widget flipping.
                        crate::services::letters::add(crate::services::letters::Letter {
                            id: format!("planka-first-{}", chrono::Local::now().format("%Y-%m-%d")),
                            created_at: chrono::Local::now().to_rfc3339(),
                            body: first_planka_letter(n).await,
                            read: false,
                            action: None,
                            action_done: false,
                        });
                    }
                }
                busy.set(false);
            });
        }
    });
    // ПЕРЕСЧЁТ ПРИ СМЕНЕ ЦЕЛИ — тоже сам, без кнопки.
    //
    // Смена курса (похудение / набор / поддержание) поднимает флаг «планка больше не
    // отвечает цели». Раньше на этом месте карточка показывала предупреждение и
    // кнопку «Пересчитать планку»; кнопки в продукте нет и не подразумевается, а
    // человека нельзя оставлять со старой планкой и без способа её обновить.
    // Поэтому считаем заново тем же алгоритмом, что и первую, — и флаг снимается
    // внутри `set_calorie_goal`.
    let planka_stale = local::planka_stale_signal();
    let stale_tried = create_rw_signal(false);
    create_effect(move |_| {
        if !planka_stale.get() || busy.get_untracked() || stale_tried.get_untracked() {
            return;
        }
        // Планки ещё нет вовсе — это дело эффекта выше, он посчитает первую.
        if matches!(planka.get(), Some(None) | None) {
            return;
        }
        stale_tried.set(true);
        busy.set(true);
        spawn_local(async move {
            if let Some(n) = local::calorie_planka_suggestion().await {
                local::set_calorie_goal(n).await;
                sync::push_background();
            }
            busy.set(false);
            // Цель может смениться ещё раз — тогда пересчитать надо снова.
            stale_tried.set(false);
        });
    });

    let goal_word = move || match profile::get_goal() {
        CourseGoal::Lose => t("dashboard.progress.word_lose"),
        CourseGoal::Gain => t("dashboard.progress.word_gain"),
        CourseGoal::Maintain => t("dashboard.progress.word_maintain"),
    };

    let counter = move |label_key: &'static str, done: u32| {
        let hit = done >= 7;
        view! {
            <div style="display: flex; align-items: center; justify-content: space-between;">
                <span class="is-size-6">{move || t(label_key)}</span>
                <span class="is-size-6 has-text-weight-semibold"
                    style:color=move || if hit { "var(--bulma-success)" } else { "var(--bulma-text)" }>
                    {format!("{}/7", done.min(7))}
                </span>
            </div>
        }
    };

    view! {
        {move || {
            // Render nothing until the primary data has loaded ONCE. After that the
            // sticky caches keep these `Some`, so navigating back to the dashboard
            // paints the real state immediately — no 0/7 / "add food" flash.
            let (Some(planka_v), Some(has_food_v), Some((food, weight, steps))) =
                (planka_s(), hasfood_s(), counts_s())
            else {
                return ().into_view();
            };
            view! {
                <div attr:data-testid="progress-widget" style=CARD>
                    {match planka_v {
                        // Planka computed → a calorie GAUGE (eaten today / target).
                        //
                        // ЦВЕТ — ПО КОРИДОРУ ±50 ККАЛ, тому же, по которому индикатор
                        // калорий судит день (`indicators::CALORIE_BAND_KCAL`):
                        //
                        //   недобрал больше 50   → серый: день ещё не закрыт;
                        //   в пределах ±50       → зелёный: планка выполнена;
                        //   перебрал больше 50   → красный.
                        //
                        // Прежде полоса краснела от любого превышения, и перебор в семь
                        // ккал выглядел провалом, хотя индикатор тот же день считал
                        // зелёным. Полоса и значок обязаны говорить одно и то же.
                        Some(n) => {
                            let calorie = {
                                let eaten = today_kcal.get().unwrap_or(0.0);
                                let state = calorie_bar_state(eaten, n);
                                let over = state == CalorieBar::Over;
                                let color = state.color().to_string();
                                view! {
                                    <div style="display: flex; flex-direction: column; gap: 10px;">
                                        // ЗАДАНИЕ НЕДЕЛИ вместо надписи «Ваша дневная
                                        // планка»: планку и так видно шкалой прямо под
                                        // ним, а задание — то единственное, что человеку
                                        // делать сегодня, и место ему первое.
                                        {move || task_caption(&TaskInput {
                                            green: gate_s().unwrap_or(0),
                                            steps_green: steps_gate_s().unwrap_or(0),
                                            calcium_green: calcium_gate_s().unwrap_or(0),
                                            iron: iron_week.get().flatten(),
                                            fat: fat_week.get().flatten(),
                                            red_meat: red_meat_week.get().flatten(),
                                            egg: egg_week.get().flatten(),
                                            fiber: fiber_week.get().flatten(),
                                            red_meat_gate: red_meat_gate.get().unwrap_or(false),
                                            egg_gate: egg_gate.get().unwrap_or(false),
                                            fiber_gate: fiber_gate.get().unwrap_or(false),
                                        })}
                                        <crate::components::gauge::Gauge
                                            value=eaten target=n
                                            label=t("dashboard.calories_title").to_string()
                                            unit=t("common.unit.kcal").to_string()
                                            color=color height=12.0
                                            value_color={over.then(|| "#e0304f".to_string())}/>
                                    </div>
                                }.into_view()
                            };
                            view! {
                                {calorie}
                                // Daily-nutrient bars below the calorie one.
                                {move || gauges_s().map(|g| daily_gauges_grid(g, iron_week.get().flatten(), heme_week.get().flatten(), fat_week.get().flatten(), red_meat_week.get().flatten(), egg_week.get().flatten()))}
                            }.into_view()
                        },
                        // Before the first food entry: explain how to add food + «?».
                        None if !has_food_v => {
                            let go_help = move |_| use_navigate()("/help/food", Default::default());
                            view! {
                                <p class="is-size-6" style="line-height: 1.5; margin: 0;">
                                    {move || t("dashboard.progress.help_1")}
                                </p>
                                <p class="is-size-6" style="line-height: 1.5; margin: 0;">
                                    {move || t("dashboard.progress.help_2")}
                                </p>
                                <p class="is-size-6" style="line-height: 1.5; margin: 0;">
                                    {move || t("dashboard.progress.help_3")}
                                </p>
                                <div style="display: flex; justify-content: center; margin-top: 6px;">
                                    <button attr:aria-label="?" on:click=go_help
                                        on:pointerup=|ev: web_sys::PointerEvent| ev.stop_propagation()
                                        style="width: 44px; height: 44px; border-radius: 50%; border: none; cursor: pointer; \
                                               background: var(--bulma-link); color: #fff; font-size: 1.5rem; \
                                               font-weight: 700; line-height: 1;">
                                        "?"
                                    </button>
                                </div>
                            }.into_view()
                        }
                        // Still collecting the week of observations. When the week
                        // completes, the auto-planka effect above computes the planka
                        // and this branch flips to the gauge state on its own — the
                        // completed state shows a brief spinner, not a button.
                        None => {
                            let all_done = food >= 7 && weight >= 7 && steps >= 7;
                            view! {
                                <p class="is-size-7 has-text-grey" style="line-height: 1.45; margin: 0;">
                                    {move || t("dashboard.progress.intro").replace("{word}", goal_word())}
                                </p>
                                <div style="display: flex; flex-direction: column; gap: 8px; margin-top: 2px;">
                                    {counter("dashboard.progress.nutrition", food)}
                                    {counter("weight.widget_title", weight)}
                                    {counter("steps.title", steps)}
                                </div>
                                {all_done.then(|| view! {
                                    <div style="display: flex; justify-content: center; padding: 6px 0 2px;">
                                        <div class="ft-spinner"></div>
                                    </div>
                                })}
                                // Documentation-style link (dashed underline) to the "how to
                                // keep the diary" help hub.
                                <div style="text-align: center; margin-top: 8px;">
                                    <a href="/help/diary" class="is-size-7"
                                        on:pointerup=|ev: web_sys::PointerEvent| ev.stop_propagation()
                                        style="color: var(--bulma-text-weak); text-decoration: underline; \
                                               text-decoration-style: dashed; text-underline-offset: 3px;">
                                        {move || t("help.link.diary")}
                                    </a>
                                </div>
                            }.into_view()
                        }
                    }}
                    // Nutrition indicators (CONSISTENCY over time) as icons at the
                    // bottom — ONLY once the FIRST PLANKA exists (the observation week
                    // is done and «Рассчитать» was pressed). Before that a newcomer
                    // must see just the 7-day counters: the indicators and the «keep
                    // them green» gate open TOGETHER WITH the planka, not with the
                    // first food entry. Drawn from the fixed unlocked list so they
                    // appear GREY immediately, then colour in when the cached
                    // aggregate resolves. Different purpose from the gauges above
                    // (what's still left TODAY), so an overlapping metric in both is
                    // intentional, not a duplicate.
                    {(planka_v.is_some()).then(|| {
                        let states: std::collections::HashMap<&'static str, IndicatorState> =
                            inds_s().unwrap_or_default().into_iter().collect();
                        let mut row: Vec<(&'static str, IndicatorState)> = indicators::displayed_indicators()
                            .into_iter()
                            .map(|k| (k, states.get(k).copied().unwrap_or(IndicatorState::Unknown)))
                            .collect();
                        // Sort left→right by severity: red, then orange, then green
                        // (unknown/grey last). Equal priority within a colour → by name.
                        // Показываются ПЕРВЫЕ СЕМЬ — ровно ряд по ширине телефона.
                        // Отсечка стоит ПОСЛЕ сортировки, поэтому теряются самые
                        // спокойные, а всё, что требует внимания, всегда на виду: стоит
                        // индикатору покраснеть, он тут же встаёт первым.
                        let rank = |s: IndicatorState| match s {
                            IndicatorState::Red => 0,
                            IndicatorState::Orange => 1,
                            IndicatorState::Green => 2,
                            IndicatorState::Unknown => 3,
                        };
                        row.sort_by(|a, b| {
                            rank(a.1).cmp(&rank(b.1)).then_with(|| icon_for(a.0).label.cmp(icon_for(b.0).label))
                        });
                        row.truncate(7);
                        view! {
                            <div style="border-bottom: 0.5px solid var(--bulma-border-weak);"></div>
                            {indicators_row(row)}
                        }
                    })}
                </div>
            }.into_view()
        }}
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Тот самый случай со снимка: 2957 при планке 2950. Семь ккал сверху — это
    /// попадание, а не провал.
    #[test]
    fn perebor_v_sem_kkal_ostayotsya_zelyonym() {
        assert_eq!(calorie_bar_state(2957.0, 2950.0), CalorieBar::Hit);
    }

    #[test]
    fn granitsy_koridora_te_zhe_chto_u_indikatora() {
        // Ровно ±50 — уже мимо: индикатор судит день по строгому «меньше 50».
        assert_eq!(calorie_bar_state(3000.0, 2950.0), CalorieBar::Over);
        assert_eq!(calorie_bar_state(2900.0, 2950.0), CalorieBar::Hit);
        assert_eq!(calorie_bar_state(2899.0, 2950.0), CalorieBar::Under);
        assert_eq!(calorie_bar_state(2999.0, 2950.0), CalorieBar::Hit);
    }

    /// Пустой день — серый: человек ещё не ел, а не провалил планку.
    #[test]
    fn pustoy_den_seryi() {
        assert_eq!(calorie_bar_state(0.0, 2950.0), CalorieBar::Under);
    }
}
