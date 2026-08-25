//! Nutrition indicators: turn a week (and up to 8 weeks of history) of diary data
//! into a green / orange / red / unknown state per indicator.
//!
//! Two families (per the product spec):
//!
//! * **Daily-goal** (calcium, fiber, veg/fruit): over the LAST 7 DAYS, count
//!   the days the per-day target was missed.
//!     0 misses → green · 1–3 → orange · ≥4 → red.
//!
//! * **Weekly-goal** (omega-3, eggs, red/processed meat, iron): судится по ДВУМ
//!   ПОСЛЕДНИМ завершённым неделям — неделя в разгаре не судится никогда, чтобы
//!   «ещё не набрано» не читалось как провал.
//!     последняя закрыта → green · последняя пропущена → orange ·
//!     две подряд пропущены → red.
//!   История за восемь недель никуда не делась: она показывается в подробностях
//!   виджета. "Missed" for a LIMIT goal (red meat) means the amount went OVER the limit.
//!
//! `Unknown` (grey) is used when a nutrient has no data at all yet (e.g. calcium is
//! never present on any logged food until the nutrient-fill pipeline exists).

use std::collections::{HashMap, HashSet};

use chrono::{Datelike, Duration, NaiveDate};

use super::local;
use super::profile::{self, Sex};

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum IndicatorState {
    Green,
    Orange,
    Red,
    Unknown,
}

// ── Targets (WHO / user-set; adjustable) ─────────────────────────────────────
const CALCIUM_PER_DAY_MG: f64 = 1000.0; // user: 1 g/day for everyone
// Омеги-3 здесь БОЛЬШЕ НЕТ. Она собиралась общим проходом как обычный нутриент в
// миллиграммах на 100 г — то есть «назови число» без таблицы категорий, — и одна
// порция скумбрии закрывала недельную норму 3500 мг целиком. Теперь длинные морские
// омега-3 (EPA+DHA) и растительная АЛК считаются из профиля жира и живут в
// `services::fats` со своими недельными нормами.

/// Vegetables/fruit target (g/day): user-set — women 600, men 800. Unknown sex →
/// 600 (the lower, so it isn't spuriously missed before the persona is complete).
fn veg_fruit_per_day_g() -> f64 {
    let ours = match profile::get_sex() {
        Some(Sex::Male) => 800.0,
        _ => 600.0,
    };
    crate::services::curator_plankas::or_ours("veg_fruit", ours)
}

/// Суточная норма кальция: наша константа, если куратор не назвал свою.
pub fn calcium_per_day_mg() -> f64 {
    crate::services::curator_plankas::or_ours("calcium", CALCIUM_PER_DAY_MG)
}

// Nutrient display names. `Food.nutrients` is keyed by the display name (same as
// `goal.nutrient`), so these are used directly as the map keys. The background
// enricher writes under the exact same names.
pub const N_CALCIUM: &str = "Кальций";
/// Legacy key: earlier builds wrote iron into `Food.nutrients`. Nothing computes
/// from it any more (iron moved to its own fields + `services::iron`); the name is
/// kept solely so those old values can be FILTERED OUT of nutrient listings.
pub const N_IRON: &str = "Железо";
/// Тоже legacy: ранние сборки писали омегу-3 в карту нутриентов. Ничего от неё уже
/// не считается — омега-3 выводится из профиля жира (`services::fats`), — имя
/// сохранено только чтобы отфильтровать старые значения из списков нутриентов.
pub const N_OMEGA3: &str = "Омега-3";
pub const N_FIBER: &str = "Клетчатка";

// ── Pure state machines (unit-tested) ────────────────────────────────────────

/// Daily-goal colour from the number of missed days out of the last 7.
///
/// Общее правило для всех дневных индикаторов, включая мясо глубокой переработки,
/// где промах — это день, в который человек его ЕЛ.
pub(crate) fn daily_state(misses: u32) -> IndicatorState {
    match misses {
        0 => IndicatorState::Green,
        1..=3 => IndicatorState::Orange,
        _ => IndicatorState::Red,
    }
}

/// The CALORIE indicator's success band: a day is green when intake landed
/// STRICTLY within ±50 kcal of that day's planka (planka 3000 → 2951…3049 is
/// green; 2950/3050 already miss). Indicator/gate semantics ONLY.
pub const CALORIE_BAND_KCAL: f64 = 50.0;

/// Green-day test for one indicator from its frozen `(value, ratio)` pair.
/// Calories: within the ±band of the day's planka (reconstructed as
/// `value / ratio`). Everything else: AtLeast — ratio ≥ 1.0.
fn day_green(key: &str, value: f64, ratio: Option<f64>) -> bool {
    match key {
        "calories" => match ratio {
            Some(r) if r > 0.0 && value > 0.0 => {
                let target = value / r;
                (value - target).abs() < CALORIE_BAND_KCAL
            }
            _ => false,
        },
        _ => matches!(ratio, Some(r) if r >= 1.0),
    }
}

/// Missed-day test — the judgeable inverse of [`day_green`]: a day counts as a
/// miss only when its frozen ratio exists (the target was known at freeze time).
fn day_missed(key: &str, value: f64, ratio: Option<f64>) -> bool {
    match key {
        "calories" => ratio.is_some() && !day_green(key, value, ratio),
        _ => ratio.map_or(false, |r| r < 1.0),
    }
}

/// Number of COMPLETED weeks every weekly indicator is judged over — the same
/// window for orange and for red.
pub const WEEKLY_WINDOW: usize = 8;

/// Weekly-goal colour by the TWO LAST completed weeks. `history_met` — the last
/// [`WEEKLY_WINDOW`] weeks, newest LAST; `history_met[i]` = that week's goal was met.
///
///   последняя закрыта                  → green
///   последняя не закрыта               → orange
///   две последние подряд не закрыты    → red
///
/// Цвет говорит о том, ЧТО СЕЙЧАС, а не о среднем за два месяца. Прежнее правило
/// считало доли по всему окну, и из него выходили две несуразности: неделя, взятая
/// после провала, не возвращала зелёный (доля-то осталась), а человек, закрывший
/// последние четыре недели подряд, продолжал видеть красный из-за четырёх старых.
/// Историю за всё окно никто не отменял — она рисуется в подробностях виджета,
/// восемью клетками; цвет же отвечает за последнюю неделю и за то, повторился ли
/// промах.
///
/// The week IN PROGRESS is deliberately NOT here. Judging it would paint every
/// indicator orange each Monday morning — «not yet reached» is not a failure, and a
/// user who closed eight weeks in a row must stay green while the ninth is running.
///
/// Empty history → `Unknown`: no completed week means nothing to judge yet.
///
/// Shared by every weekly indicator — omega-3, eggs, red meat and iron (whose weeks
/// are cut from the day its story opened rather than Mon–Sun). One rule, one place.
pub(crate) fn weekly_state(history_met: &[bool]) -> IndicatorState {
    let mut back = history_met.iter().rev();
    match (back.next(), back.next()) {
        (None, _) => IndicatorState::Unknown,
        (Some(true), _) => IndicatorState::Green,
        // Промах повторился — это уже не случайность.
        (Some(false), Some(false)) => IndicatorState::Red,
        (Some(false), _) => IndicatorState::Orange,
    }
}

// ── Data gathering ───────────────────────────────────────────────────────────

fn fmt(d: NaiveDate) -> String {
    d.format("%Y-%m-%d").to_string()
}

/// Does the user have at least a week of diary history? (The indicators row is
/// hidden before that.)
pub async fn enough_history() -> bool {
    // Count DISTINCT days with entries — `list_diary_dates` returns one date per
    // entry (with duplicates), so 7 items in a single day must NOT pass.
    let days: HashSet<String> = local::list_diary_dates().await.into_iter().collect();
    days.len() >= 7
}

// ── Progressive disclosure ───────────────────────────────────────────────────
// Which metrics are currently surfaced in the widget. The product opens more over
// time; today only the week-1 set. Calories is the planka gauge (drawn directly by
// the widget, not via `daily_gauges`).
pub const UNLOCKED_GAUGES: &[&str] = &["protein", "veg_fruit"];
/// The week-2 indicators — ALSO the set the "keep green 7 days" gate watches. The
/// step indicator is NOT here: it's added to the DISPLAY by [`displayed_indicators`]
/// once the activity week unlocks, and gets its OWN separate gate.
/// `calories` is the planka-adherence indicator: green day = intake within
/// ±[`CALORIE_BAND_KCAL`] of that day's planka (indicator/gate semantics only —
/// the gauge and the goal keep their AtMost meaning).
pub const UNLOCKED_INDICATORS: &[&str] = &["calories", "protein", "veg_fruit"];

/// App-flag: the activity week (step planka + step indicator) has been unlocked.
const ACTIVITY_UNLOCKED_KEY: &str = "activity_week_unlocked";

/// App-flag: the calcium week (calcium goal + calcium indicator + gauge) has been
/// unlocked. Opens once the activity (steps) gate is cleared.
const CALCIUM_UNLOCKED_KEY: &str = "calcium_week_unlocked";

/// Whether the activity week is unlocked (step indicator visible).
pub fn activity_unlocked() -> bool {
    crate::services::app_flags::get_bool(ACTIVITY_UNLOCKED_KEY)
}

/// Whether the calcium week is unlocked (calcium indicator + gauge visible).
pub fn calcium_unlocked() -> bool {
    crate::services::app_flags::get_bool(CALCIUM_UNLOCKED_KEY)
}

/// Indicators shown in the widget (icons + histograms), in display order: the
/// week-2 set plus `steps` once the activity week is unlocked. NB: distinct from
/// the gate set (`UNLOCKED_INDICATORS`) — steps has its own gate, so adding it to
/// the display must NOT change the protein/veg-fruit gate.
pub fn displayed_indicators() -> Vec<&'static str> {
    let mut v: Vec<&'static str> = UNLOCKED_INDICATORS.to_vec();
    if activity_unlocked() {
        v.push("steps");
    }
    if calcium_unlocked() {
        v.push("calcium");
    }
    // Iron is WEEKLY (see `services::iron`) but shares the indicator row.
    if crate::services::iron::unlocked() {
        v.push("iron");
        // Гемовое железо открывается ВМЕСТЕ с железом: это две стороны одного
        // разговора — «хватило ли» и «из чего». Своего условия у него нет.
        v.push("heme");
    }
    // Жиры — два недельных индикатора, открываются вместе, после закрытой планки
    // железа. Порознь они не читаются: «сколько морских омега-3» и «каков жир в
    // целом» — два вопроса об одном.
    if crate::services::fats::unlocked() {
        v.push("epa_dha");
        v.push("fat_ratio");
    }
    // Мясо — тоже пара, открывается целиком: сколько красного мяса за неделю и как
    // часто оно приходит переработанным. Первые наши индикаторы про ОГРАНИЧЕНИЕ:
    // зелёный тут значит «не перебрал», а не «набрал».
    if crate::services::red_meat::unlocked() {
        v.push("red_meat");
        v.push("processed_meat");
    }
    // Яйца — недельная планка, но обратная мясной: её надо НАБРАТЬ, а не удержаться
    // ниже. Открывается после закрытой недели мяса.
    if crate::services::egg::unlocked() {
        v.push("egg");
    }
    // Клетчатка — недельная планка граммов и ЕДИНСТВЕННЫЙ индикатор без своей шкалы:
    // она набирается фоном всего съеденного, и суточная полоска у неё дёргалась бы
    // от одного яблока. Открывается после закрытой недели яиц.
    if crate::services::fiber::unlocked() {
        v.push("fiber");
    }
    v
}

/// Daily gauges shown on the dashboard, in display order: the week-1 set plus
/// `calcium` once the calcium week is unlocked (steps is NOT a gauge — it has its
/// own widget). Distinct from the gate sets — each unlock has its own gate.
pub fn displayed_gauges() -> Vec<&'static str> {
    let mut v: Vec<&'static str> = UNLOCKED_GAUGES.to_vec();
    if calcium_unlocked() {
        v.push("calcium");
    }
    v
}

/// Unlock the activity week once the week-2 gate (protein + veg-fruit green for a
/// week) is cleared: compute the step planka from the whole history and set it as
/// the daily Steps goal, then flip the flag so the step indicator appears and its
/// own gate begins. Idempotent (guarded by the flag) and a no-op until there is
/// step data to base a planka on. Call on launch and after step/diary saves.
pub async fn maybe_unlock_activity_week() {
    if activity_unlocked() {
        return;
    }
    if green_gate_progress().await < GREEN_GATE_DAYS {
        return; // week-2 gate not cleared yet
    }
    open_activity_week().await;
}

/// САМО открытие недели активности, без проверки условия.
///
/// Вынесено из гейта, чтобы кураторская директива «открыть тему» делала ровно то
/// же, что честный путь, а не поднимала один флаг: тема — это ещё и планка шагов, и
/// якорь её собственного гейта.
pub async fn open_activity_week() {
    if activity_unlocked() {
        return;
    }
    let Some(planka) = local::steps_planka_from_history().await else {
        return; // no step history yet → can't set a planka; wait for data
    };
    crate::services::profile::set_steps_planka(planka as f64);
    // Anchor the step gate at today so "hold steps a week" counts from now.
    let today = crate::services::local::today_date();
    crate::services::app_flags::set(STEPS_GATE_OPEN_KEY, &fmt(today));
    crate::services::app_flags::set_bool(ACTIVITY_UNLOCKED_KEY, true);
}

/// Unlock the calcium week once the activity (steps) gate is cleared: set the daily
/// calcium goal (1 g/day) so the AI starts filling calcium on new foods, flip the
/// flag so the calcium indicator + gauge appear, and anchor the calcium gate at
/// today so its own "keep it green a week" begins now. Idempotent (guarded by the
/// flag). Call on launch, after the activity week is (re)checked.
pub async fn maybe_unlock_calcium_week() {
    if calcium_unlocked() {
        return;
    }
    if !activity_unlocked() {
        return; // the activity week must open (and be gated) first
    }
    if steps_gate_progress().await < GREEN_GATE_DAYS {
        return; // steps gate not cleared yet
    }
    open_calcium_week().await;
}

/// САМО открытие недели кальция, без проверки условия (см. `open_activity_week`).
pub async fn open_calcium_week() {
    if calcium_unlocked() {
        return;
    }
    local::set_calcium_goal(CALCIUM_PER_DAY_MG).await;
    let today = crate::services::local::today_date();
    crate::services::app_flags::set(CALCIUM_GATE_OPEN_KEY, &fmt(today));
    crate::services::app_flags::set_bool(CALCIUM_UNLOCKED_KEY, true);
}

/// Unlock the IRON week once the calcium gate is cleared: stamp the week's opening
/// day (day 1 of every iron week rolls from here — the gauge and the indicator both
/// count from it) and flip the flag so the weekly gauge + indicator appear and the
/// dedicated iron enrichment pass starts filling foods. Idempotent (guarded by the
/// flag). Call on launch, after the calcium week is (re)checked.
pub async fn maybe_unlock_iron_week() {
    use crate::services::iron;
    if iron::unlocked() {
        return;
    }
    if !calcium_unlocked() {
        return; // the calcium week must open (and be gated) first
    }
    if calcium_gate_progress().await < GREEN_GATE_DAYS {
        return; // calcium gate not cleared yet
    }
    open_iron_week().await;
}

/// САМО открытие недели железа, без проверки условия (см. `open_activity_week`).
pub async fn open_iron_week() {
    use crate::services::iron;
    if iron::unlocked() {
        return;
    }
    let today = crate::services::local::today_date();
    crate::services::app_flags::set(iron::IRON_WEEK_OPEN_KEY, &fmt(today));
    crate::services::app_flags::set_bool(iron::IRON_UNLOCKED_KEY, true);
    // Foods already in the diary have no iron yet — queue them now, otherwise the
    // first iron week would be measured against an empty set.
    crate::services::classify::sweep_unprocessed().await;
}

/// Открыть ЖИРЫ, когда закрыта планка железа: поставить якорь недели жира, поднять
/// флаг (три шкалы и три индикатора появляются, фоновый проход начинает выяснять
/// профили) и поставить в очередь всю уже имеющуюся продукцию.
///
/// Условие — именно ЗАКРЫТАЯ недельная планка железа, а не семь зелёных дней: железо
/// недельное, и «выдержал планку» у него означает закрытую неделю.
///
/// Идемпотентно: флаг монотонен, и за гардом «уже открыто» ничего не считается.
/// Возвращает `true`, если открытие произошло именно сейчас — по этому признаку
/// вызывающий может показать историю шестой недели.
pub async fn maybe_unlock_fat_week() -> bool {
    use crate::services::{fats, iron};
    if fats::unlocked() {
        return false;
    }
    if !iron::unlocked() {
        return false; // неделя железа должна открыться первой
    }
    let today = crate::services::local::today_date();
    // Якорь ставится в первый же запуск, когда железо открыто, а жиры ещё нет. С
    // этого дня и начинается отсчёт: недели железа, закончившиеся раньше, в гейт не
    // идут — иначе человек с уже закрытой прошлой неделей получал бы жиры мгновенно,
    // ничего для этого не сделав.
    let anchor = match fats::gate_anchor() {
        Some(d) => d,
        None => {
            crate::services::app_flags::set(fats::FAT_GATE_ANCHOR_KEY, &fmt(today));
            today
        }
    };
    if !iron::planka_closed(anchor).await {
        return false; // планка железа ещё не закрыта после якоря
    }
    open_fat_week().await;
    true
}

/// САМО открытие недели жиров, без проверки условия (см. `open_activity_week`).
pub async fn open_fat_week() {
    use crate::services::fats;
    if fats::unlocked() {
        return;
    }
    let today = crate::services::local::today_date();
    crate::services::app_flags::set(fats::FAT_WEEK_OPEN_KEY, &fmt(today));
    crate::services::app_flags::set_bool(fats::FAT_UNLOCKED_KEY, true);
    // У продуктов в дневнике профиля жира ещё нет — иначе первая неделя жира
    // мерилась бы по пустому множеству.
    crate::services::classify::sweep_unprocessed().await;
}

/// Открыть НЕДЕЛЮ КРАСНОГО МЯСА, когда закрыта неделя жиров: поставить якорь своей
/// недели, поднять флаг (шкала красного мяса и два индикатора появляются) и
/// добрать признаки у уже заведённых продуктов.
///
/// Следующее звено той же цепочки: кальций → железо → жиры → красное мясо. Условие —
/// хотя бы одна ЗАКРЫТАЯ по омега-3 неделя жиров с тех пор, как тема жиров открылась
/// у этого человека (`fats::week_closed_since_open`).
///
/// Своего якоря по дате выката здесь НЕТ, и это исправление: сначала он был
/// скопирован с гейта жиров, где брался день первого запуска новой сборки. Там он
/// был вынужденным — железо открылось раньше, чем появился механизм его недель, и
/// дату открытия взять было неоткуда. Здесь дата есть (`fat_week_opened_at`), и
/// правило должно опираться на неё: иначе человек, месяцами державший жиры,
/// получал «ждите ещё неделю» просто потому, что мы поздно выпустили главу.
///
/// Идемпотентно и монотонно, как остальные гейты: за гардом «уже открыто» ничего не
/// считается. Возвращает `true`, если открытие произошло именно сейчас — по этому
/// признаку показывается история про красное мясо.
pub async fn maybe_unlock_red_meat_week() -> bool {
    use crate::services::{fats, red_meat};
    if red_meat::unlocked() {
        return false;
    }
    if !fats::unlocked() {
        return false; // неделя жиров должна открыться первой
    }
    let today = crate::services::local::today_date();
    if !fats::week_closed_since_open().await {
        return false; // ни одной закрытой недели жиров с начала темы
    }
    open_red_meat_week().await;
    true
}

/// САМО открытие недели красного мяса, без проверки условия (см. `open_activity_week`).
pub async fn open_red_meat_week() {
    use crate::services::red_meat;
    if red_meat::unlocked() {
        return;
    }
    let today = crate::services::local::today_date();
    crate::services::app_flags::set(red_meat::RED_MEAT_WEEK_OPEN_KEY, &fmt(today));
    crate::services::app_flags::set_bool(red_meat::RED_MEAT_UNLOCKED_KEY, true);
    // Мясные признаки собираются с первого дня, но у кого-то из продуктов их может
    // не быть — например, они заведены сборкой, где признаков ещё не существовало.
    crate::services::classify::sweep_unprocessed().await;
}

/// Открыть НЕДЕЛЮ ЯИЦ, когда закрыта неделя красного мяса: поставить якорь своей
/// недели, поднять флаг (шкала и индикатор появляются) и добрать признаки у уже
/// заведённых продуктов.
///
/// Последнее звено цепочки: кальций → железо → жиры → красное мясо → яйца. Условие —
/// хотя бы одна ЗАВЕРШЁННАЯ неделя мяса, в которую человек уложился в планку
/// (`red_meat::week_closed_since_open`), то есть ровно тот гейт, счётчик которого
/// виджет уже показывает.
///
/// Идемпотентно и монотонно, как остальные гейты. Возвращает `true`, если открытие
/// произошло именно сейчас — по этому признаку показывается история про яйца.
pub async fn maybe_unlock_egg_week() -> bool {
    use crate::services::{egg, red_meat};
    if egg::unlocked() {
        return false;
    }
    if !red_meat::unlocked() {
        return false; // неделя мяса должна открыться первой
    }
    if !red_meat::week_closed_since_open().await {
        return false; // ни одной закрытой недели мяса с начала темы
    }
    open_egg_week().await;
    true
}

/// Открыть НЕДЕЛЮ КЛЕТЧАТКИ, когда закрыта неделя яиц.
///
/// Восьмая и пока последняя тема пути: кальций → железо → жиры → красное мясо →
/// яйца → клетчатка. Условие — хотя бы одна ЗАВЕРШЁННАЯ неделя яиц, в которую
/// планка набрана (`egg::week_closed_since_open`), то есть ровно тот гейт, счётчик
/// которого виджет уже показывает.
///
/// Идемпотентно и монотонно, как остальные гейты. Возвращает `true`, если открытие
/// произошло именно сейчас.
pub async fn maybe_unlock_fiber_week() -> bool {
    use crate::services::{egg, fiber};
    if fiber::unlocked() {
        return false;
    }
    if !egg::unlocked() {
        return false; // неделя яиц должна открыться первой
    }
    if !egg::week_closed_since_open().await {
        return false; // ни одной закрытой недели яиц с начала темы
    }
    open_fiber_week().await;
    true
}

/// САМО открытие недели клетчатки, без проверки условия (см. `open_activity_week`).
///
/// Ни признака, ни фонового прохода здесь не нужно: клетчатка приходит из нутриентов
/// продукта, которые заполняет обычный разбор, — доспрашивать нечего.
pub async fn open_fiber_week() {
    use crate::services::fiber;
    if fiber::unlocked() {
        return;
    }
    let today = crate::services::local::today_date();
    crate::services::app_flags::set(fiber::FIBER_WEEK_OPEN_KEY, &fmt(today));
    crate::services::app_flags::set_bool(fiber::FIBER_UNLOCKED_KEY, true);
}

/// САМО открытие недели яиц, без проверки условия (см. `open_activity_week`).
pub async fn open_egg_week() {
    use crate::services::egg;
    if egg::unlocked() {
        return;
    }
    let today = crate::services::local::today_date();
    crate::services::app_flags::set(egg::EGG_WEEK_OPEN_KEY, &fmt(today));
    crate::services::app_flags::set_bool(egg::EGG_UNLOCKED_KEY, true);
    // Признак яйца собирается с первого дня, но у продуктов, заведённых сборкой без
    // него, его нет — иначе первая неделя мерилась бы по пустому множеству.
    crate::services::classify::sweep_unprocessed().await;
}

// ── Per-indicator per-day cache ──────────────────────────────────────────────
// Each cacheable indicator has its OWN store (`ind_<key>`), keyed by date, holding
// the completed-day aggregate so it isn't recomputed on every render. Today is
// never cached (it's still changing). Invalidated per-day on diary edits and
// wholesale on food changes (nutrients/tags shift many days at once).

#[derive(Clone, serde::Serialize, serde::Deserialize)]
struct IndDay {
    date: String,
    value: f64,
    /// The DEGREE to which the day hit its target: `value / target`, FROZEN with the
    /// target valid at summarization time (1.0 = exactly on target, 0.5 = half, 1.5 =
    /// one-and-a-half). Frozen, not recomputed, when the target later changes. Storing
    /// the ratio (with the value) lets us both show the degree and reconstruct that
    /// day's target (`target = value / ratio`). `None` = a legacy record (or a day
    /// summarized while the target was unknown).
    #[serde(default)]
    ratio: Option<f64>,
    /// ПЛАНКА, по которой день был засчитан. Пишется прямо, а не выводится из
    /// `value / ratio`: делением она восстанавливается, только пока ratio верен, а
    /// если день замёрз по чужой планке — из него уже ничего не достать. Живой
    /// случай: планка шагов выросла с 10800 до 11800, пятница с 11500 шагами замёрзла
    /// по новой, и понять, что planka тогда была другой, стало не по чему.
    ///
    /// `None` — записи, сделанные до появления этого поля.
    #[serde(default)]
    target: Option<f64>,
    /// RFC3339 freeze moment — the sync conflict key (first computation wins).
    /// Empty on rows frozen before ind-day sync existed.
    #[serde(default)]
    computed_at: String,
}

fn now_stamp() -> String {
    chrono::Utc::now().to_rfc3339()
}

/// Indicator keys that have a per-day cache store. Keep in sync with the `ind_*`
/// object stores in `db::builder` and with [`invalidate_day`]/[`clear_cache`].
const CACHED_STORES: &[&str] = &["ind_protein", "ind_veg_fruit", "ind_steps", "ind_calories"];

/// Из них — те, что считаются ПО ЕДЕ. Правка дневника или продукта сбрасывает
/// только их.
///
/// `ind_steps` сюда НЕ входит: шаги от еды не зависят ни на грамм. Пока сброс шёл по
/// всем хранилищам разом, любое изменение продукта сносило замороженные дни шагов, и
/// они пересчитывались уже по ТЕКУЩЕЙ планке. Живой случай: миграция стирания
/// кальция и железа прошлась по всем продуктам, снесла заморозку шагов за каждый
/// день, где что-то ели, и дни пересчитались по новой планке 11800 — пятница с 11500
/// шагами из выполненной стала недобором, зелёный индикатор стал оранжевым.
const FOOD_CACHED_STORES: &[&str] = &["ind_protein", "ind_veg_fruit", "ind_calories"];

/// The cache store for `key`, or None if the indicator isn't cached.
fn cache_store(key: &str) -> Option<&'static str> {
    match key {
        "protein" => Some("ind_protein"),
        "veg_fruit" => Some("ind_veg_fruit"),
        "steps" => Some("ind_steps"),
        "calories" => Some("ind_calories"),
        _ => None,
    }
}

/// Raw per-day aggregate for `key` on `date` — the number compared to the target.
async fn compute_day_value(key: &str, date: &str) -> f64 {
    match key {
        "protein" => local::protein_grams_on(date).await,
        "veg_fruit" => local::veg_fruit_grams_on(date).await,
        "steps" => local::steps_on(date).await,
        "calories" => local::kcal_on(date).await,
        "calcium" => local::nutrient_grams_on(date, N_CALCIUM).await,
        "fiber" => local::nutrient_grams_on(date, N_FIBER).await,
        // Железо считается в УСВОЕННЫХ миллиграммах — то же, что копит недельная
        // полоса. Дневной ЦЕЛИ у него нет (`target_for` вернёт 0), поэтому столбик
        // показывает количество, но день по нему не судится: недельная механика.
        "iron" => crate::services::iron::absorbed_on(date).await,
        "heme" => crate::services::heme::portions_on(date).await,
        // Жиры — тоже недельная механика: дневное значение показывается столбиком,
        // но день по нему не судится (дневной цели нет).
        "epa_dha" => crate::services::local::fatty_acids_on(date).await.epa_dha_g,
        "fat_ratio" => crate::services::local::balance_acids_on(date)
            .await
            .unsat_to_sat()
            .unwrap_or(0.0),
        _ => 0.0,
    }
}

/// Cached-or-computed per-day `(value, ratio)`, where `ratio = value / target` is
/// FROZEN at summarization time (`None` while the target is unknown). A hit returns
/// the stored pair; a miss computes the value, freezes the ratio, and stores both.
/// `date` is expected to be a COMPLETED day — the caller never caches today.
///
/// The ratio is frozen deliberately: the target (protein from weight, veg from sex,
/// the calorie planka) can change later, but the degree to which a past day hit its
/// target must not be recomputed retrospectively.
async fn day_cached(key: &str, date: &str) -> (f64, Option<f64>) {
    let Some(store) = cache_store(key) else {
        let value = compute_day_value(key, date).await;
        return (value, ratio_for(key, date, value).await.0);
    };
    if let Some(rec) = crate::services::db::get::<IndDay>(store, date).await {
        if rec.ratio.is_some() {
            return (rec.value, rec.ratio);
        }
        // Legacy / not-yet-frozen record → freeze now (if the target is known) and store.
        let (ratio, target) = ratio_for(key, date, rec.value).await;
        if ratio.is_some() {
            crate::services::db::put(
                store,
                &IndDay {
                    date: date.to_string(),
                    value: rec.value,
                    ratio,
                    target,
                    computed_at: now_stamp(),
                },
            )
            .await;
        }
        return (rec.value, ratio);
    }
    let value = compute_day_value(key, date).await;
    let (ratio, target) = ratio_for(key, date, value).await;
    crate::services::db::put(
        store,
        &IndDay { date: date.to_string(), value, ratio, target, computed_at: now_stamp() },
    )
    .await;
    (value, ratio)
}

/// `(value / target, target)` по планке, ДЕЙСТВОВАВШЕЙ на `date`; `(None, None)`,
/// если планки на тот день ещё не было.
///
/// Планка возвращается вместе с долей и кладётся в запись: доля без неё не читается —
/// по 0.97 не видно, промах это по 11800 или попадание по 10800.
///
/// Калории и шаги берутся из ИСТОРИИ планок: они движутся, и день обязан судиться по
/// той величине, что действовала тогда. Остальные (белок, овощи) выводятся из
/// профиля и в истории не нуждаются — для них берётся текущая норма.
async fn ratio_for(key: &str, date: &str, value: f64) -> (Option<f64>, Option<f64>) {
    let kind = match key {
        "calories" => Some(local::PLANKA_CALORIES),
        "steps" => Some(local::PLANKA_STEPS),
        // Белок считается от веса, а вес меняется — значит меняется и норма.
        "protein" => Some(local::PLANKA_PROTEIN),
        _ => None,
    };
    let target = match kind {
        Some(k) => match local::planka_on(k, date).await {
            Some(t) => t,
            // Планки в тот день ЕЩЁ НЕ БЫЛО — неделя наблюдений, до самой первой
            // установки. Судить нечем: правила человеку не давали, а значит и
            // нарушить он его не мог. Планкой дня становится его собственный
            // результат, то есть день засчитывается как выполненный.
            //
            // Прежде здесь бралась ТЕКУЩАЯ планка, и первая же выдача перекрашивала
            // всю неделю наблюдений в перебор: человек ел 3850, планку получил на
            // 3550 — и задним числом оказался виноват в том, что ел до неё.
            //
            // Это правило ровно для тех планок, что ДВИЖУТСЯ и потому имеют
            // историю: калории, шаги, белок. Нормы, не зависящие от веса и его
            // динамики (кальций, овощи, жиры, омега-3), едины во времени — там
            // берётся текущая, и прошлое ею не искажается.
            None => value,
        },
        None => target_for(key).await,
    };
    if target > 0.0 {
        (Some(value / target), Some(target))
    } else {
        (None, None)
    }
}

/// Сбросить кэш за `date` у индикаторов, считающихся ПО ЕДЕ — вызывается при
/// изменении дневника за этот день. Шаги не трогаются: они от еды не зависят, а их
/// сброшенный день пересчитался бы по текущей планке и переписал уже вынесенный
/// вердикт.
pub async fn invalidate_day(date: &str) {
    for store in FOOD_CACHED_STORES {
        crate::services::db::delete(store, date).await;
    }
}

/// Clear every indicator cache — call for a bulk change (e.g. a range delete) that
/// can touch arbitrary past days.
pub async fn clear_cache() {
    for store in CACHED_STORES {
        crate::services::db::clear(store).await;
    }
}

/// Write-through for the STEP indicator: recompute `date`'s steps ratio against the
/// CURRENT planka and store it in `ind_steps`. Called from `save_steps` — the ONLY
/// moment the step indicator recomputes (steps are one final value per day, so no
/// waiting for day-end like the diary needs). Overwrites any previous row, so
/// re-logging or editing an old day rewrites that day's verdict. A `None` ratio
/// (planka not set yet) is stored but never counts as a miss.
pub async fn record_steps(date: &str) {
    let value = local::steps_on(date).await;
    let (ratio, target) = ratio_for("steps", date, value).await;
    crate::services::db::put(
        "ind_steps",
        &IndDay { date: date.to_string(), value, ratio, target, computed_at: now_stamp() },
    )
    .await;
}

// ── Sync bridge: frozen indicator days ride the regular sync ────────────────
// A day is computed ONCE (by whichever device had the planka + diary data at
// that moment) and then travels as DATA; other devices apply the ready value
// instead of computing their own. Conflict resolution lives on the server
// (first-writer-wins by `computed_at`).

/// Store name → the indicator key used in the wire `id` (`"<key>:<date>"`).
fn store_indicator(store: &str) -> &'static str {
    match store {
        "ind_protein" => "protein",
        "ind_veg_fruit" => "veg_fruit",
        "ind_steps" => "steps",
        _ => "calories",
    }
}

/// Every locally frozen indicator day, in the sync wire shape.
pub async fn export_ind_days() -> Vec<api_types::IndDayRow> {
    let mut out = Vec::new();
    for store in CACHED_STORES {
        let indicator = store_indicator(store);
        for r in crate::services::db::list_all::<IndDay>(store).await {
            out.push(api_types::IndDayRow {
                id: format!("{indicator}:{}", r.date),
                indicator: indicator.to_string(),
                date: r.date,
                value: r.value,
                ratio: r.ratio,
                computed_at: r.computed_at,
            });
        }
    }
    out
}

/// The local `ind_*` cache store for an indicator key (sync wire mapping).
pub fn store_for_indicator(indicator: &str) -> Option<&'static str> {
    cache_store(indicator)
}

/// Sync-apply of ONE wire indicator-day row: UNTRACKED write (remote data must
/// not re-enter the outbox), skipped entirely when the local row is identical.
pub async fn apply_ind_day(row: &serde_json::Value) {
    let parsed: api_types::IndDayRow = match serde_json::from_value(row.clone()) {
        Ok(p) => p,
        Err(e) => {
            leptos::logging::error!("sync v2: bad ind_days row ({e}): {row}");
            return;
        }
    };
    let Some(store) = cache_store(&parsed.indicator) else {
        leptos::logging::error!("sync v2: unknown indicator {:?}", parsed.indicator);
        return;
    };
    let incoming = IndDay {
        date: parsed.date.clone(),
        value: parsed.value,
        ratio: parsed.ratio,
        // Планка по проводу не ездит: формат синка её не несёт. Восстанавливаем из
        // доли — для приехавшего дня она посчитана там же, где и доля, так что
        // деление здесь честное (в отличие от дня, замёрзшего по чужой планке).
        target: match (parsed.ratio, parsed.value) {
            (Some(r), v) if r > 0.0 => Some(v / r),
            _ => None,
        },
        computed_at: parsed.computed_at.clone(),
    };
    // Journal order is the truth (per-row conflicts, incl. the first-computation-
    // wins rule for indicator days, are resolved by the sync merge gate before a
    // batch is ever pushed); identical rows are skipped to keep idle syncs silent.
    if let Some(existing) = crate::services::db::get::<IndDay>(store, &parsed.date).await {
        if existing.date == incoming.date
            && existing.value == incoming.value
            && existing.ratio == incoming.ratio
            && existing.computed_at == incoming.computed_at
        {
            return;
        }
    }
    crate::services::db::put_untracked(store, &incoming).await;
}

// ── Calorie planka (per-day, frozen) ─────────────────────────────────────────
// The calorie planka is an AtMost goal recomputed every week. To keep PAST days
// honest (the diary must show the planka that applied THAT day, not today's), we
// freeze each completed day's `(intake, ratio)` — like the other indicators — from
// which that day's planka is reconstructed (`target = intake / ratio`). This is
// entirely separate from the weekly recompute: the recompute is unchanged.

/// The calorie planka that APPLIED on `date`. Today → the live planka. A completed
/// past day → the target frozen when the day was summarized (`intake / ratio`), so
/// it survives the weekly recompute; falls back to the current planka when there is
/// no usable frozen record (a 0-intake day, or days before this cache existed).
pub async fn calorie_planka_on(date: &str) -> Option<f64> {
    let current = local::calorie_goal_amount().await;
    let today = fmt(crate::services::local::today_date());
    if date >= today.as_str() {
        return current; // today / future: the live planka applies
    }
    let (value, ratio) = day_cached("calories", date).await;
    match ratio {
        Some(r) if r > 0.0 => Some(value / r),
        _ => current,
    }
}

/// Freeze the last two weeks of completed days' calorie result at the CURRENT planka
/// (idempotent — already-frozen days are left untouched). Call on launch/resume so a
/// completed day is captured against the planka that applied to it. Ordered BEFORE
/// the weekly recompute in the bootstrap, so on a recompute launch the recent days
/// are frozen at the OLD planka first. No-op until a planka exists.
pub async fn freeze_calories_recent() {
    if local::calorie_goal_amount().await.is_none() {
        return;
    }
    let today = crate::services::local::today_date();
    for i in 1..=14 {
        let _ = day_cached("calories", &fmt(today - Duration::days(i))).await;
    }
}

/// То же для ШАГОВ: заморозить последние две недели завершённых дней по ТЕКУЩЕЙ
/// планке (уже замороженные не трогаются). Вызывается перед недельным пересчётом,
/// иначе поднятая планка судит задним числом уже прошедшие дни.
///
/// Наблюдалось живьём: планка выросла с 10800 до 11800, а пятница с 11500 шагами
/// замёрзла уже по новой — 11500/11800 = 0.97, недобор, индикатор оранжевый. День
/// был выполнен по той планке, что тогда действовала, и обязан таким остаться.
pub async fn freeze_steps_recent() {
    if crate::services::profile::get_steps_planka().is_none() {
        return;
    }
    let today = crate::services::local::today_date();
    for i in 1..=14 {
        let _ = day_cached("steps", &fmt(today - Duration::days(i))).await;
    }
}

/// Invalidate cached days affected by a change to `food_id` — every distinct diary
/// date that food appears on (via the diary `food_id` index). A change to a food
/// only ever affects the days it was eaten, so classifying/​editing a food logged
/// today invalidates only today (not a completed day) and the cache stays warm.
pub async fn invalidate_food(food_id: &str) {
    let entries: Vec<api_types::DiaryEntry> =
        crate::services::db::list_by_index("diary", "food_id", food_id).await;
    let dates: HashSet<String> = entries.into_iter().map(|e| e.date).collect();
    for d in dates {
        invalidate_day(&d).await;
    }
}

/// The daily target for `key` (0 → not computable yet, e.g. protein before the
/// profile/weight is set).
async fn target_for(key: &str) -> f64 {
    match key {
        "protein" => match local::list_weight_entries().await.into_iter().last() {
            Some(e) => profile::protein_target_from_profile(e.weight_kg).await as f64,
            None => 0.0,
        },
        "veg_fruit" => veg_fruit_per_day_g(),
        "steps" => crate::services::profile::get_steps_planka().unwrap_or(0.0),
        "calories" => local::calorie_goal_amount().await.unwrap_or(0.0),
        "calcium" => calcium_per_day_mg(),
        "fiber" => crate::services::fiber::daily_target_effective_g().await,
        _ => 0.0,
    }
}

/// Classifier metrics always have data → never Unknown. veg/fruit is derived from
/// tags (a day with none = 0 g). Steps too: a day with no logged steps counts as a
/// MISS (0 < planka), not grey — we're disciplining the user to log every day.
fn is_classifier(key: &str) -> bool {
    key == "veg_fruit" || key == "steps"
}

/// Indicator colour for `key` over the 7 COMPLETED days ending yesterday, read
/// through the per-day cache. Unknown (grey) when the target is unset or a nutrient
/// metric has no data yet.
/// Ключ индикатора → который из трёх жировых, если это он.
fn fat_key(key: &str) -> Option<crate::services::fats::Fat> {
    use crate::services::fats::Fat;
    match key {
        "epa_dha" => Some(Fat::EpaDha),
        "fat_ratio" => Some(Fat::Ratio),
        _ => None,
    }
}

pub async fn indicator_state(key: &str) -> IndicatorState {
    // Iron has its OWN weekly mechanics and its own week anchor — it never goes
    // through the 7-completed-days path below.
    if key == "iron" {
        return crate::services::iron::indicator_state().await;
    }
    // Гем считается порциями и своими неделями — тоже мимо дневного пути.
    if key == "heme" {
        return crate::services::heme::indicator_state().await;
    }
    // Жиры — три своих недельных индикатора, у каждого своя величина и своя норма.
    if let Some(which) = fat_key(key) {
        return crate::services::fats::indicator_state(which).await;
    }
    // Мясо: недельная планка граммов и дневная частота переработанного. Оба про
    // ограничение и оба считают своё — мимо общего дневного пути.
    if key == "red_meat" {
        return crate::services::red_meat::indicator_state().await;
    }
    // Яйца: недельная планка штук, считанных через белок, — тоже мимо дневного пути.
    if key == "egg" {
        return crate::services::egg::indicator_state().await;
    }
    // Клетчатка судится НЕДЕЛЕЙ: её норма привязана к калорийной планке, а сама она
    // приходит фоном — дневное «попал/не попал» о ней ничего не говорит.
    if key == "fiber" {
        return crate::services::fiber::indicator_state().await;
    }
    if key == "processed_meat" {
        return crate::services::processed_meat::indicator_state().await;
    }
    // Not evaluable yet (e.g. protein before the profile/weight is set).
    if target_for(key).await <= 0.0 {
        return IndicatorState::Unknown;
    }
    let today = crate::services::local::today_date();
    let days: Vec<NaiveDate> = (1..=7).map(|i| today - Duration::days(i)).collect();
    let mut misses = 0u32;
    let mut any_data = false;
    for d in &days {
        let (value, ratio) = day_cached(key, &fmt(*d)).await;
        if value > 0.0 {
            any_data = true;
        }
        // Colour off the FROZEN per-day pair (per-key miss rule: calories = the
        // ±band, the rest = ratio < 1.0), not a fresh compare to the current target.
        if day_missed(key, value, ratio) {
            misses += 1;
        }
    }
    if !is_classifier(key) && !any_data {
        return IndicatorState::Unknown;
    }
    daily_state(misses)
}

/// States for the currently-unlocked indicators, in display order (cached).
pub async fn unlocked_indicator_states() -> Vec<(&'static str, IndicatorState)> {
    let mut out = Vec::new();
    for key in displayed_indicators() {
        out.push((key, indicator_state(key).await));
    }
    out
}

/// Calorie-planka adherence over the last 7 COMPLETED days, as an indicator state.
/// AtMost semantics: a day is a MISS when intake went OVER that day's planka (the
/// opposite of the AtLeast nutrient indicators). Unknown until a planka exists.
/// Days with no logged food are skipped (not counted as adhered). Same bands as
/// [`daily_state`]: 0 over → green · 1–3 → orange · ≥4 → red.
pub async fn calorie_adherence_state() -> IndicatorState {
    if local::calorie_goal_amount().await.is_none() {
        return IndicatorState::Unknown;
    }
    let today = crate::services::local::today_date();
    let mut over = 0u32;
    for i in 1..=7 {
        let d = fmt(today - Duration::days(i));
        let Some(planka) = calorie_planka_on(&d).await else { continue };
        if planka <= 0.0 {
            continue;
        }
        let intake = local::kcal_on(&d).await;
        if intake <= 0.0 {
            continue; // no food logged that day — not an over-eat
        }
        if intake > planka {
            over += 1;
        }
    }
    daily_state(over)
}

/// The full indicator board for the curator food-share. It MUST match the colours
/// the user sees on their own widget, so every indicator goes through the exact
/// same `indicator_state` the widget uses — 7 COMPLETED days, TODAY EXCLUDED.
/// Including today, an in-progress day that hasn't met its target yet, once showed
/// veg-fruit / calcium as orange in the admin while the user's widget — excluding
/// today — showed green.
pub async fn share_states() -> Vec<(&'static str, IndicatorState)> {
    let mut out = Vec::new();
    // Calorie-planka adherence (also last 7 COMPLETED days).
    out.push(("calories", calorie_adherence_state().await));
    // Daily indicators — same method + window as the widget.
    for key in ["protein", "veg_fruit", "calcium", "fiber", "steps"] {
        out.push((key, indicator_state(key).await));
    }
    // Жиры — три своих недельных индикатора, каждый со своей нормой. Только когда
    // открыты: до этого у продуктов нет профиля, и куратор увидел бы «не ел ни
    // грамма» вместо «ещё не считается».
    if crate::services::fats::unlocked() {
        use crate::services::fats::Fat;
        for which in [Fat::EpaDha, Fat::Ratio] {
            out.push((which.key(), crate::services::fats::indicator_state(which).await));
        }
    }
    out
}

// ── "Keep them green" gate ───────────────────────────────────────────────────
// The widget nudges the user to keep EVERY unlocked indicator green for a full
// week: 7 GREEN days inside a rolling 8-day window (one day may slip). Counting
// begins the day BEFORE the indicators first appeared (the open date's yesterday,
// the earliest completed day we have on open) and then rolls forward over 8 days.

/// GREEN days required to clear the gate.
pub const GREEN_GATE_DAYS: u32 = 7;
/// Rolling window (in completed days) the required GREEN days must fall within.
const GREEN_GATE_WINDOW: i64 = 8;

/// App-flag holding the date (YYYY-MM-DD) the indicators first became visible.
const GATE_OPEN_KEY: &str = "ind_opened_at";

/// App-flag holding the date the STEP gate (activity week) opened — its rolling
/// window counts from here, so pre-planka days never count. Also the seed for the
/// WEEKLY steps-planka recompute clock (see `letters::maybe_recompute_weekly_steps_planka`),
/// so the first step-up lands one week after the planka was set.
pub(crate) const STEPS_GATE_OPEN_KEY: &str = "steps_gate_opened_at";

/// App-flag holding the date the CALCIUM gate opened — its rolling window counts
/// from here, so days before the calcium goal existed never count.
const CALCIUM_GATE_OPEN_KEY: &str = "calcium_gate_opened_at";

/// The date the indicators "opened" for this user — persisted the first time we
/// evaluate the gate, so the window is anchored to when the nudge began (not to
/// arbitrary earlier diary history).
fn gate_open_date(today: NaiveDate) -> NaiveDate {
    if let Some(s) = crate::services::app_flags::get(GATE_OPEN_KEY) {
        if let Ok(d) = NaiveDate::parse_from_str(&s, "%Y-%m-%d") {
            return d;
        }
    }
    crate::services::app_flags::set(GATE_OPEN_KEY, &fmt(today));
    today
}

/// Did EVERY unlocked indicator meet its target on `date` (per-key green rule:
/// calories = the ±band, the rest = frozen ratio ≥ 1.0)?
async fn all_green_on(date: &str) -> bool {
    for key in UNLOCKED_INDICATORS.iter().copied() {
        let (value, ratio) = day_cached(key, date).await;
        if !day_green(key, value, ratio) {
            return false;
        }
    }
    true
}

/// GREEN days accrued so far toward the gate: the number of completed days in the
/// rolling [`GREEN_GATE_WINDOW`]-day window (yesterday back) on which all unlocked
/// indicators were green — never counting days before the open date's yesterday.
/// Capped at [`GREEN_GATE_DAYS`] (the requirement). `== GREEN_GATE_DAYS` ⇒ cleared.
pub async fn green_gate_progress() -> u32 {
    let today = crate::services::local::today_date();
    let earliest = gate_open_date(today) - Duration::days(1);
    let mut green = 0u32;
    for i in 1..=GREEN_GATE_WINDOW {
        let d = today - Duration::days(i);
        if d < earliest {
            break;
        }
        if all_green_on(&fmt(d)).await {
            green += 1;
        }
    }
    green.min(GREEN_GATE_DAYS)
}

/// GREEN steps-days accrued toward the STEP gate (activity week): completed days
/// from the step-gate open date on which steps met the planka. Steps-only — its
/// own gate, independent of the protein/veg-fruit one. `== GREEN_GATE_DAYS` ⇒ the
/// activity week is cleared (→ next task).
pub async fn steps_gate_progress() -> u32 {
    if !activity_unlocked() {
        return 0;
    }
    let Some(s) = crate::services::app_flags::get(STEPS_GATE_OPEN_KEY) else {
        return 0;
    };
    let Ok(open) = NaiveDate::parse_from_str(&s, "%Y-%m-%d") else {
        return 0;
    };
    let today = crate::services::local::today_date();
    let mut green = 0u32;
    for i in 1..=GREEN_GATE_WINDOW {
        let d = today - Duration::days(i);
        if d < open {
            break; // never count days before the planka was set
        }
        let (_v, ratio) = day_cached("steps", &fmt(d)).await;
        if matches!(ratio, Some(r) if r >= 1.0) {
            green += 1;
        }
    }
    green.min(GREEN_GATE_DAYS)
}

/// GREEN calcium-days accrued toward the CALCIUM gate: completed days from the
/// calcium-gate open date on which calcium met its per-day target (1 g). Calcium
/// isn't cached (`day_cached` computes it live), and stays 0 until foods carry
/// calcium — so the gate only advances once calcium data actually appears.
pub async fn calcium_gate_progress() -> u32 {
    if !calcium_unlocked() {
        return 0;
    }
    let Some(s) = crate::services::app_flags::get(CALCIUM_GATE_OPEN_KEY) else {
        return 0;
    };
    let Ok(open) = NaiveDate::parse_from_str(&s, "%Y-%m-%d") else {
        return 0;
    };
    let today = crate::services::local::today_date();
    let mut green = 0u32;
    for i in 1..=GREEN_GATE_WINDOW {
        let d = today - Duration::days(i);
        if d < open {
            break; // never count days before the calcium goal was set
        }
        let (_v, ratio) = day_cached("calcium", &fmt(d)).await;
        if matches!(ratio, Some(r) if r >= 1.0) {
            green += 1;
        }
    }
    green.min(GREEN_GATE_DAYS)
}

/// One indicator's per-day history for the expanded view's histogram: the 7
/// COMPLETED days (oldest → newest). Each day carries `(date, value, ratio)`, where
/// `ratio` is the FROZEN `value / target` (see [`day_cached`]) — so the bar colours
/// don't shift when the target later changes. `missed` = days with `ratio < 1.0`.
#[derive(Clone)]
pub struct IndicatorSeries {
    pub key: &'static str,
    pub state: IndicatorState,
    pub days: Vec<(String, f64, Option<f64>)>,
    /// Per-day verdict for the bar colours, by THIS indicator's own rule
    /// (calories = the ±50 kcal band, the rest = ratio ≥ 1.0):
    /// Some(true) met · Some(false) missed · None unevaluable (no target).
    pub met_days: Vec<Option<bool>>,
    pub missed: u32,
    /// Подписи под столбиками. Пусто — подпись берётся из даты (день недели).
    /// У недельных индикаторов дни недели бессмысленны, там свои подписи.
    pub labels: Vec<String>,
}

/// Per-day series for every unlocked indicator (cached), for the histograms.
pub async fn unlocked_indicator_series() -> Vec<IndicatorSeries> {
    let today = crate::services::local::today_date();
    // Oldest → newest: today-7 … today-1.
    let dates: Vec<NaiveDate> = (1..=7).rev().map(|i| today - Duration::days(i)).collect();
    let mut out = Vec::new();
    for key in displayed_indicators() {
        // Железо судится НЕДЕЛЯМИ, поэтому и столбики у него недельные: восемь
        // завершённых недель, подписанные «−8 … −1» — сколько недель назад.
        // Дни недели под ними ничего не значат.
        if key == "iron" {
            out.push(crate::services::iron::weekly_series().await);
            continue;
        }
        if key == "heme" {
            out.push(crate::services::heme::weekly_series().await);
            continue;
        }
        if let Some(which) = fat_key(key) {
            out.push(crate::services::fats::weekly_series(which).await);
            continue;
        }
        // Красное мясо — недельная планка, столбики недельные. Переработанное —
        // дневное, но считается не через кэш вердиктов: величина там двоичная.
        if key == "red_meat" {
            out.push(crate::services::red_meat::weekly_series().await);
            continue;
        }
        if key == "egg" {
            out.push(crate::services::egg::weekly_series().await);
            continue;
        }
        if key == "fiber" {
            out.push(crate::services::fiber::weekly_series().await);
            continue;
        }
        if key == "processed_meat" {
            out.push(crate::services::processed_meat::daily_series().await);
            continue;
        }
        let mut days = Vec::with_capacity(dates.len());
        for d in &dates {
            let (value, ratio) = day_cached(key, &fmt(*d)).await;
            days.push((fmt(*d), value, ratio));
        }
        let missed = days
            .iter()
            .filter(|(_, value, ratio)| day_missed(key, *value, *ratio))
            .count() as u32;
        let met_days = days
            .iter()
            .map(|(_, value, ratio)| ratio.map(|_| day_green(key, *value, *ratio)))
            .collect();
        out.push(IndicatorSeries {
            key,
            state: indicator_state(key).await,
            days,
            met_days,
            missed,
            labels: Vec::new(),
        });
    }
    out
}

/// One daily gauge: TODAY's amount toward `target`, plus the indicator's state to
/// colour it. `state == Unknown` → grey (no data / target unset yet).
#[derive(Clone)]
pub struct DailyGauge {
    pub key: &'static str,
    pub value: f64, // eaten TODAY, in `unit`
    pub target: f64,
    pub unit: &'static str,
    pub state: IndicatorState,
}

fn unit_for(key: &str) -> &'static str {
    match key {
        "calcium" => "мг",
        _ => "г",
    }
}

/// Today's progress toward each UNLOCKED daily target, for the dashboard gauges.
/// The value is TODAY only (live); the colour is the indicator's 7-day state.
pub async fn daily_gauges() -> Vec<DailyGauge> {
    let today = fmt(crate::services::local::today_date());
    let mut out = Vec::new();
    for key in displayed_gauges() {
        out.push(DailyGauge {
            key,
            value: compute_day_value(key, &today).await,
            target: target_for(key).await,
            unit: unit_for(key),
            state: indicator_state(key).await,
        });
    }
    out
}



#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn daily_bands() {
        assert_eq!(daily_state(0), IndicatorState::Green);
        assert_eq!(daily_state(1), IndicatorState::Orange);
        assert_eq!(daily_state(3), IndicatorState::Orange);
        assert_eq!(daily_state(4), IndicatorState::Red);
        assert_eq!(daily_state(7), IndicatorState::Red);
    }

    /// The CALORIE indicator band, exactly per the product spec: planka 3000 →
    /// 2951…3049 is green; 2950/2948/3050 already miss. The frozen pair stores
    /// `(value, value/target)`, so the test feeds the same shape.
    #[test]
    fn calorie_band() {
        let pair = |v: f64, planka: f64| (v, Some(v / planka));
        let green = |(v, r): (f64, Option<f64>)| day_green("calories", v, r);
        assert!(green(pair(2951.0, 3000.0)));
        assert!(green(pair(3000.0, 3000.0)));
        assert!(green(pair(3049.0, 3000.0)));
        assert!(!green(pair(2950.0, 3000.0)));
        assert!(!green(pair(3050.0, 3000.0)));
        assert!(!green(pair(2948.0, 3000.0)));
        // No data / no target → never green, and a miss only when judgeable.
        assert!(!day_green("calories", 0.0, Some(0.0)));
        assert!(!day_green("calories", 3000.0, None));
        assert!(!day_missed("calories", 3000.0, None));
        assert!(day_missed("calories", 2900.0, Some(2900.0 / 3000.0)));
        // Other indicators keep the AtLeast rule.
        assert!(day_green("protein", 100.0, Some(1.0)));
        assert!(!day_green("protein", 90.0, Some(0.9)));
    }

    #[test]
    fn weekly_needs_a_completed_week() {
        // Идёт первая неделя — судить не по чему.
        assert_eq!(weekly_state(&[]), IndicatorState::Unknown);
    }

    #[test]
    fn poslednyaya_nedelya_zakryta_znachit_zelyonyj() {
        assert_eq!(weekly_state(&[true]), IndicatorState::Green);
        assert_eq!(weekly_state(&[true; 8]), IndicatorState::Green);
        // Прошлые провалы цвет не держат: неделя взята — индикатор зелёный.
        assert_eq!(weekly_state(&[false, false, false, true]), IndicatorState::Green);
    }

    #[test]
    fn odna_propushchennaya_nedelya_eto_oranzhevyj() {
        assert_eq!(weekly_state(&[false]), IndicatorState::Orange);
        assert_eq!(weekly_state(&[true, false]), IndicatorState::Orange);
        assert_eq!(weekly_state(&[false, false, true, false]), IndicatorState::Orange);
    }

    #[test]
    fn dve_propushchennye_podryad_eto_krasnyj() {
        assert_eq!(weekly_state(&[false, false]), IndicatorState::Red);
        assert_eq!(weekly_state(&[true, true, false, false]), IndicatorState::Red);
        // Порядок важен: свежий промах — последний в списке.
        assert_eq!(weekly_state(&[true; 6].iter().copied().chain([false, false]).collect::<Vec<_>>().as_slice()),
                   IndicatorState::Red);
    }

    #[test]
    fn weekly_ignores_the_week_in_progress() {
        // Восемь закрытых недель подряд — зелёный, что бы ни творилось на текущей:
        // она в историю не попадает вовсе.
        assert_eq!(weekly_state(&[true; 8]), IndicatorState::Green);
    }
}
