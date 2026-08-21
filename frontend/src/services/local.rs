use std::collections::BTreeMap;

use api_types::*;

use super::db;

pub(crate) fn now() -> String {
    chrono::Utc::now().to_rfc3339()
}

fn new_id() -> String {
    uuid::Uuid::now_v7().to_string()
}

/// Hour (local) at which a new logical/diary day begins. Entries logged BEFORE
/// this hour belong to the PREVIOUS day, so a 01:00 night snack lands on the day
/// it really belongs to instead of opening a fresh calendar day (which used to
/// swallow the following morning's breakfast into "ночной перекус"). The diary,
/// its date navigation and daily aggregations all pivot on this.
pub const DAY_START_HOUR: i64 = 4;

/// The current logical day: the calendar date shifted back by [`DAY_START_HOUR`],
/// so the day flips at 04:00 local rather than at midnight.
pub fn today_date() -> chrono::NaiveDate {
    (chrono::Local::now() - chrono::Duration::hours(DAY_START_HOUR)).date_naive()
}

/// The current logical day as "YYYY-MM-DD" (see [`today_date`]).
pub fn today() -> String {
    today_date().format("%Y-%m-%d").to_string()
}

fn time_now() -> String {
    chrono::Local::now().format("%H:%M").to_string()
}

// --- Foods ---

pub async fn list_foods() -> Vec<Food> {
    db::list_all("foods").await
}

pub async fn get_food(id: &str) -> Option<Food> {
    db::get("foods", id).await
}

pub async fn archive_food(id: &str, archived: bool) -> Option<Food> {
    let mut food: Food = db::get("foods", id).await?;
    food.archived = archived;
    food.updated_at = now();
    db::put("foods", &food).await;
    Some(food)
}

// --- Diary ---

pub async fn latest_diary_time_per_food() -> std::collections::BTreeMap<String, String> {
    let entries: Vec<DiaryEntry> = db::list_all("diary").await;
    let mut map = std::collections::BTreeMap::new();
    for e in entries {
        if e.deleted { continue; }
        map.entry(e.food_id)
            .and_modify(|existing: &mut String| {
                if e.created_at > *existing { *existing = e.created_at.clone(); }
            })
            .or_insert(e.created_at);
    }
    map
}

pub async fn list_diary_range(dates: &[String]) -> Vec<DiaryEntry> {
    let mut all = Vec::new();
    for d in dates {
        let entries: Vec<DiaryEntry> = db::list_by_index("diary", "date", d).await;
        all.extend(entries.into_iter().filter(|e| !e.deleted));
    }
    all
}

pub async fn list_diary(date: &str) -> Vec<DiaryEntry> {
    let entries: Vec<DiaryEntry> = db::list_by_index("diary", "date", date).await;
    entries.into_iter().filter(|e| !e.deleted).collect()
}

/// All distinct dates with at least one non-deleted diary entry.
pub async fn list_diary_dates() -> Vec<String> {
    let entries: Vec<DiaryEntry> = db::list_all("diary").await;
    entries
        .into_iter()
        .filter(|e| !e.deleted)
        .map(|e| e.date)
        .collect()
}

/// Per-day total effective kcal over the last `window_days` calendar days ending
/// today, OLDEST first (`(date, kcal)`). Days with no diary entries are `0.0`
/// (drawn as an empty slot). Same per-entry formula as [`avg_daily_kcal`] — honours
/// waste and the restaurant surcharge — so the chart matches the diary totals.
pub async fn daily_kcal_series(window_days: i64) -> Vec<(String, f64)> {
    // ONE read of the whole diary + in-memory bucketing (not `window_days`
    // separate index queries) so the chart loads in a single frame.
    let all: Vec<DiaryEntry> = db::list_all("diary").await;
    // Only the foods actually referenced by these entries (a slice, not the table).
    let foods = foods_by_ids(all.iter().filter(|e| !e.deleted).map(|e| e.food_id.clone())).await;
    let mut by_date: std::collections::HashMap<String, f64> = std::collections::HashMap::new();
    for e in &all {
        if e.deleted {
            continue;
        }
        if let Some(food) = foods.get(&e.food_id) {
            let eaten = (e.grams - e.waste_grams).max(0.0);
            *by_date.entry(e.date.clone()).or_insert(0.0) += food.effective_kcal() * eaten / 100.0;
        }
    }
    let today = chrono::Local::now().date_naive();
    (0..window_days)
        .rev()
        .map(|i| {
            let d = (today - chrono::Duration::days(i)).format("%Y-%m-%d").to_string();
            let kc = by_date.get(&d).copied().unwrap_or(0.0);
            (d, kc)
        })
        .collect()
}

/// Total effective kcal eaten on `date` (same per-entry formula as the diary).
pub async fn kcal_on(date: &str) -> f64 {
    let entries = list_diary(date).await;
    let foods = foods_by_ids(entries.iter().map(|e| e.food_id.clone())).await;
    entries
        .iter()
        .filter_map(|e| {
            foods
                .get(&e.food_id)
                .map(|food| food.effective_kcal() * (e.grams - e.waste_grams).max(0.0) / 100.0)
        })
        .sum()
}

/// Average daily effective kcal over the last `window_days` days — see
/// [`daily_kcal_totals`] for what counts as a day.
pub async fn avg_daily_kcal(window_days: i64) -> Option<f64> {
    let totals = daily_kcal_totals(window_days).await;
    if totals.is_empty() {
        return None;
    }
    Some(totals.iter().sum::<f64>() / totals.len() as f64)
}

/// Средняя, максимальная и минимальная калорийность дня за окно — то, что письмо
/// о первой планке показывает человеку. Считается по тем же дням, что и средняя:
/// разойтись они не должны, иначе в письме будет максимум из дня, которого нет в
/// расчёте планки.
pub async fn daily_kcal_stats(window_days: i64) -> Option<(f64, f64, f64)> {
    let totals = daily_kcal_totals(window_days).await;
    if totals.is_empty() {
        return None;
    }
    let avg = totals.iter().sum::<f64>() / totals.len() as f64;
    let max = totals.iter().cloned().fold(f64::MIN, f64::max);
    let min = totals.iter().cloned().fold(f64::MAX, f64::min);
    Some((avg, max, min))
}

/// Per-day effective kcal over the last `window_days` calendar days, counting
/// ONLY days that have diary entries. Per-day kcal is the sum of each entry's
/// `effective_kcal() * (grams - waste_grams).max(0) / 100` — exactly how the
/// diary/summary computes calories (honouring waste and the restaurant
/// surcharge). Empty when no day in the window has any diary entry.
pub async fn daily_kcal_totals(window_days: i64) -> Vec<f64> {
    let today = chrono::Local::now().date_naive();
    // Only COMPLETED days: today is still in progress (maybe just breakfast so
    // far), and averaging a partial day would drag the mean down. So the window
    // is the `window_days` days ENDING YESTERDAY (today-1 … today-window_days).
    let dates: Vec<String> = (1..=window_days)
        .map(|i| (today - chrono::Duration::days(i)).format("%Y-%m-%d").to_string())
        .collect();

    // Diaries for the days that have entries, then only the foods they reference.
    let mut diaries: Vec<Vec<DiaryEntry>> = Vec::new();
    for d in &dates {
        let diary = list_diary(d).await;
        if !diary.is_empty() {
            diaries.push(diary);
        }
    }
    if diaries.is_empty() {
        return Vec::new();
    }
    let foods = foods_by_ids(diaries.iter().flatten().map(|e| e.food_id.clone())).await;

    diaries
        .iter()
        .map(|diary| {
            diary
                .iter()
                .filter_map(|e| {
                    foods
                        .get(&e.food_id)
                        .map(|food| food.effective_kcal() * (e.grams - e.waste_grams).max(0.0) / 100.0)
                })
                .sum()
        })
        .collect()
}

// ── Careful calorie-planka control loop ──────────────────────────────────────
// `calorie_planka(base, trend, weight)` = `base` nudged by AT MOST one small step
// (±5%) from the weight trend. The BASE differs by caller:
//   • FIRST planka (ch3 «Рассчитать») — base = average intake (calibrate to how
//     much the user actually eats). See `calorie_planka_suggestion`.
//   • WEEKLY recompute — base = the PREVIOUS planka (`letters::maybe_recompute…`),
//     so the target moves at most ±5%/week and a low-intake week (e.g. anxiety
//     undereating) can NOT ratchet it down; only a confirmed weight trend moves it.
// The step is cut ONLY when justified — so a slow, comfortable weight loss is
// never disrupted by a premature reduction:
//   • confident loss, rate inside the comfortable band → HOLD;
//   • confident loss but too SLOW → −5% (gently speed up toward the band);
//   • confident loss but too FAST → +5% (protect comfort / muscle);
//   • probably-but-not-confidently losing → HOLD, gather another week;
//   • flat / gaining → −5% (induce a deficit);
//   • no usable trend yet (week 1 / too few weigh-ins) → HOLD (baseline = average).
//
// И ПОВЕРХ ЭТОГО — второй контур, `calorie_planka_weekly`: шаг, к которому зовёт
// вес, разрешается только если человек планку ИСПОЛНЯЛ. Один лишь вес образует
// положительную обратную связь — недоедание разгоняет похудение, похудение поднимает
// планку, планка отдаляется от того, что человек ест, и так по кругу с растущим
// разрывом. Подробный разбор петли — в доке к `calorie_planka_weekly`.

/// Comfortable weekly weight-loss rate, as a FRACTION of body weight.
const COMFORT_LOSS_MIN: f64 = 0.003; // 0.3 %/week
const COMFORT_LOSS_MAX: f64 = 0.007; // 0.7 %/week
/// The largest single-step planka change per weekly recompute.
const PLANKA_STEP: f64 = 0.05; // ±5 %

/// Multiplier applied to the average intake, chosen from the weight trend +
/// current body weight. Pure (no I/O) so it is unit-tested. See the block comment.
pub fn planka_factor(trend: &crate::services::weight_trend::WeightTrend, weight_kg: f64) -> f64 {
    use crate::services::weight_trend::{Direction, WeightTrend, CONFIDENT, WEAK};
    // `p_down` = probability the weight is genuinely FALLING; `slope_wk` in kg/week.
    let (p_down, slope_wk) = match *trend {
        WeightTrend::Estimated { direction, confidence, slope_kg_per_week, .. } => {
            let p = match direction {
                Direction::Down => confidence,
                Direction::Up => 1.0 - confidence,
            };
            (p, slope_kg_per_week)
        }
        // < 3 distinct weigh-days: a sign exists but no confidence — HOLD, never cut on noise.
        WeightTrend::Tentative { .. } | WeightTrend::Insufficient { .. } => return 1.0,
    };

    if p_down >= CONFIDENT {
        // Confidently losing → steer toward the comfortable-rate band.
        if weight_kg <= 0.0 {
            return 1.0;
        }
        let rate = slope_wk.abs() / weight_kg; // fraction of body weight lost per week
        if rate < COMFORT_LOSS_MIN {
            1.0 - PLANKA_STEP // too slow → gentle cut
        } else if rate > COMFORT_LOSS_MAX {
            1.0 + PLANKA_STEP // too fast → ease up
        } else {
            1.0 // comfortable → hold
        }
    } else if p_down >= WEAK {
        1.0 // probably losing but not confirmed → HOLD, wait another week (don't cut prematurely)
    } else {
        1.0 - PLANKA_STEP // flat / gaining → induce a deficit
    }
}

/// The daily calorie planka: the average intake nudged by [`planka_factor`] and
/// rounded to the nearest 50 kcal. Pure (no I/O) so it is unit-testable.
pub fn calorie_planka(
    avg_kcal: f64,
    trend: &crate::services::weight_trend::WeightTrend,
    weight_kg: f64,
) -> f64 {
    ((avg_kcal * planka_factor(trend, weight_kg)) / 50.0).round() * 50.0
}

/// Как человек ПРОЖИЛ неделю относительно своей планки.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Adherence {
    /// Ел меньше планки.
    Under,
    /// Ел больше планки.
    Over,
    /// Держался планки — в пределах той же погрешности, по которой день считается
    /// зелёным.
    OnTarget,
}

/// Куда человек отклонился от планки за неделю: средняя дневная калорийность
/// против планки, порог — тот же `band`, что делает день зелёным (±50 ккал).
pub fn adherence(avg_kcal: f64, planka: f64, band: f64) -> Adherence {
    if avg_kcal < planka - band {
        Adherence::Under
    } else if avg_kcal > planka + band {
        Adherence::Over
    } else {
        Adherence::OnTarget
    }
}

/// Недельный пересчёт планки: прежняя планка, сдвинутая трендом веса, НО с оглядкой
/// на то, исполнялась ли она.
///
/// # Зачем это правило
///
/// Планка, которую двигает ОДИН ТОЛЬКО вес, образует положительную обратную связь —
/// петлю, которая сама себя разгоняет:
///
/// 1. человек ест заметно меньше планки (тревожная неделя, болезнь, стресс, просто
///    не до еды);
/// 2. вес от этого падает быстро — быстрее комфортной полосы;
/// 3. правило по весу читает это как «слишком быстро худеет» и ПОДНИМАЕТ планку,
///    чтобы поберечь мышцы;
/// 4. человек ест столько же, сколько ел, — то есть теперь ещё дальше от планки;
/// 5. вес продолжает падать так же быстро → планка поднимается снова.
///
/// С каждой неделей разрыв между тем, что человек ест, и тем, что ему предписано,
/// РАСТЁТ, и планка тем безумнее, чем хуже человеку. Никакой обратной силы, которая
/// вернула бы её к реальности, в этой петле нет: вес подтверждает подъём на каждом
/// круге. То же самое зеркально — перебор при стоящем весе тянет планку вниз, к
/// цифре, которую человек и не пробовал выполнять, и невыполнимость только растёт.
///
/// Разорвать петлю можно ровно в одном месте: перестать двигать планку туда, куда
/// зовёт вес, когда причина движения веса — НЕИСПОЛНЕНИЕ планки, а не её величина.
/// Пока человек не ест по планке, вес не говорит о планке ничего: он говорит о том,
/// сколько человек ест на самом деле.
///
/// # Как именно
///
/// Исполнение работает СТОПОРОМ, а не ещё одним слагаемым: недоедающему планку не
/// поднимаем, переедающему не опускаем. Стопор односторонний — в противоположную
/// сторону планка ходит свободно: недоедающему её МОЖНО опустить (если вес стоит
/// даже при недоедании, дело не в дисциплине), переедающему — поднять.
///
/// Держать на месте можно всегда: это единственное решение, которое не требует от
/// человека того, чего он на прошлой неделе не делал.
pub fn calorie_planka_weekly(
    previous: f64,
    trend: &crate::services::weight_trend::WeightTrend,
    weight_kg: f64,
    adherence: Adherence,
) -> f64 {
    let factor = match adherence {
        Adherence::Under => planka_factor(trend, weight_kg).min(1.0),
        Adherence::Over => planka_factor(trend, weight_kg).max(1.0),
        Adherence::OnTarget => planka_factor(trend, weight_kg),
    };
    ((previous * factor) / 50.0).round() * 50.0
}

/// The suggested daily calorie planka shown (and accepted) in ch3: average intake
/// over the last 7 COMPLETED days (today excluded — it's still in progress), nudged
/// by the weight trend + latest weight via [`calorie_planka`] (see the block
/// comment — hold while comfortably/probably losing, small −5% only when plateaued).
/// `None` when there are no logged days yet. Single source of truth so the widget's
/// displayed figure and the value it accepts cannot drift apart.
pub async fn calorie_planka_suggestion() -> Option<f64> {
    use crate::services::weight_trend::{self, DEFAULT_WINDOW_DAYS};
    let avg = avg_daily_kcal(7).await?;
    let entries = list_weight_entries().await;
    let trend = weight_trend::weight_trend(&entries, DEFAULT_WINDOW_DAYS);
    // Latest weigh-in — for the %-of-body-weight loss rate.
    let weight_kg = entries
        .iter()
        .max_by(|a, b| a.date.cmp(&b.date))
        .map(|e| e.weight_kg)
        .unwrap_or(0.0);
    Some(calorie_planka(avg, &trend, weight_kg))
}

/// The currently-set daily calorie planka (the `Calories`/`AtMost` goal), if any.
pub async fn calorie_goal_amount() -> Option<f64> {
    list_goals()
        .await
        .into_iter()
        .find(|g| g.nutrient == "Calories" && g.direction == GoalDirection::AtMost && g.amount > 0.0)
        .map(|g| g.amount)
}

// ── История планок ───────────────────────────────────────────────────────────
//
// Планка движется: калорийную раз в неделю пересчитывает тренд веса, шаговую
// поднимает цвет индикатора. Пока хранилось только текущее значение, вопрос «какая
// планка была во вторник» оставался без ответа, и день, засчитанный тогда,
// приходилось либо замораживать, либо пересуживать по сегодняшней — то есть
// переписывать прошлое. Живой случай: планка шагов выросла с 10800 до 11800, и
// пятница с 11500 шагами из выполненной стала недобором.
//
// Хранятся только планки, НЕ зависящие от личности, — калории и шаги. Нормы белка,
// овощей, кальция выводятся из профиля и истории не требуют.

/// Виды планок, у которых есть история.
pub const PLANKA_CALORIES: &str = "calories";
pub const PLANKA_STEPS: &str = "steps";
/// Норма белка. Выводится из профиля, но зависит от ВЕСА — а он меняется, и вместе
/// с ним норма. День, закрытый по норме 120 г, не должен становиться недобором,
/// когда после похудения норма стала 115.
pub const PLANKA_PROTEIN: &str = "protein";

/// Записать установку планки, действующую С СЕГОДНЯШНЕГО дня.
///
/// Одна запись на вид и день: повторная установка в тот же день перезаписывает её,
/// а не плодит вторую. Ничего не делает, если значение совпадает с уже действующим —
/// история хранит ИЗМЕНЕНИЯ, а не каждое касание.
pub async fn record_planka(kind: &str, amount: f64) {
    if amount <= 0.0 {
        return;
    }
    let date = today_date().format("%Y-%m-%d").to_string();
    if planka_on(kind, &date).await == Some(amount) {
        return;
    }
    let id = format!("{kind}:{date}");
    let created_at = db::get::<PlankaEntry>("planka_history", &id)
        .await
        .map(|e| e.created_at)
        .unwrap_or_else(now);
    db::put(
        "planka_history",
        &PlankaEntry {
            id,
            kind: kind.to_string(),
            date,
            amount,
            created_at,
            updated_at: now(),
        },
    )
    .await;
}

/// Записать установку планки ЗАДНИМ ЧИСЛОМ, с указанной даты. Только для миграции,
/// заводящей историю из уже существующих значений: обычная установка всегда
/// датируется сегодняшним днём ([`record_planka`]).
pub async fn seed_planka_at(kind: &str, date: &str, amount: f64) {
    if amount <= 0.0 || date.len() < 10 {
        return;
    }
    let id = format!("{kind}:{date}");
    if db::get::<PlankaEntry>("planka_history", &id).await.is_some() {
        return;
    }
    db::put(
        "planka_history",
        &PlankaEntry {
            id,
            kind: kind.to_string(),
            date: date.to_string(),
            amount,
            created_at: now(),
            updated_at: now(),
        },
    )
    .await;
}

/// Вся история планок этого вида, по возрастанию даты.
pub async fn planka_history(kind: &str) -> Vec<PlankaEntry> {
    let mut all: Vec<PlankaEntry> = db::list_by_index("planka_history", "kind", kind).await;
    all.sort_by(|a, b| a.date.cmp(&b.date));
    all
}

/// Планка, ДЕЙСТВОВАВШАЯ на `date` — последняя установка не позже этого дня.
///
/// `None`, если на тот день планки ещё не было: день до первой установки судить
/// нечем, и это не провал, а отсутствие правила.
pub async fn planka_on(kind: &str, date: &str) -> Option<f64> {
    planka_history(kind)
        .await
        .into_iter()
        .take_while(|e| e.date.as_str() <= date)
        .last()
        .map(|e| e.amount)
}

/// Progress toward the "one week of observations" the planka needs: for each of
/// food / weight / steps, how many distinct days have an entry, capped at 7.
///
/// FOOD counts only the 7 COMPLETED days (yesterday … 7 days ago). Today is a
/// PARTIAL day and is deliberately EXCLUDED — it's exactly the window the calorie
/// planka averages over, so the "calculate" button only unlocks once there are
/// seven FULL days of food (a single item logged today must NOT complete the week).
///
/// WEIGHT/STEPS use the last 8 days (today included). Why 8, not 7: weight is a
/// TODAY morning measurement while steps are logged for the COMPLETED previous day —
/// so a diligent user's steps trail weight by a day, and the extra day absorbs that
/// one-day offset so consecutive logging still reaches 7/7 for both.
pub async fn progress_week_counts() -> (u32, u32, u32) {
    let today = chrono::Local::now().date_naive();
    // Food: the 7 completed days ending yesterday (today excluded).
    let food_window: std::collections::BTreeSet<chrono::NaiveDate> =
        (1..=7).map(|i| today - chrono::Duration::days(i)).collect();
    // Weight/steps: the last 8 days (today included).
    let ws_window: std::collections::BTreeSet<chrono::NaiveDate> =
        (0..8).map(|i| today - chrono::Duration::days(i)).collect();
    let count = |dates: &[String], window: &std::collections::BTreeSet<chrono::NaiveDate>| -> u32 {
        dates
            .iter()
            .filter_map(|d| chrono::NaiveDate::parse_from_str(d, "%Y-%m-%d").ok())
            .filter(|d| window.contains(d))
            .collect::<std::collections::BTreeSet<_>>()
            .len()
            .min(7) as u32
    };
    let food = count(&list_diary_dates().await, &food_window);
    let weight_dates = list_weight_entries().await.into_iter().map(|e| e.date).collect::<Vec<_>>();
    let steps_dates = list_step_entries().await.into_iter().map(|e| e.date).collect::<Vec<_>>();
    let weight = count(&weight_dates, &ws_window);
    let steps = count(&steps_dates, &ws_window);
    (food, weight, steps)
}

/// True if `food` is a vegetable / fruit — by the cached AI tag. `None` counts as
/// not-veg-fruit until classified in the background.
pub fn is_veg_fruit_food(food: &Food) -> bool {
    food.is_veg_fruit == Some(true)
}

/// Какой признак продукта записывается. Спрашиваются они порознь, поэтому и
/// пишутся порознь: ответ про один признак не трогает соседние поля.
#[derive(Debug, Clone, Copy)]
pub enum FoodFlag {
    VegFruit,
    Heme,
    /// Жир заключён в целую молочно-жировую глобулу — см. `Food::is_milk_globule`.
    MilkGlobule,
    /// Мышечная ткань млекопитающих — см. `Food::is_red_meat`.
    RedMeat,
    /// Мясо, консервированное ради хранения — см. `Food::is_processed_meat`.
    ProcessedMeat,
    /// Яйцо птицы — см. `Food::is_egg`.
    Egg,
}

/// Записать ОДИН признак продукта и толкнуть синк, чтобы он доехал до других
/// устройств. Продукт, которого уже нет, пропускается.
pub async fn cache_food_flag(id: &str, flag: FoodFlag, value: bool) {
    let Some(mut food) = db::get::<Food>("foods", id).await else { return };
    match flag {
        FoodFlag::VegFruit => food.is_veg_fruit = Some(value),
        FoodFlag::Heme => food.is_heme = Some(value),
        FoodFlag::MilkGlobule => food.is_milk_globule = Some(value),
        FoodFlag::RedMeat => food.is_red_meat = Some(value),
        FoodFlag::ProcessedMeat => food.is_processed_meat = Some(value),
        FoodFlag::Egg => food.is_egg = Some(value),
    }
    food.updated_at = now();
    db::put("foods", &food).await;
    // Признак изменился → пересчитать надо только те дни, в которые ели ЭТОТ продукт.
    crate::services::indicators::invalidate_food(id).await;
    crate::services::sync::push_background();
}

/// Merge background-enriched nutrient values (name → per-100g amount, canonical
/// unit) into a food and push. Written by the background enricher; overwrites the
/// same keys idempotently.
/// Сохранить найденные значения нутриентов. Ноль — полноценное значение: он
/// означает «в этом продукте столько», а не «не нашли». Иначе фоновый проход
/// запрашивал бы такой нутриент при каждом запуске приложения.
pub async fn cache_food_nutrients(id: &str, values: BTreeMap<String, f64>) {
    if values.is_empty() {
        return;
    }
    if let Some(mut food) = db::get::<Food>("foods", id).await {
        for (k, v) in values {
            food.nutrients.insert(k, v);
        }
        food.updated_at = now();
        db::put("foods", &food).await;
        // Блюда с этим продуктом считаются из состава — значит устарели.
        recompute_recipes_using(id).await;
        // Nutrient values changed → only the days this food was eaten on need
        // re-summarizing.
        crate::services::indicators::invalidate_food(id).await;
        crate::services::sync::push_background();
    }
}

/// Store a food's IRON verdict (mg per 100 g + absorbed fraction) from the
/// dedicated iron pass. Deliberately separate from [`cache_food_nutrients`]:
/// iron lives in its own fields, never in the `nutrients` map that forms and
/// badges render.
pub async fn cache_food_iron(id: &str, iron_mg: f64, absorption: f64) {
    if let Some(mut food) = db::get::<Food>("foods", id).await {
        food.iron_mg = Some(iron_mg);
        food.iron_absorption = Some(absorption);
        food.updated_at = now();
        db::put("foods", &food).await;
        // Блюда с этим продуктом считаются из состава — значит устарели.
        recompute_recipes_using(id).await;
        crate::services::sync::push_background();
    }
}

/// Fetch ONLY the foods with these ids (deduped), each by primary key — a bounded
/// slice, never the whole `foods` table. Missing ids are skipped. Every per-day
/// aggregate loads just the handful of foods its own diary slice references, so
/// cost scales with what was eaten that day, not with the size of the foods table.
pub(crate) async fn foods_by_ids(ids: impl IntoIterator<Item = String>) -> BTreeMap<String, Food> {
    let unique: std::collections::HashSet<String> = ids.into_iter().collect();
    let mut map = BTreeMap::new();
    for id in unique {
        if let Some(f) = db::get::<Food>("foods", &id).await {
            map.insert(id, f);
        }
    }
    map
}

/// Evening protein (grams) on `date`: sum of `protein * eaten_grams / 100`
/// (honouring waste, like the diary) over entries whose derived meal bucket is
/// Dinner or NightSnack.
pub async fn evening_protein_on(date: &str) -> f64 {
    let diary = list_diary(date).await;
    let foods = foods_by_ids(diary.iter().map(|e| e.food_id.clone())).await;
    let groups = crate::services::meal_split::group_by_meal(&diary);
    use crate::services::meal_split::MealType;
    let mut total = 0.0;
    for g in groups {
        if g.meal != MealType::Dinner && g.meal != MealType::NightSnack {
            continue;
        }
        for e in &g.entries {
            if let Some(food) = foods.get(&e.food_id) {
                let eaten = (e.grams - e.waste_grams).max(0.0);
                total += food.protein * eaten / 100.0;
            }
        }
    }
    total
}

/// EATEN grams of foods matching `selector` on `date`, honouring waste and
/// **expanding recipes by composition**: for a logged dish (a food with a
/// `recipe_id`), each matching ingredient contributes `ing.grams × eaten /
/// recipe.total_grams` — so e.g. the egg content of 200 g of a cake is counted from
/// the recipe's egg ingredient, not from classifying the whole cake. Plain foods
/// contribute their full eaten grams when they match. Ingredients are treated as
/// leaf foods (no nested-recipe recursion in v1).
pub async fn food_tag_grams_on(date: &str, selector: impl Fn(&Food) -> bool) -> f64 {
    let entries = list_diary(date).await;
    // The day's own foods (needed to know which entries are recipe dishes).
    let mut foods = foods_by_ids(entries.iter().map(|e| e.food_id.clone())).await;

    // Only the recipes those dishes reference, and only their ingredients (by
    // index) — never the whole recipes/ingredients tables.
    let recipe_ids: std::collections::HashSet<String> =
        foods.values().filter_map(|f| f.recipe_id.clone()).collect();
    let mut recipes: BTreeMap<String, Recipe> = BTreeMap::new();
    let mut ings_by_recipe: BTreeMap<String, Vec<RecipeIngredient>> = BTreeMap::new();
    for rid in &recipe_ids {
        if let Some(r) = db::get::<Recipe>("recipes", rid).await {
            recipes.insert(rid.clone(), r);
        }
        ings_by_recipe.insert(
            rid.clone(),
            db::list_by_index("recipe_ingredients", "recipe_id", rid).await,
        );
    }
    // Leaf ingredient foods, so the selector can be evaluated on them too.
    let ing_ids: Vec<String> = ings_by_recipe.values().flatten().map(|i| i.food_id.clone()).collect();
    for (id, f) in foods_by_ids(ing_ids).await {
        foods.entry(id).or_insert(f);
    }

    let mut total = 0.0;
    for e in entries {
        let Some(food) = foods.get(&e.food_id) else { continue };
        let eaten = (e.grams - e.waste_grams).max(0.0);
        if eaten <= 0.0 {
            continue;
        }
        match food.recipe_id.as_ref().and_then(|rid| recipes.get(rid)) {
            // Logged dish → attribute matching ingredients by their share of the
            // finished (cooked) weight.
            Some(recipe) if recipe.total_grams.unwrap_or(0.0) > 0.0 => {
                let tg = recipe.total_grams.unwrap();
                if let Some(ings) = ings_by_recipe.get(&recipe.id) {
                    let matched_raw: f64 = ings.iter()
                        .filter(|ing| foods.get(&ing.food_id).is_some_and(|f| selector(f)))
                        .map(|ing| ing.grams)
                        .sum();
                    total += matched_raw * eaten / tg;
                }
            }
            // Plain food.
            _ => {
                if selector(food) {
                    total += eaten;
                }
            }
        }
    }
    total
}

/// БЕЛОК (г), пришедший на `date` из продуктов, подходящих под `selector`, с тем же
/// раскрытием блюд по составу, что и [`food_tag_grams_on`].
///
/// Отдельно от граммов продукта: граммы несопоставимы между собой (100 г печени и
/// 100 г мясного бульона — разные вещи), а белок сопоставим и заодно сам учитывает
/// уварку — в готовом продукте его на 100 г больше.
pub async fn tag_protein_g_on(date: &str, selector: impl Fn(&Food) -> bool) -> f64 {
    let entries = list_diary(date).await;
    let mut foods = foods_by_ids(entries.iter().map(|e| e.food_id.clone())).await;

    let recipe_ids: std::collections::HashSet<String> =
        foods.values().filter_map(|f| f.recipe_id.clone()).collect();
    let mut recipes: BTreeMap<String, Recipe> = BTreeMap::new();
    let mut ings_by_recipe: BTreeMap<String, Vec<RecipeIngredient>> = BTreeMap::new();
    for rid in &recipe_ids {
        if let Some(r) = db::get::<Recipe>("recipes", rid).await {
            recipes.insert(rid.clone(), r);
        }
        ings_by_recipe.insert(
            rid.clone(),
            db::list_by_index("recipe_ingredients", "recipe_id", rid).await,
        );
    }
    let ing_ids: Vec<String> = ings_by_recipe.values().flatten().map(|i| i.food_id.clone()).collect();
    for (id, f) in foods_by_ids(ing_ids).await {
        foods.entry(id).or_insert(f);
    }

    let mut total = 0.0;
    for e in entries {
        let Some(food) = foods.get(&e.food_id) else { continue };
        let eaten = (e.grams - e.waste_grams).max(0.0);
        if eaten <= 0.0 {
            continue;
        }
        match food.recipe_id.as_ref().and_then(|rid| recipes.get(rid)) {
            Some(recipe) if recipe.total_grams.unwrap_or(0.0) > 0.0 => {
                let tg = recipe.total_grams.unwrap();
                if let Some(ings) = ings_by_recipe.get(&recipe.id) {
                    for ing in ings {
                        let Some(f) = foods.get(&ing.food_id) else { continue };
                        if !selector(f) {
                            continue;
                        }
                        // Белок ингредиента, попавший в съеденную часть блюда.
                        total += f.protein * ing.grams / 100.0 * eaten / tg;
                    }
                }
            }
            _ => {
                if selector(food) {
                    total += food.protein * eaten / 100.0;
                }
            }
        }
    }
    total
}

/// EATEN grams of vegetable/fruit on `date` (recipe-composition aware).
pub async fn veg_fruit_grams_on(date: &str) -> f64 {
    food_tag_grams_on(date, is_veg_fruit_food).await
}

/// Жирные кислоты за день, в граммах.
///
/// Состав блюда здесь НЕ раскрывается — и это не упущение: профиль блюда уже
/// посчитан из состава (`recipe_nutrition`) и лежит на самом блюде, как и его
/// железо. Раскрывать второй раз значило бы считать одно и то же двумя путями.
///
/// Продукт без профиля не вносит ничего: у него не «нет кислот», а «мы пока не
/// знаем», и подставлять ноль нельзя.
pub async fn fatty_acids_on(date: &str) -> FattyAcids {
    acids_on(date, Food::fatty_acids_per_100g).await
}

/// Кислоты за день ДЛЯ ИНДИКАТОРА БАЛАНСА: без жира, запертого в целой
/// молочно-жировой глобуле.
///
/// Мембрана глобулы меняет судьбу тех же самых жирных кислот: сыр против масла
/// при одинаковом составе даёт другой ответ по ЛПНП. Считать их наравне значит
/// требовать от человека компенсировать то, что компенсации не требует, — а
/// обезжиренный творог с его тремя граммами жира ещё и не выкинуть.
///
/// Убирается ТОЛЬКО из баланса. Омега-3 такой оговорки не знает: её в молочном
/// жире всё равно нет, и вычитать там нечего.
pub async fn balance_acids_on(date: &str) -> FattyAcids {
    acids_on(date, Food::balance_acids_per_100g).await
}

/// Общий сумматор: кислоты съеденного за день. `per100` решает, ЧТО брать у
/// продукта — весь его жир или только участвующий в балансе; вернувший `None`
/// пропускается, потому что «не знаем» это не «ноль».
async fn acids_on(date: &str, per100: impl Fn(&Food) -> Option<FattyAcids>) -> FattyAcids {
    let entries = list_diary(date).await;
    let foods = foods_by_ids(entries.iter().map(|e| e.food_id.clone())).await;
    let mut total = FattyAcids::default();
    for e in &entries {
        let Some(food) = foods.get(&e.food_id) else { continue };
        let Some(acids) = per100(food) else { continue };
        let eaten = (e.grams - e.waste_grams).max(0.0);
        if eaten <= 0.0 {
            continue;
        }
        total += acids.scaled(eaten / 100.0);
    }
    total
}

/// Записать профиль жира продукта. Блюда с этим продуктом в составе устаревают —
/// пересчёт идёт сразу, как у железа.
pub async fn cache_food_fat_profile(id: &str, profile: FatProfile) {
    let Some(mut food) = db::get::<Food>("foods", id).await else { return };
    food.fat_profile = Some(profile);
    food.updated_at = now();
    db::put("foods", &food).await;
    recompute_recipes_using(id).await;
    crate::services::sync::push_background();
}

/// Total amount of the custom nutrient `key` eaten on `date` — sum of
/// `food.nutrients[key] * eaten_grams / 100` over the day's entries. Recipe dishes
/// already carry the SUMMED ingredient nutrients per 100 g (see `finish_recipe`),
/// so no composition expansion is needed here. Unit is whatever the nutrient uses.
pub async fn nutrient_grams_on(date: &str, key: &str) -> f64 {
    let entries = list_diary(date).await;
    let foods = foods_by_ids(entries.iter().map(|e| e.food_id.clone())).await;
    entries.iter().filter_map(|e| {
        foods.get(&e.food_id).and_then(|f| f.nutrients.get(key)).map(|v| {
            let eaten = (e.grams - e.waste_grams).max(0.0);
            v * eaten / 100.0
        })
    }).sum()
}

/// Total protein (grams) eaten on `date` — sum of `protein * eaten_grams / 100`
/// over the day's diary entries (honouring waste).
pub async fn protein_grams_on(date: &str) -> f64 {
    let entries = list_diary(date).await;
    let foods = foods_by_ids(entries.iter().map(|e| e.food_id.clone())).await;
    entries.iter().filter_map(|e| {
        foods.get(&e.food_id).map(|f| {
            let eaten = (e.grams - e.waste_grams).max(0.0);
            f.protein * eaten / 100.0
        })
    }).sum()
}

/// Logical "yesterday" (today - 1 day) as "YYYY-MM-DD", pivoting on the 04:00
/// day boundary like [`today`].
pub fn yesterday() -> String {
    (today_date() - chrono::Duration::days(1))
        .format("%Y-%m-%d")
        .to_string()
}

/// Resolve a food whose `is_restaurant` flag matches `want`. If `food` already
/// matches it's stored and returned as-is. Otherwise we Copy-on-Write: reuse an
/// existing identical variant carrying the wanted flag, or create a fresh Food —
/// leaving the original untouched so other (e.g. past) diary entries keep it.
async fn food_with_restaurant_flag(food: &Food, want: bool) -> Food {
    if food.is_restaurant == want {
        db::put("foods", food).await;
        return food.clone();
    }
    let existing = list_foods().await.into_iter().find(|f| {
        f.id != food.id
            && f.is_restaurant == want
            && f.name == food.name
            && f.is_recipe == food.is_recipe
            && f.recipe_id == food.recipe_id
            && f.kcal == food.kcal
            && f.protein == food.protein
            && f.fat == food.fat
            && f.carbs == food.carbs
            && f.package_weight == food.package_weight
            && f.nutrients == food.nutrients
    });
    if let Some(variant) = existing {
        return variant;
    }
    let variant = Food {
        id: new_id(),
        is_restaurant: want,
        created_at: now(),
        updated_at: now(),
        ..food.clone()
    };
    db::put("foods", &variant).await;
    variant
}

pub async fn save_food_to_diary(
    food: &Food,
    grams: f64,
    waste_grams: f64,
    is_restaurant: bool,
    // Explicit meal this entry belongs to ("breakfast"/"lunch"/"dinner"), from the
    // panel whose «+» was tapped. `None` leaves it unlabelled (derived by time).
    meal_label: Option<String>,
) -> DiaryEntry {
    let food = food_with_restaurant_flag(food, is_restaurant).await;
    let entry = DiaryEntry {
        id: new_id(),
        food_id: food.id.clone(),
        date: today(),
        time: Some(time_now()),
        grams,
        waste_grams,
        meal_label,
        deleted: false,
        created_at: now(),
        updated_at: now(),
    };
    db::put("diary", &entry).await;
    // Classify this food's categories in the background (snack / liquid calories /
    // vegetable-fruit) as soon as it's logged.
    crate::services::classify::enqueue(food.id.clone());
    entry
}

pub async fn update_diary_entry(
    id: &str,
    grams: f64,
    waste_grams: f64,
    is_restaurant: bool,
) -> Option<DiaryEntry> {
    let mut entry: DiaryEntry = db::get("diary", id).await?;
    let food: Food = db::get("foods", &entry.food_id).await?;
    let food = food_with_restaurant_flag(&food, is_restaurant).await;
    entry.food_id = food.id.clone();
    entry.grams = grams;
    entry.waste_grams = waste_grams;
    entry.updated_at = now();
    db::put("diary", &entry).await;
    // The day's aggregates changed → drop its cached indicator values.
    crate::services::indicators::invalidate_day(&entry.date).await;
    // Editing may fork a new food variant (restaurant CoW) — classify it.
    crate::services::classify::enqueue(entry.food_id.clone());
    Some(entry)
}

/// One-time backfill of explicit meal labels. The diary switched from meals
/// DERIVED by time to EXPLICIT `meal_label` (breakfast/lunch/dinner). This assigns
/// a label to every pre-existing entry that has none, using the same time windows
/// the old derivation used, so history keeps the meal it always appeared under.
/// Idempotent + guarded by an app_flags flag → runs once per device; pushes the
/// relabelled entries so the labels reach the server and other devices.
pub async fn migrate_meal_labels() {
    const FLAG: &str = "meal_labels_backfilled_v1";
    if crate::services::app_flags::get_bool(FLAG) {
        return;
    }
    let all: Vec<DiaryEntry> = db::list_all("diary").await;
    let mut changed = false;
    for mut e in all {
        if e.meal_label.is_none() {
            // meal_key_for on an unlabelled entry derives from its local time.
            e.meal_label = Some(crate::services::meal_split::meal_key_for(&e).to_string());
            e.updated_at = now();
            db::put("diary", &e).await;
            changed = true;
        }
    }
    crate::services::app_flags::set_bool(FLAG, true);
    if changed {
        crate::services::sync::push_background();
    }
}

/// Stores whose rows can be deleted via the deletion log (kind == store name).
const DELETABLE_STORES: &[&str] = &[
    "foods", "diary", "recipes", "recipe_ingredients", "goals", "weight_entries", "step_entries",
];

/// Record an explicit deletion (tombstone) for `target_id` in store `kind`. The
/// record is synced and re-applied on every device; the local row is removed by
/// the caller (and re-removed by [`apply_deletions`] after each pull, since the
/// server never hard-deletes the entity — it only accumulates these records).
pub async fn record_deletion(kind: &str, target_id: &str) {
    let rec = api_types::DeletionRecord {
        id: new_id(),
        kind: kind.to_string(),
        target_id: target_id.to_string(),
        created_at: now(),
    };
    db::put("deletions", &rec).await;
}

/// Apply every known deletion record: remove the target row from its store. Run
/// after each pull so deletions made on other devices take effect locally.
pub async fn apply_deletions() {
    let dels: Vec<api_types::DeletionRecord> = db::list_all("deletions").await;
    for d in dels {
        if DELETABLE_STORES.contains(&d.kind.as_str()) {
            db::delete(&d.kind, &d.target_id).await;
        }
    }
}

pub async fn remove_food_diary(entry_id: &str) -> Result<(), String> {
    let entry: DiaryEntry = db::get("diary", entry_id)
        .await
        .ok_or_else(|| "entry not found".to_string())?;
    if entry.date != today() {
        return Err("can only delete today's entries".to_string());
    }
    record_deletion("diary", entry_id).await;
    db::delete("diary", entry_id).await;
    crate::services::indicators::invalidate_day(&entry.date).await;
    Ok(())
}

/// Danger zone — delete diary food entries (and their cached day-summaries).
/// `cutoff` (YYYY-MM-DD): entries with `date < cutoff` are removed; `None`
/// removes ALL. Uses a SOFT delete (`deleted=true`, bumped `updated_at`) — that's
/// the ONLY form the server honours (`/sync/push` upserts and only tombstones via
/// `deleted`; absent rows are left intact), so the caller must `sync::push_*`
/// afterwards for it to propagate. Day/week summaries are a local-only derived
/// cache, so they're hard-deleted (keeping the `meta:` markers).
pub async fn delete_diary_data(cutoff: Option<&str>) {
    let entries: Vec<DiaryEntry> = db::list_all("diary").await;
    for e in entries {
        if e.deleted {
            continue;
        }
        if cutoff.map_or(true, |c| e.date.as_str() < c) {
            record_deletion("diary", &e.id).await;
            db::delete("diary", &e.id).await;
        }
    }
    // AI summaries were removed (functionality moved to the dashboard); the
    // `summaries` store is no longer written or read. Wipe any stale records.
    db::clear("summaries").await;
    // A bulk delete can touch arbitrary past days → drop the whole indicator cache.
    crate::services::indicators::clear_cache().await;
}

/// Duplicate a diary entry as a NEW entry today with a fresh time (food and
/// grams/waste copied). Used by the diary-row long-press "Duplicate" action.
pub async fn duplicate_diary_entry(entry_id: &str, meal_label: Option<String>) -> Option<DiaryEntry> {
    let src: DiaryEntry = db::get("diary", entry_id).await?;
    let entry = DiaryEntry {
        id: new_id(),
        food_id: src.food_id.clone(),
        date: today(),
        time: Some(time_now()),
        grams: src.grams,
        waste_grams: src.waste_grams,
        // Target meal chosen in the duplicate dialog; falls back to the source's.
        meal_label: meal_label.or_else(|| src.meal_label.clone()),
        deleted: false,
        created_at: now(),
        updated_at: now(),
    };
    db::put("diary", &entry).await;
    crate::services::classify::enqueue(entry.food_id.clone());
    Some(entry)
}

/// Edit the product (name + KBJU + custom nutrients) behind a diary entry.
/// Copy-on-write: if the product is referenced ONLY by this entry, edit it in
/// place; if it's shared (any other non-deleted diary entry, or any recipe
/// ingredient), create a copy with the edits and repoint just this entry — so
/// other usages keep the original values.
/// Всё, что о продукте нашёл ИИ и что человек может поправить руками в форме
/// редактирования: нутриенты, железо (количество + усвоение) и категорийные флаги.
#[derive(Clone, Default)]
pub struct AiFoodData {
    pub nutrients: BTreeMap<String, f64>,
    pub iron_mg: Option<f64>,
    pub iron_absorption: Option<f64>,
    pub is_veg_fruit: Option<bool>,
    pub is_heme: Option<bool>,
    pub is_red_meat: Option<bool>,
    pub is_processed_meat: Option<bool>,
    pub is_egg: Option<bool>,
}

pub async fn edit_food_for_entry(
    entry_id: &str,
    name: String,
    kcal: f64,
    protein: f64,
    fat: f64,
    carbs: f64,
    ai: AiFoodData,
) -> Option<()> {
    let mut entry: DiaryEntry = db::get("diary", entry_id).await?;
    let food: Food = db::get("foods", &entry.food_id).await?;

    // Everything the AI derived is bound to the product NAME: the category tags, the
    // enriched nutrients and the iron pair. When the name changes they're stale, so
    // clear them and let the background queue re-derive them under the new name.
    // Unchanged name → keep whatever the form holds (including manual corrections).
    let renamed = name != food.name;
    let mut nutrients = ai.nutrients;
    // Мясные признаки сбрасываются вместе с остальными: они выведены из ТОГО ЖЕ
    // имени, и под новым именем «колбаса» от прежнего продукта осталась бы висеть.
    let (is_veg_fruit, is_heme, is_red_meat, is_processed_meat, is_egg, iron_mg, iron_absorption) =
        if renamed {
            for key in crate::services::enrich::nutrient_names() {
                nutrients.remove(key);
            }
            (None, None, None, None, None, None, None)
        } else {
            (
                ai.is_veg_fruit,
                ai.is_heme,
                ai.is_red_meat,
                ai.is_processed_meat,
                ai.is_egg,
                ai.iron_mg,
                ai.iron_absorption,
            )
        };

    let all_diary: Vec<DiaryEntry> = db::list_all("diary").await;
    let other_diary = all_diary
        .iter()
        .any(|e| e.food_id == food.id && e.id != entry.id && !e.deleted);
    let recipe_refs: Vec<RecipeIngredient> =
        db::list_by_index("recipe_ingredients", "food_id", &food.id).await;
    let shared = other_diary || !recipe_refs.is_empty();

    if shared {
        let copy = Food {
            id: new_id(),
            name, kcal, protein, fat, carbs, nutrients,
            is_veg_fruit, is_heme, is_red_meat, is_processed_meat, is_egg,
            iron_mg, iron_absorption,
            created_at: now(),
            updated_at: now(),
            ..food.clone()
        };
        db::put("foods", &copy).await;
        entry.food_id = copy.id.clone();
        entry.updated_at = now();
        db::put("diary", &entry).await;
        crate::services::classify::enqueue(copy.id);
    } else {
        let updated = Food {
            name, kcal, protein, fat, carbs, nutrients,
            is_veg_fruit, is_heme, is_red_meat, is_processed_meat, is_egg,
            iron_mg, iron_absorption,
            updated_at: now(),
            ..food.clone()
        };
        db::put("foods", &updated).await;
        crate::services::classify::enqueue(updated.id);
    }
    Some(())
}

// --- Food Drafts ---

pub async fn save_draft(food: &Food) -> FoodDraft {
    let new_keys: std::collections::BTreeSet<&str> =
        food.nutrients.keys().map(|k| k.as_str()).collect();

    let existing: Vec<FoodDraft> = db::list_all("food_drafts").await;
    let matched = existing.into_iter().find(|d| {
        d.name == food.name
            && d.nutrients.len() == new_keys.len()
            && d.nutrients.keys().all(|k| new_keys.contains(k.as_str()))
    });

    if let Some(mut draft) = matched {
        draft.kcal = food.kcal;
        draft.protein = food.protein;
        draft.fat = food.fat;
        draft.carbs = food.carbs;
        draft.nutrients = food.nutrients.clone();
        draft.package_weight = food.package_weight;
        draft.created_at = now();
        db::put("food_drafts", &draft).await;
        return draft;
    }

    let draft = FoodDraft {
        id: new_id(),
        name: food.name.clone(),
        kcal: food.kcal,
        protein: food.protein,
        fat: food.fat,
        carbs: food.carbs,
        nutrients: food.nutrients.clone(),
        package_weight: food.package_weight,
        food_id: None,
        created_at: now(),
    };
    db::put("food_drafts", &draft).await;
    draft
}

pub async fn update_draft_fields(draft_id: &str, food: &Food) {
    if let Some(mut draft) = db::get::<FoodDraft>("food_drafts", draft_id).await {
        draft.name = food.name.clone();
        draft.kcal = food.kcal;
        draft.protein = food.protein;
        draft.fat = food.fat;
        draft.carbs = food.carbs;
        draft.nutrients = food.nutrients.clone();
        draft.package_weight = food.package_weight;
        let linked_food_id = draft.food_id.clone();
        db::put("food_drafts", &draft).await;

        // Keep the Food created from this draft in sync — editing the name (or
        // КБЖУ) of a recognized product must update both the draft AND its Food.
        // Only the editable fields change; id / flags / timestamps are preserved.
        if let Some(fid) = linked_food_id {
            if let Some(mut f) = db::get::<Food>("foods", &fid).await {
                // Имя — это то, О ЧЁМ мы спрашивали модель. Изменилось оно —
                // прошлый разговор больше не про этот продукт: запомненное
                // опознание и следы попыток забываются, иначе новое имя получит
                // признаки, выведенные из старого.
                let renamed = f.name != food.name;
                f.name = food.name.clone();
                f.kcal = food.kcal;
                f.protein = food.protein;
                f.fat = food.fat;
                f.carbs = food.carbs;
                f.nutrients = food.nutrients.clone();
                f.package_weight = food.package_weight;
                f.updated_at = now();
                db::put("foods", &f).await;
                if renamed {
                    super::food_probe::forget(&fid).await;
                }
            }
        }
    }
}

pub async fn set_draft_food_id(draft_id: &str, food_id: &str) {
    if let Some(mut draft) = db::get::<FoodDraft>("food_drafts", draft_id).await {
        draft.food_id = Some(food_id.to_string());
        db::put("food_drafts", &draft).await;
    }
}

pub async fn list_drafts() -> Vec<FoodDraft> {
    let mut drafts: Vec<FoodDraft> = db::list_all("food_drafts").await;
    drafts.sort_by(|a, b| b.created_at.cmp(&a.created_at));
    drafts
}

/// Add a draft to diary: create Food if not yet created, then diary entry.
pub async fn add_draft_to_diary(draft_id: &str, grams: f64) -> Option<DiaryEntry> {
    let mut draft: FoodDraft = db::get("food_drafts", draft_id).await?;

    let food_id = if let Some(ref fid) = draft.food_id {
        fid.clone()
    } else {
        let food = Food {
            id: new_id(),
            name: draft.name.clone(),
            kcal: draft.kcal,
            protein: draft.protein,
            fat: draft.fat,
            carbs: draft.carbs,
            nutrients: draft.nutrients.clone(),
            package_weight: draft.package_weight,
            is_recipe: false,
            recipe_id: None,
            archived: false,
            is_restaurant: false,
            is_veg_fruit: None, is_heme: None,
            is_milk_globule: None,
            is_red_meat: None,
            is_processed_meat: None,
            is_egg: None,
            iron_mg: None, iron_absorption: None,
            fat_profile: None,
            balance_fat_profile: None,
            created_at: now(),
            updated_at: now(),
        };
        db::put("foods", &food).await;
        draft.food_id = Some(food.id.clone());
        db::put("food_drafts", &draft).await;
        food.id
    };

    let food: Food = db::get("foods", &food_id).await?;
    let entry = DiaryEntry {
        id: new_id(),
        food_id: food.id,
        date: today(),
        time: Some(time_now()),
        grams,
        waste_grams: 0.0,
        meal_label: None,
        deleted: false,
        created_at: now(),
        updated_at: now(),
    };
    db::put("diary", &entry).await;
    Some(entry)
}

// --- Food-photo recognition (dish photo → list of foods) ---------------------

/// Candidate foods to match a detected name against: the user's own foods,
/// excluding archived ones and recipes (a recipe isn't a raw catalog food).
/// Returns `(id, name)` pairs for `ai::match_food`.
pub async fn match_candidates() -> Vec<(String, String)> {
    list_foods()
        .await
        .into_iter()
        .filter(|f| !f.archived && !f.is_recipe)
        .map(|f| (f.id, f.name))
        .collect()
}

/// One resolved detected-food row, ready to write to the diary: a name, the
/// eaten grams, and per-100g КБЖУ + custom nutrients. Built by the caller from
/// a matched existing `Food` or from an `ai::lookup` result.
pub struct ResolvedFood {
    pub name: String,
    pub grams: f64,
    /// Per-100g macros.
    pub kcal: f64,
    pub protein: f64,
    pub fat: f64,
    pub carbs: f64,
    /// Per-100g custom nutrients (name -> value), same shape as `Food::nutrients`.
    pub nutrients: BTreeMap<String, f64>,
}

/// Write a batch of detected foods to today's diary: ONE new `Food` + ONE
/// `DiaryEntry` per item (decision D6). Mirrors the Food/DiaryEntry construction
/// in `add_draft_to_diary` (all AI tags None), fires the first-food story task
/// once, and enqueues background classification per food. Returns the entries.
pub async fn add_detected_foods_to_diary(items: &[ResolvedFood]) -> Vec<DiaryEntry> {
    let mut entries = Vec::with_capacity(items.len());
    for it in items {
        let food = Food {
            id: new_id(),
            name: it.name.clone(),
            kcal: it.kcal,
            protein: it.protein,
            fat: it.fat,
            carbs: it.carbs,
            nutrients: it.nutrients.clone(),
            package_weight: None,
            is_recipe: false,
            recipe_id: None,
            archived: false,
            is_restaurant: false,
            is_veg_fruit: None,
            is_heme: None,
            is_milk_globule: None,
            is_red_meat: None,
            is_processed_meat: None,
            is_egg: None,
            iron_mg: None,
            iron_absorption: None,
            fat_profile: None,
            balance_fat_profile: None,
            created_at: now(),
            updated_at: now(),
        };
        db::put("foods", &food).await;
        let entry = DiaryEntry {
            id: new_id(),
            food_id: food.id.clone(),
            date: today(),
            time: Some(time_now()),
            grams: it.grams,
            waste_grams: 0.0,
            meal_label: None,
            deleted: false,
            created_at: now(),
            updated_at: now(),
        };
        db::put("diary", &entry).await;
        // Classify this food's categories in the background (as `save_food_to_diary`).
        crate::services::classify::enqueue(food.id.clone());
        entries.push(entry);
    }
    entries
}

// --- Recipes ---

pub async fn list_recipes() -> Vec<Recipe> {
    db::list_all("recipes").await
}

/// All recipe ingredients across every recipe (id, recipe_id, food_id, grams).
pub async fn list_recipe_ingredients() -> Vec<RecipeIngredient> {
    db::list_all("recipe_ingredients").await
}

pub async fn new_recipe(name: &str) -> Recipe {
    let recipe = Recipe {
        id: new_id(),
        name: name.to_string(),
        notes: None,
        total_grams: None,
        finalized: false,
        food_id: None,
        superseded_by: None,
        ingredients: Vec::new(),
        created_at: now(),
        updated_at: now(),
    };
    db::put("recipes", &recipe).await;
    recipe
}

pub async fn get_recipe(id: &str) -> Option<Recipe> {
    let mut recipe: Recipe = db::get("recipes", id).await?;
    let ingredients: Vec<RecipeIngredient> =
        db::list_by_index("recipe_ingredients", "recipe_id", id).await;
    recipe.ingredients = ingredients;
    Some(recipe)
}

pub async fn change_recipe_name(id: &str, name: &str) -> Option<Recipe> {
    let mut recipe: Recipe = db::get("recipes", id).await?;
    recipe.name = name.to_string();
    recipe.updated_at = now();
    db::put("recipes", &recipe).await;
    get_recipe(id).await
}

pub async fn clone_recipe(id: &str) -> Option<Recipe> {
    let source = get_recipe(id).await?;
    let new_id = new_id();
    let ts = now();

    let mut cloned = Recipe {
        id: new_id.clone(),
        name: source.name.clone(),
        notes: source.notes.clone(),
        total_grams: None,
        finalized: false,
        food_id: None,
        superseded_by: None,
        ingredients: Vec::new(),
        created_at: ts.clone(),
        updated_at: ts.clone(),
    };
    db::put("recipes", &cloned).await;

    for ing in &source.ingredients {
        let new_ing = RecipeIngredient {
            id: self::new_id(),
            recipe_id: new_id.clone(),
            food_id: ing.food_id.clone(),
            grams: ing.grams,
            created_at: ts.clone(),
            updated_at: ts.clone(),
        };
        db::put("recipe_ingredients", &new_ing).await;
        cloned.ingredients.push(new_ing);
    }

    // Cooking again: mark the PARENT recipe as superseded by this new one. Explicit
    // parentage (id of the successor), never inferred from the mutable name. The
    // parent then drops out of the recipes list and the diary food search; its
    // finished food stays referenced by past diary entries. (The stored "recipes"
    // row keeps ingredients separately, so re-put here doesn't duplicate them.)
    if let Some(mut parent) = db::get::<Recipe>("recipes", id).await {
        parent.superseded_by = Some(new_id.clone());
        parent.updated_at = now();
        db::put("recipes", &parent).await;
    }

    Some(cloned)
}

// --- Recipe Ingredients ---

pub async fn add_ingredient_to_recipe(
    food: &Food,
    grams: f64,
    recipe_id: &str,
) -> RecipeIngredient {
    db::put("foods", food).await;
    // Classify the ingredient itself (not just the finished dish), so a recipe's
    // egg / red-meat / veg-fruit content can be counted by composition later.
    crate::services::classify::enqueue(food.id.clone());
    let ing = RecipeIngredient {
        id: new_id(),
        recipe_id: recipe_id.to_string(),
        food_id: food.id.clone(),
        grams,
        created_at: now(),
        updated_at: now(),
    };
    db::put("recipe_ingredients", &ing).await;
    ing
}

pub async fn update_ingredient(id: &str, grams: f64) -> Option<RecipeIngredient> {
    let mut ing: RecipeIngredient = db::get("recipe_ingredients", id).await?;
    ing.grams = grams;
    ing.updated_at = now();
    db::put("recipe_ingredients", &ing).await;
    Some(ing)
}

pub async fn remove_ingredient(id: &str) {
    record_deletion("recipe_ingredients", id).await;
    db::delete("recipe_ingredients", id).await;
}

// --- Finalize Recipe ---

/// Питательность блюда, посчитанная ИЗ СОСТАВА, на 100 г готового.
///
/// Единственное место, где это считается: раньше та же арифметика стояла в ТРЁХ
/// местах (доготовка, смена веса, показ на экране рецепта), а железа не было ни в
/// одном — его блюду проставляла модель по НАЗВАНИЮ. Отсюда и брались мидийные
/// 4,5 мг при 25 % у готового блюда, где мидий пятая часть массы: модель узнавала
/// слово, а не состав.
///
/// Вложенные рецепты выходят сами собой: доготовленный рецепт — обычный продукт
/// со своими значениями на 100 г, и складывается как всякий другой. Важен лишь
/// порядок — внутренний должен быть посчитан раньше внешнего.
pub struct RecipeNutrition {
    pub kcal: f64,
    pub protein: f64,
    pub fat: f64,
    pub carbs: f64,
    pub nutrients: BTreeMap<String, f64>,
    /// `None`, если ХОТЬ У ОДНОГО ингредиента железо ещё не выяснено: сумма по
    /// части состава — это не «мало железа», а «мы пока не знаем», и выдавать
    /// одно за другое нельзя.
    pub iron_mg: Option<f64>,
    pub iron_absorption: Option<f64>,
    /// Профиль жира блюда. Складывается ПО ВКЛАДУ: граммы кислот от каждого
    /// ингредиента суммируются и делятся на общий жир блюда — среднее по долям
    /// ингредиентов было бы неверным, потому что ложка льняного масла и стакан
    /// сметаны вносят в жир блюда несопоставимо разное.
    ///
    /// `None`, если профиля нет хоть у одного ингредиента, В КОТОРОМ ЕСТЬ ЖИР:
    /// безжирный ингредиент без профиля ничего не портит, у него нечего складывать.
    pub fat_profile: Option<FatProfile>,
    /// Профиль ТОЙ ЧАСТИ жира блюда, что участвует в балансе, — в долях от ОБЩЕГО
    /// жира блюда. Ингредиенты с целой молочно-жировой глобулой в него не входят.
    ///
    /// `None`, если хоть у одного жирного ингредиента признак глобулы не выяснен:
    /// сумма по остальным была бы не «меньше жира», а неправдой.
    pub balance_fat_profile: Option<FatProfile>,
}

/// Что блюдо даёт по нашим показателям — В РАСЧЁТЕ НА 100 Г готового блюда.
///
/// У блюда не может быть «флагов» вроде «овощ: да/нет»: рагу из говядины с
/// капустой — и то и другое сразу, а вопрос в том, СКОЛЬКО. Поэтому вместо
/// пометок здесь количества, посчитанные из состава и конечного веса. Ничего из
/// этого не спрашивается у модели и не правится руками: блюдо — функция состава.
///
/// `None` везде значит «не знаем», а не «ноль»: если хоть у одного ингредиента
/// нужный признак не выяснен, сумма по остальным — не ответ.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RecipeFlags {
    /// Граммы овощей и фруктов в 100 г блюда.
    pub veg_fruit_g: Option<f64>,
    /// Порций гемового железа в 100 г блюда. Порция — та же, что в недельной
    /// шкале: [`crate::services::heme::PORTION_PROTEIN_G`] граммов белка из
    /// гемовых продуктов.
    pub heme_portions: Option<f64>,
    /// Отношение (МНЖК+ПНЖК)/НЖК самого блюда. Сравнивать его с нормой —
    /// дело показывающего; здесь только величина. `None` — жира нет или профиль
    /// неизвестен.
    pub unsat_to_sat: Option<f64>,
    /// Граммы ЭПК+ДГК в 100 г блюда.
    pub epa_dha_g: Option<f64>,
}

/// Посчитать [`RecipeFlags`] по составу и конечному весу.
///
/// Овощи и порции гема считаются ПО ИНГРЕДИЕНТАМ (в самом блюде таких полей нет
/// и быть не должно), а жиры берутся из уже посчитанного профиля блюда — он
/// собран тем же проходом по вкладу и второй раз считать его незачем.
pub fn recipe_flags(
    ingredients: &[RecipeIngredient],
    foods: &BTreeMap<String, Food>,
    total_grams: f64,
    dish: &Food,
) -> RecipeFlags {
    if total_grams <= 0.0 {
        return RecipeFlags { veg_fruit_g: None, heme_portions: None, unsat_to_sat: None, epa_dha_g: None };
    }
    let scale = 100.0 / total_grams;
    let mut veg_g = 0.0_f64;
    let mut veg_known = true;
    let mut heme_protein_g = 0.0_f64;
    let mut heme_known = true;
    for ing in ingredients {
        let Some(f) = foods.get(&ing.food_id) else {
            veg_known = false;
            heme_known = false;
            continue;
        };
        match f.is_veg_fruit {
            Some(true) => veg_g += ing.grams,
            Some(false) => {}
            None => veg_known = false,
        }
        match f.is_heme {
            // Белок ингредиента — на 100 г, поэтому приводим к его массе.
            Some(true) => heme_protein_g += f.protein * ing.grams / 100.0,
            Some(false) => {}
            None => heme_known = false,
        }
    }
    // Баланс у блюда считается по ТОЙ ЖЕ части жира, что и в индикаторе, — иначе
    // форма и шкала говорили бы о разном. Омега-3 берётся из всего жира.
    let acids = dish.fatty_acids_per_100g();
    let balance = dish.balance_acids_per_100g();
    RecipeFlags {
        veg_fruit_g: veg_known.then(|| veg_g * scale),
        heme_portions: heme_known
            .then(|| heme_protein_g * scale / crate::services::heme::PORTION_PROTEIN_G),
        unsat_to_sat: balance.as_ref().and_then(|a| a.unsat_to_sat()),
        epa_dha_g: acids.map(|a| a.epa_dha_g),
    }
}

/// [`recipe_flags`] по идентификатору блюда: сам достаёт состав, вес и продукт.
pub async fn recipe_flags_for(recipe_id: &str) -> Option<RecipeFlags> {
    let recipe = get_recipe(recipe_id).await?;
    let food_id = recipe.food_id.clone()?;
    let dish = db::get::<Food>("foods", &food_id).await?;
    let total_grams = recipe.total_grams.filter(|g| *g > 0.0)?;
    let foods = foods_by_ids(recipe.ingredients.iter().map(|i| i.food_id.clone())).await;
    Some(recipe_flags(&recipe.ingredients, &foods, total_grams, &dish))
}

pub fn recipe_nutrition(
    ingredients: &[RecipeIngredient],
    foods: &BTreeMap<String, Food>,
    total_grams: f64,
) -> RecipeNutrition {
    let mut kcal = 0.0_f64;
    let mut protein = 0.0_f64;
    let mut fat = 0.0_f64;
    let mut carbs = 0.0_f64;
    let mut nutrients = BTreeMap::<String, f64>::new();
    // Железо копим в ДВУХ величинах: всего миллиграммов и сколько из них
    // усваивается. Долю усвоения блюда нельзя брать средним по ингредиентам —
    // мидии с их 25 % на пятой части массы вытянули бы всё блюдо. Правильная
    // доля — усвоенное, делённое на общее.
    let mut iron_mg = 0.0_f64;
    let mut iron_absorbed = 0.0_f64;
    let mut iron_known = true;
    // Жир копим В ГРАММАХ КИСЛОТ, а доли восстанавливаем в конце: это и есть
    // «по вкладу».
    let mut fa = api_types::FattyAcids::default();
    let mut fa_known = true;
    // ВТОРАЯ сумма — только тот жир, что участвует в балансе: без ингредиентов,
    // чей жир заперт в целой молочно-жировой глобуле. У продукта такой жир
    // отбрасывается целиком по флагу, а у блюда флага нет и быть не может —
    // сырники это и творог, и мука, и масло сразу. Значит считать надо по составу.
    let mut fa_balance = api_types::FattyAcids::default();
    let mut fa_balance_known = true;

    for ing in ingredients {
        let Some(f) = foods.get(&ing.food_id) else {
            // Ингредиента нет в базе — состав неполон, и железо блюда неизвестно.
            iron_known = false;
            fa_known = false;
            fa_balance_known = false;
            continue;
        };
        let factor = ing.grams / 100.0;
        kcal += f.kcal * factor;
        protein += f.protein * factor;
        fat += f.fat * factor;
        carbs += f.carbs * factor;
        for (k, v) in &f.nutrients {
            *nutrients.entry(k.clone()).or_default() += v * factor;
        }
        match (f.iron_mg, f.iron_absorption) {
            (Some(mg), Some(a)) => {
                iron_mg += mg * factor;
                iron_absorbed += mg * a * factor;
            }
            _ => iron_known = false,
        }
        match f.fatty_acids_per_100g() {
            Some(acids) => {
                fa += acids.scaled(factor);
                match f.is_milk_globule {
                    // Глобульный ингредиент в баланс не идёт — но это ОТВЕТ, а не
                    // пробел: сумма остаётся известной.
                    Some(true) => {}
                    Some(false) => fa_balance += acids.scaled(factor),
                    // Признак не выяснен — неизвестно, считать его или нет.
                    None => fa_balance_known = false,
                }
            }
            // Безжирный ингредиент без профиля блюду не мешает: складывать у него
            // нечего. Неизвестен профиль только там, где жир есть.
            None if f.fat <= 0.0 => {}
            None => {
                fa_known = false;
                fa_balance_known = false;
            }
        }
    }

    let scale = 100.0 / total_grams;
    RecipeNutrition {
        kcal: kcal * scale,
        protein: protein * scale,
        fat: fat * scale,
        carbs: carbs * scale,
        nutrients: nutrients.into_iter().map(|(k, v)| (k, v * scale)).collect(),
        iron_mg: iron_known.then(|| iron_mg * scale),
        // Доля усвоения от веса НЕ зависит: это отношение, и обе его части
        // масштабируются одинаково. Нулевое железо — доля неопределена.
        iron_absorption: iron_known
            .then(|| (iron_mg > 0.0).then(|| iron_absorbed / iron_mg))
            .flatten(),
        // Доли от веса не зависят — обе части отношения масштабируются одинаково,
        // поэтому делим на НЕмасштабированный жир. Нет жира — нет и профиля: делить
        // не на что, а нули означали бы «выяснено, что кислот нет».
        fat_profile: fa_known
            .then(|| {
                (fat > 0.0).then(|| {
                    let pct = |g: f64| g / fat * 100.0;
                    FatProfile {
                        sfa_pct: pct(fa.sfa_g),
                        mufa_pct: pct(fa.mufa_g),
                        pufa_pct: pct(fa.pufa_g),
                        epa_dha_pct: pct(fa.epa_dha_g),
                    }
                })
            })
            .flatten(),
        // Тот же приём для балансовой части: доли считаются от ОБЩЕГО жира блюда,
        // а не от «небалансового» — тогда умножение на `Food::fat` в дневнике даёт
        // сразу нужные граммы, без второго знаменателя.
        balance_fat_profile: fa_balance_known
            .then(|| {
                (fat > 0.0).then(|| {
                    let pct = |g: f64| g / fat * 100.0;
                    FatProfile {
                        sfa_pct: pct(fa_balance.sfa_g),
                        mufa_pct: pct(fa_balance.mufa_g),
                        pufa_pct: pct(fa_balance.pufa_g),
                        epa_dha_pct: pct(fa_balance.epa_dha_g),
                    }
                })
            })
            .flatten(),
    }
}


// ── Пересчёт рецептов ────────────────────────────────────────────────────────
//
// Блюдо — не самостоятельный продукт, а функция от состава. Но `finish_recipe`
// делает СНИМОК, а данные ингредиентов дозаполняются фоном уже после: человек
// собирает рецепт из свежих продуктов, доготавливает, и только потом модель
// выясняет их железо. Без пересчёта блюдо навсегда осталось бы с тем, что было
// известно в момент доготовки.
//
// Отсюда два уровня. Точечный — сразу после того, как данные ингредиента
// изменились. Сплошной — на запуске, чтобы поймать приехавшее синком с другого
// устройства и всё, что точечный путь упустит.

/// Пересчитать одно блюдо из состава. Пишет ТОЛЬКО если что-то изменилось:
/// холостая перезапись попала бы в журнал синхронизации и сделала бы холостой
/// синк нехолостым.
///
/// Возвращает `true`, если запись состоялась, — по этому признаку внешние рецепты
/// понимают, что их тоже надо пересчитать.
pub async fn recompute_recipe(recipe_id: &str) -> bool {
    let Some(recipe) = get_recipe(recipe_id).await else { return false };
    let Some(food_id) = recipe.food_id.clone() else { return false };
    let Some(total_grams) = recipe.total_grams.filter(|g| *g > 0.0) else { return false };
    let Some(mut food) = db::get::<Food>("foods", &food_id).await else { return false };

    let ids = recipe.ingredients.iter().map(|i| i.food_id.clone());
    let foods = foods_by_ids(ids).await;
    let n = recipe_nutrition(&recipe.ingredients, &foods, total_grams);

    let same = |a: f64, b: f64| (a - b).abs() < 1e-6;
    // Профили сравниваются одинаково, поэтому сравнение — одно на оба.
    let same_profile = |a: Option<FatProfile>, b: Option<FatProfile>| match (a, b) {
        (Some(a), Some(b)) => {
            same(a.sfa_pct, b.sfa_pct)
                && same(a.mufa_pct, b.mufa_pct)
                && same(a.pufa_pct, b.pufa_pct)
                && same(a.epa_dha_pct, b.epa_dha_pct)
        }
        (None, None) => true,
        _ => false,
    };
    let unchanged = same(food.kcal, n.kcal)
        && same(food.protein, n.protein)
        && same(food.fat, n.fat)
        && same(food.carbs, n.carbs)
        && food.nutrients.len() == n.nutrients.len()
        && food.nutrients.iter().all(|(k, v)| n.nutrients.get(k).is_some_and(|w| same(*v, *w)))
        && match (food.iron_mg, n.iron_mg) {
            (Some(a), Some(b)) => same(a, b),
            (None, None) => true,
            _ => false,
        }
        && match (food.iron_absorption, n.iron_absorption) {
            (Some(a), Some(b)) => same(a, b),
            (None, None) => true,
            _ => false,
        }
        && same_profile(food.fat_profile, n.fat_profile)
        && same_profile(food.balance_fat_profile, n.balance_fat_profile);
    if unchanged {
        return false;
    }

    food.kcal = n.kcal;
    food.protein = n.protein;
    food.fat = n.fat;
    food.carbs = n.carbs;
    food.nutrients = n.nutrients;
    food.iron_mg = n.iron_mg;
    food.iron_absorption = n.iron_absorption;
    food.fat_profile = n.fat_profile;
    food.balance_fat_profile = n.balance_fat_profile;
    food.updated_at = now();
    db::put("foods", &food).await;
    true
}

/// Пересчитать все блюда, где этот продукт — ингредиент, и дальше вверх по
/// вложенности: пересчитанное блюдо само может быть ингредиентом другого.
///
/// Глубина ограничена: рецепт, попавший сам в себя через цепочку, иначе крутил бы
/// обход вечно. Такого состава быть не должно, но проверять это здесь — не наше
/// дело, а вот не зависнуть — наше.
pub async fn recompute_recipes_using(food_id: &str) {
    let mut frontier = vec![food_id.to_string()];
    let mut seen = std::collections::HashSet::new();
    for _ in 0..8 {
        if frontier.is_empty() {
            return;
        }
        let mut next = Vec::new();
        for fid in frontier.drain(..) {
            let refs: Vec<RecipeIngredient> =
                db::list_by_index("recipe_ingredients", "food_id", &fid).await;
            for r in refs {
                if !seen.insert(r.recipe_id.clone()) {
                    continue;
                }
                if recompute_recipe(&r.recipe_id).await {
                    // Изменилось — значит внешние рецепты с этим блюдом устарели.
                    if let Some(rec) = get_recipe(&r.recipe_id).await {
                        if let Some(fid) = rec.food_id {
                            next.push(fid);
                        }
                    }
                }
            }
        }
        frontier = next;
    }
    leptos::logging::warn!("пересчёт рецептов: слишком глубокая вложенность, обход прерван");
}

/// Пересчитать ВСЕ блюда — от внутренних к внешним.
///
/// Порядок обязателен: пересчитай внешний раньше внутреннего, и он унаследует
/// старые числа внутреннего и разойдётся до следующего запуска. Глубина
/// ограничена по той же причине, что и выше.
pub async fn recompute_all_recipes() -> usize {
    let recipes = list_recipes().await;
    // Блюдо каждого рецепта — чтобы понять, кто в ком.
    let dish_of: BTreeMap<String, String> = recipes
        .iter()
        .filter_map(|r| r.food_id.clone().map(|f| (f, r.id.clone())))
        .collect();

    let mut pending: Vec<Recipe> = recipes;
    let mut done: std::collections::HashSet<String> = std::collections::HashSet::new();
    let mut changed = 0usize;
    for _ in 0..8 {
        if pending.is_empty() {
            break;
        }
        let mut deferred = Vec::new();
        for r in pending.drain(..) {
            // Готов к пересчёту, когда все вложенные рецепты уже пересчитаны.
            let waits = r.ingredients.iter().any(|i| {
                dish_of.get(&i.food_id).is_some_and(|rid| *rid != r.id && !done.contains(rid))
            });
            if waits {
                deferred.push(r);
                continue;
            }
            if recompute_recipe(&r.id).await {
                changed += 1;
            }
            done.insert(r.id.clone());
        }
        pending = deferred;
    }
    // Оставшиеся — кольцо в составе. Пересчитываем как есть, лишь бы не бросить.
    for r in pending {
        leptos::logging::warn!("пересчёт рецептов: кольцо в составе «{}»", r.name);
        if recompute_recipe(&r.id).await {
            changed += 1;
        }
    }
    if changed > 0 {
        crate::services::sync::push_background();
    }
    changed
}

pub async fn finish_recipe(recipe_id: &str, total_grams: f64) -> Option<Food> {
    let recipe = get_recipe(recipe_id).await?;
    let foods = foods_by_ids(recipe.ingredients.iter().map(|i| i.food_id.clone())).await;
    let n = recipe_nutrition(&recipe.ingredients, &foods, total_grams);

    let food = Food {
        id: new_id(),
        name: recipe.name.clone(),
        kcal: n.kcal,
        protein: n.protein,
        fat: n.fat,
        carbs: n.carbs,
        nutrients: n.nutrients,
        package_weight: None,
        is_recipe: true,
        recipe_id: Some(recipe_id.to_string()),
        archived: false,
        is_restaurant: false,
        is_veg_fruit: None,
        is_heme: None,
        is_milk_globule: None,
        is_red_meat: None,
        is_processed_meat: None,
        is_egg: None,
        // Железо блюда — из состава, а не от модели по названию. `None` здесь
        // означает «у части ингредиентов оно ещё не выяснено»; пересчёт вернётся
        // сюда, когда выяснится.
        iron_mg: n.iron_mg,
        iron_absorption: n.iron_absorption,
        // То же и с профилем жира: он складывается из состава.
        fat_profile: n.fat_profile,
        balance_fat_profile: n.balance_fat_profile,
        created_at: now(),
        updated_at: now(),
    };
    db::put("foods", &food).await;

    let mut updated_recipe: Recipe = db::get("recipes", recipe_id).await?;
    updated_recipe.finalized = true;
    updated_recipe.total_grams = Some(total_grams);
    updated_recipe.food_id = Some(food.id.clone());
    updated_recipe.updated_at = now();
    db::put("recipes", &updated_recipe).await;

    // NB: the previous version of a re-cooked dish is hidden purely at DISPLAY time
    // (newest-recipe-per-name) in both the recipes list (`recipes.rs`) and the food
    // picker (`food_picker.rs`) — no `archived` write here, so pre-existing
    // duplicates are handled too and there's no data migration.

    // Ensure every ingredient is classified, so the dish's egg / red-meat / veg-fruit
    // content can be counted by composition when it's logged.
    for ing in &recipe.ingredients {
        crate::services::classify::enqueue(ing.food_id.clone());
    }

    Some(food)
}

/// Change a finalized recipe's final total weight and REPRICE its finished food
/// in place. The dish's total nutrients (summed from ingredients) are fixed; only
/// the per-100g density changes, so recompute `100/new_total_grams` and update the
/// SAME food row (recipe.food_id) plus the recipe's `total_grams`. Any diary entry
/// referencing this food picks up the new per-100g values automatically.
pub async fn change_recipe_weight(recipe_id: &str, new_total_grams: f64) -> Option<Food> {
    if new_total_grams <= 0.0 {
        return None;
    }
    let recipe = get_recipe(recipe_id).await?;
    let food_id = recipe.food_id.clone()?;
    let foods = foods_by_ids(recipe.ingredients.iter().map(|i| i.food_id.clone())).await;
    let n = recipe_nutrition(&recipe.ingredients, &foods, new_total_grams);

    let mut food: Food = db::get("foods", &food_id).await?;
    food.kcal = n.kcal;
    food.protein = n.protein;
    food.fat = n.fat;
    food.carbs = n.carbs;
    food.nutrients = n.nutrients;
    food.iron_mg = n.iron_mg;
    food.iron_absorption = n.iron_absorption;
    food.updated_at = now();
    db::put("foods", &food).await;

    let mut updated_recipe: Recipe = db::get("recipes", recipe_id).await?;
    updated_recipe.total_grams = Some(new_total_grams);
    updated_recipe.updated_at = now();
    db::put("recipes", &updated_recipe).await;

    Some(food)
}

// --- Goals ---

pub async fn list_goals() -> Vec<Goal> {
    db::list_all("goals").await
}

pub async fn create_goal(input: CreateGoalInput) -> Goal {
    let key = api_types::nutrient_key::generate(&input.nutrient);
    let goal = Goal {
        id: new_id(),
        nutrient: input.nutrient,
        key,
        direction: input.direction,
        amount: input.amount,
        unit: input.unit,
        period: input.period,
        created_at: now(),
        updated_at: now(),
    };
    db::put("goals", &goal).await;
    goal
}

pub async fn update_goal(goal: &Goal) {
    db::put("goals", goal).await;
}

pub async fn delete_goal(id: &str) {
    record_deletion("goals", id).await;
    db::delete("goals", id).await;
}

/// Create or update the hidden daily-Calories `AtMost` goal (the "planka") to
/// `amount` kcal. Used when the user accepts the calorie planka in ch3.
pub async fn set_calorie_goal(amount: f64) {
    match list_goals().await.into_iter().find(|g| g.nutrient == "Calories") {
        Some(mut g) => {
            g.direction = GoalDirection::AtMost;
            g.amount = amount;
            g.unit = GoalUnit::Kcal;
            g.period = GoalPeriod::Day;
            g.updated_at = now();
            update_goal(&g).await;
        }
        None => {
            create_goal(CreateGoalInput {
                nutrient: "Calories".to_string(),
                direction: GoalDirection::AtMost,
                amount,
                unit: GoalUnit::Kcal,
                period: GoalPeriod::Day,
            })
            .await;
        }
    }
    // Установка попадает в ИСТОРИЮ: индикатор судит день по планке, действовавшей
    // именно в тот день, а не по сегодняшней.
    record_planka(PLANKA_CALORIES, amount).await;
    // Планка по белку — ДОЛЯ калорийной, значит движется вместе с ней. Пересчёт
    // стоит здесь, а не у вызывающих: через `set_calorie_goal` проходят ВСЕ пути
    // установки калорий (онбординг, недельное письмо, кнопка «Пересчитать», чат).
    record_protein_planka().await;
    // The planka now matches the current goal/trend again.
    set_planka_stale(false);
    // Restart the weekly-recompute clock: the planka was just (re)computed, so the
    // next automatic weekly letter is a full week out. Single point covering BOTH
    // the manual «Пересчитать» button and the background weekly recompute.
    crate::services::letters::mark_planka_recomputed();
}

/// Create (idempotently) the daily calcium goal — an `AtLeast` nutrient goal keyed
/// by the display name `Кальций`, so the food editor shows a calcium field and the
/// background enricher fills calcium on new foods. Set when the calcium week opens.
pub async fn set_calcium_goal(amount_mg: f64) {
    if list_goals()
        .await
        .into_iter()
        .any(|g| g.nutrient == crate::services::indicators::N_CALCIUM)
    {
        return; // already exists — don't duplicate or overwrite a user-tuned value
    }
    create_goal(CreateGoalInput {
        nutrient: crate::services::indicators::N_CALCIUM.to_string(),
        direction: GoalDirection::AtLeast,
        amount: amount_mg,
        unit: GoalUnit::Mg,
        period: GoalPeriod::Day,
    })
    .await;
}

// ── "Planka needs recalculating" signal ──────────────────────────────────────
// Raised when the course goal changes (the old planka no longer fits the new
// goal); cleared whenever the planka is (re)computed. Persisted per device.
const PLANKA_STALE_KEY: &str = "planka_stale";
thread_local! {
    static PLANKA_STALE: std::cell::RefCell<Option<leptos::RwSignal<bool>>> =
        const { std::cell::RefCell::new(None) };
}

/// Create the signal at the root, seeded from the persisted flag. Call from main().
pub fn init_planka_stale() {
    PLANKA_STALE.with(|c| {
        if c.borrow().is_none() {
            *c.borrow_mut() = Some(leptos::create_rw_signal(
                crate::services::app_flags::get_bool(PLANKA_STALE_KEY),
            ));
        }
    });
}

/// Reactive flag: the planka should be recomputed (the course goal changed).
pub fn planka_stale_signal() -> leptos::RwSignal<bool> {
    PLANKA_STALE.with(|c| c.borrow().expect("init_planka_stale() must run first"))
}

/// Set/clear the "planka needs recalculating" flag (persisted + reactive).
pub fn set_planka_stale(v: bool) {
    use leptos::SignalSet;
    crate::services::app_flags::set_bool(PLANKA_STALE_KEY, v);
    planka_stale_signal().set(v);
}

/// One-time migration: the steps planka used to live as a `Goal{nutrient:"Steps"}`
/// in the synced `goals` store, which every nutrient-iterating food UI wrongly
/// picked up. Move it to its own profile field (`profile::steps_planka`) and delete
/// the goal (soft-delete + push, so it clears server-side too). Idempotent: a no-op
/// once no Steps goal remains.
pub async fn migrate_steps_goal_to_planka() {
    let Some(g) = list_goals().await.into_iter().find(|g| g.nutrient == "Steps") else {
        return;
    };
    if crate::services::profile::get_steps_planka().is_none() && g.amount > 0.0 {
        crate::services::profile::set_steps_planka(g.amount);
    }
    delete_goal(&g.id).await;
    crate::services::sync::push_background();
}

// --- Weight Entries ---

pub async fn save_weight(weight_kg: f64, no_water: bool, no_food: bool, no_wash: bool, used_toilet: bool, morning: bool) -> WeightEntry {
    let date = today();
    if let Some(mut existing) = get_weight_for_date(&date).await {
        existing.weight_kg = weight_kg;
        existing.no_water = no_water;
        existing.no_food = no_food;
        existing.no_wash = no_wash;
        existing.used_toilet = used_toilet;
        existing.morning = morning;
        existing.updated_at = now();
        db::put("weight_entries", &existing).await;
        // Перевзвешивание в тот же день — тоже новый вес, а значит и новые границы
        // нормы белка.
        record_protein_planka().await;
        return existing;
    }
    let entry = WeightEntry {
        id: new_id(),
        date,
        weight_kg,
        no_water,
        no_food,
        no_wash,
        used_toilet,
        morning,
        created_at: now(),
        updated_at: now(),
    };
    db::put("weight_entries", &entry).await;
    // Границы нормы белка (пол по безжировой массе, потолок по полному весу)
    // считаются от ВЕСА, значит новый вес может дать новую норму. Пишем её в
    // историю здесь: иначе прошлые дни пересуживались бы по сегодняшней норме.
    record_protein_planka().await;
    entry
}

/// Пересчитать планку по белку по ТЕКУЩИМ данным (последний вес, профиль,
/// действующая калорийная планка) и записать её в историю сегодняшним днём.
/// Ничего не делает, пока нет веса или неполон профиль. Идемпотентна: повторный
/// вызов с тем же результатом не пишет ([`record_planka`]).
pub async fn record_protein_planka() {
    let Some(weight_kg) = list_weight_entries().await.last().map(|e| e.weight_kg) else {
        return;
    };
    let protein = crate::services::profile::protein_target_from_profile(weight_kg).await;
    if protein > 0 {
        record_planka(PLANKA_PROTEIN, protein as f64).await;
    }
}

/// Насколько вес изменился за последние `window_days` дней: последний замер минус
/// первый, в килограммах. `None`, когда замеров в окне меньше двух — сравнивать не с
/// чем.
///
/// Это НЕ тренд (`weight_trend`): тот оценивает наклон с доверием по всей истории и
/// решает, двигать ли планку. Здесь нужно другое — то, что человек видит сам на
/// весах за прошедшую неделю, чтобы письмо не разошлось с его собственным
/// наблюдением.
pub async fn weight_change_over(window_days: i64) -> Option<f64> {
    let today = chrono::Local::now().date_naive();
    let from = (today - chrono::Duration::days(window_days))
        .format("%Y-%m-%d")
        .to_string();
    let entries: Vec<WeightEntry> = list_weight_entries()
        .await
        .into_iter()
        .filter(|e| e.date >= from)
        .collect();
    match (entries.first(), entries.last()) {
        (Some(a), Some(b)) if entries.len() >= 2 => Some(b.weight_kg - a.weight_kg),
        _ => None,
    }
}

pub async fn get_weight_for_date(date: &str) -> Option<WeightEntry> {
    let entries: Vec<WeightEntry> = db::list_by_index("weight_entries", "date", date).await;
    entries.into_iter().next()
}

pub async fn list_weight_entries() -> Vec<WeightEntry> {
    let mut entries: Vec<WeightEntry> = db::list_all("weight_entries").await;
    entries.sort_by(|a, b| a.date.cmp(&b.date));
    entries
}

// --- Step Entries ---

pub async fn save_steps(date: &str, steps: u32) -> api_types::StepEntry {
    let entry = if let Some(mut existing) = get_steps_for_date(date).await {
        existing.steps = steps;
        existing.updated_at = now();
        db::put("step_entries", &existing).await;
        existing
    } else {
        let entry = api_types::StepEntry {
            id: new_id(),
            date: date.to_string(),
            steps,
            created_at: now(),
            updated_at: now(),
        };
        db::put("step_entries", &entry).await;
        entry
    };
    // Steps are ONE final value per day, so the step indicator is rewritten right
    // here (write-through) — the ONLY moment it recomputes. Editing an old day (e.g.
    // logging yesterday today) rewrites that day's row against the current planka.
    crate::services::indicators::record_steps(date).await;
    // Logging steps may be the data that finally lets the activity week open (its
    // planka needs step history), so re-check the unlock here too.
    crate::services::indicators::maybe_unlock_activity_week().await;
    entry
}

/// Steps logged on `date` as an `f64` (0 if none) — the value the step indicator
/// compares to the planka.
pub async fn steps_on(date: &str) -> f64 {
    get_steps_for_date(date).await.map(|e| e.steps as f64).unwrap_or(0.0)
}

/// The steps planka for this user from their ENTIRE step history: the average daily
/// steps over all logged days, mapped to a discipline target —
///   avg < 3000          → 7000
///   3000 ≤ avg < 10000  → 10000
///   avg ≥ 10000         → the average rounded UP to the nearest 100
/// `None` when there is no step data at all yet.
pub async fn steps_planka_from_history() -> Option<u32> {
    let entries = list_step_entries().await;
    if entries.is_empty() {
        return None;
    }
    let total: u64 = entries.iter().map(|e| e.steps as u64).sum();
    let avg = total as f64 / entries.len() as f64;
    Some(steps_planka_for_avg(avg))
}

/// Pure band mapping for the steps planka (unit-tested).
pub fn steps_planka_for_avg(avg: f64) -> u32 {
    if avg < 3000.0 {
        7000
    } else if avg < 10000.0 {
        10000
    } else {
        (((avg / 100.0).ceil()) as u32) * 100
    }
}

// ── Weekly step-up of the steps planka ───────────────────────────────────────
// Unlike the calorie planka (a control loop around a measured weight trend), the
// steps planka is a DISCIPLINE ladder: it only ever climbs, and only as fast as
// the user actually carries it. The signal is the step indicator's own colour —
// the very thing the user sees on the widget — so the target can never move for
// a reason invisible on screen.

/// The steps planka never climbs past this.
pub const STEPS_PLANKA_MAX: u32 = 15_000;
/// The two named rungs a green week climbs between.
const STEPS_RUNG_LOW: u32 = 7_000;
const STEPS_RUNG_HIGH: u32 = 10_000;
/// Step above the named rungs, and the whole step for a partial (orange) week.
const STEPS_PLANKA_STEP: u32 = 1_000;

/// Next steps planka from the current one and the step indicator's colour over
/// the last 7 completed days. Pure (no I/O) so it is unit-tested.
///
///   red     → hold (the week wasn't carried — don't pile more on)
///   orange  → +1000 (partial week: a small nudge)
///   green   → up the ladder: below 7000 → 7000 · below 10000 → 10000 ·
///             from 10000 up → +1000
///   unknown → hold (nothing to judge)
///
/// Never exceeds [`STEPS_PLANKA_MAX`] and never LOWERS an already-higher planka.
pub fn next_steps_planka(current: u32, state: crate::services::indicators::IndicatorState) -> u32 {
    use crate::services::indicators::IndicatorState;
    let raised = match state {
        IndicatorState::Red | IndicatorState::Unknown => current,
        IndicatorState::Orange => current.saturating_add(STEPS_PLANKA_STEP),
        IndicatorState::Green => {
            if current < STEPS_RUNG_LOW {
                STEPS_RUNG_LOW
            } else if current < STEPS_RUNG_HIGH {
                STEPS_RUNG_HIGH
            } else {
                current.saturating_add(STEPS_PLANKA_STEP)
            }
        }
    };
    raised.min(STEPS_PLANKA_MAX).max(current)
}

pub async fn get_steps_for_date(date: &str) -> Option<api_types::StepEntry> {
    let entries: Vec<api_types::StepEntry> = db::list_by_index("step_entries", "date", date).await;
    entries.into_iter().next()
}

pub async fn list_step_entries() -> Vec<api_types::StepEntry> {
    let mut entries: Vec<api_types::StepEntry> = db::list_all("step_entries").await;
    entries.sort_by(|a, b| a.date.cmp(&b.date));
    entries
}

/// Average steps over the last `days` COMPLETED days (today excluded), averaged over
/// only the days that actually have an entry — the same "completed days" window the
/// calorie planka uses for intake. `None` when no day in the window has steps.
pub async fn avg_steps_last_days(days: i64) -> Option<u32> {
    let today = chrono::Local::now().date_naive();
    let window: std::collections::BTreeSet<String> = (1..=days)
        .map(|i| (today - chrono::Duration::days(i)).format("%Y-%m-%d").to_string())
        .collect();
    let vals: Vec<u32> = list_step_entries()
        .await
        .into_iter()
        .filter(|e| window.contains(&e.date))
        .map(|e| e.steps)
        .collect();
    if vals.is_empty() {
        return None;
    }
    let sum: u64 = vals.iter().map(|&s| s as u64).sum();
    Some((sum as f64 / vals.len() as f64).round() as u32)
}

/// Every step ever logged, summed. The «за время использования вы прошли …»
/// figure in the weekly steps letter.
pub async fn total_steps() -> u64 {
    list_step_entries().await.iter().map(|e| e.steps as u64).sum()
}

/// Average daily steps over the FIRST `days` logged days — the baseline the
/// current week is compared against in the weekly letter. `None` until there are
/// that many logged days: comparing against a half-filled first fortnight would
/// make any later week look like a leap.
pub async fn avg_steps_first_days(days: usize) -> Option<u32> {
    // `list_step_entries` is sorted by date ascending, so the first `days`
    // entries ARE the earliest days (one entry per day).
    let entries = list_step_entries().await;
    if entries.len() < days || days == 0 {
        return None;
    }
    let sum: u64 = entries.iter().take(days).map(|e| e.steps as u64).sum();
    Some((sum as f64 / days as f64).round() as u32)
}

// --- Progress photos (client-only: front / side / back, for tracking) ---

/// One of the three required poses.
pub const POSES: [&str; 3] = ["front", "side", "back"];

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ProgressPhoto {
    pub id: String,
    pub pose: String,
    pub date: String,
    /// Image as a data URL (base64).
    pub image: String,
    pub created_at: String,
}

pub async fn save_progress_photo(pose: &str, image: &str) -> ProgressPhoto {
    let photo = ProgressPhoto {
        id: new_id(),
        pose: pose.to_string(),
        date: today(),
        image: image.to_string(),
        created_at: now(),
    };
    db::put("progress_photos", &photo).await;
    photo
}

pub async fn list_progress_photos() -> Vec<ProgressPhoto> {
    let mut photos: Vec<ProgressPhoto> = db::list_all("progress_photos").await;
    photos.sort_by(|a, b| b.created_at.cmp(&a.created_at)); // newest first
    photos
}

#[cfg(test)]
mod tests {
    use super::steps_planka_for_avg;
    use super::{adherence, calorie_planka, calorie_planka_weekly, planka_factor, Adherence, PLANKA_STEP};
    use crate::services::weight_trend::{Direction, WeightTrend};

    // ── Исполнение планки как стопор недельного пересчёта ────────────────────

    /// Уверенное быстрое похудение — тот случай, когда правило зовёт ПОДНЯТЬ планку.
    fn losing_fast() -> WeightTrend {
        WeightTrend::Estimated {
            direction: Direction::Down,
            confidence: 0.99,
            slope_kg_per_week: -1.2, // при 80 кг это 1.5 %/нед — сильно выше комфортных 0.7
            days: 14,
        }
    }

    /// Вес стоит — правило зовёт ОПУСТИТЬ планку.
    fn flat() -> WeightTrend {
        WeightTrend::Estimated {
            direction: Direction::Up,
            confidence: 0.9,
            slope_kg_per_week: 0.05,
            days: 14,
        }
    }

    #[test]
    fn otklonenie_ot_planki_po_koridoru() {
        assert_eq!(adherence(2400.0, 2400.0, 50.0), Adherence::OnTarget);
        assert_eq!(adherence(2360.0, 2400.0, 50.0), Adherence::OnTarget); // в пределах
        assert_eq!(adherence(2000.0, 2400.0, 50.0), Adherence::Under);
        assert_eq!(adherence(2600.0, 2400.0, 50.0), Adherence::Over);
    }

    /// Боевой случай: тревожная неделя, человек ест сильно меньше планки и быстро
    /// худеет. Без стопора планка поехала бы ВВЕРХ — и отрывалась бы дальше каждую
    /// неделю, потому что есть человек больше не станет.
    #[test]
    fn nedoedaet_planku_ne_podnimaem() {
        let base = 2400.0;
        // Само правило по весу зовёт вверх.
        assert!(planka_factor(&losing_fast(), 80.0) > 1.0);
        assert_eq!(calorie_planka_weekly(base, &losing_fast(), 80.0, Adherence::Under), base);
        // А тому, кто планку держал, поднимаем как и раньше.
        assert!(calorie_planka_weekly(base, &losing_fast(), 80.0, Adherence::OnTarget) > base);
    }

    /// Обратная сторона: перебор при стоящем весе. Опускать планку тому, кто её и
    /// не пробовал выполнять, бессмысленно — сначала пусть удержится в этой.
    #[test]
    fn pereedaet_planku_ne_ponizhaem() {
        let base = 2400.0;
        assert!(planka_factor(&flat(), 80.0) < 1.0);
        assert_eq!(calorie_planka_weekly(base, &flat(), 80.0, Adherence::Over), base);
        assert!(calorie_planka_weekly(base, &flat(), 80.0, Adherence::OnTarget) < base);
    }

    /// Стопор односторонний: недоедающему планку МОЖНО опустить (вес не падает —
    /// значит и эта планка велика), переедающему — поднять.
    #[test]
    fn stopor_ne_meshaet_dvizheniyu_v_druguyu_storonu() {
        let base = 2400.0;
        assert!(calorie_planka_weekly(base, &flat(), 80.0, Adherence::Under) < base);
        assert!(calorie_planka_weekly(base, &losing_fast(), 80.0, Adherence::Over) > base);
    }

    mod recipe_flags {
        use super::super::{recipe_flags, recipe_nutrition, RecipeIngredient};
        use api_types::{FatProfile, Food};
        use std::collections::BTreeMap;

        fn food(id: &str, protein: f64, fat: f64, veg: Option<bool>, heme: Option<bool>) -> Food {
            Food {
                id: id.to_string(),
                name: id.to_string(),
                kcal: 100.0,
                protein,
                fat,
                carbs: 0.0,
                nutrients: BTreeMap::new(),
                package_weight: None,
                is_recipe: false,
                recipe_id: None,
                archived: false,
                is_restaurant: false,
                is_veg_fruit: veg,
                is_heme: heme,
                is_milk_globule: None,
                is_red_meat: None,
                is_processed_meat: None,
            is_egg: None,
                    iron_mg: None,
                iron_absorption: None,
                fat_profile: None,
                balance_fat_profile: None,
                created_at: String::new(),
                updated_at: String::new(),
            }
        }

        fn ing(food_id: &str, grams: f64) -> RecipeIngredient {
            RecipeIngredient {
                id: format!("i-{food_id}"),
                recipe_id: "r".into(),
                food_id: food_id.into(),
                grams,

                created_at: String::new(),
                updated_at: String::new(),
            }
        }

        /// Пример пользователя: рагу из 400 г говядины и 400 г капусты, конечный
        /// вес 700 г. На 100 г блюда: капусты 400/700·100 = 57 г; белка от
        /// говядины 26·4 = 104 г, на 100 г блюда 14.86 г — это 0.59 порции.
        #[test]
        fn stew_of_beef_and_cabbage() {
            let mut foods = BTreeMap::new();
            foods.insert("beef".to_string(), food("beef", 26.0, 10.0, Some(false), Some(true)));
            foods.insert("cabbage".to_string(), food("cabbage", 1.8, 0.1, Some(true), Some(false)));
            let dish = food("dish", 0.0, 0.0, None, None);
            let f = recipe_flags(&[ing("beef", 400.0), ing("cabbage", 400.0)], &foods, 700.0, &dish);
            assert!((f.veg_fruit_g.unwrap() - 57.142_857).abs() < 1e-4, "{:?}", f.veg_fruit_g);
            assert!((f.heme_portions.unwrap() - 0.594_285).abs() < 1e-4, "{:?}", f.heme_portions);
        }

        /// Признак не выяснен хоть у одного ингредиента — ответа НЕТ. Сумма по
        /// остальным означала бы «столько и есть», а это неправда.
        #[test]
        fn one_unclassified_ingredient_makes_the_answer_unknown() {
            let mut foods = BTreeMap::new();
            foods.insert("beef".to_string(), food("beef", 26.0, 10.0, Some(false), Some(true)));
            foods.insert("x".to_string(), food("x", 5.0, 1.0, None, None));
            let dish = food("dish", 0.0, 0.0, None, None);
            let f = recipe_flags(&[ing("beef", 400.0), ing("x", 300.0)], &foods, 700.0, &dish);
            assert_eq!(f.veg_fruit_g, None);
            assert_eq!(f.heme_portions, None);
        }

        /// Жиры берутся из профилей САМОГО блюда, и у двух показателей они РАЗНЫЕ:
        /// омега-3 — из всего жира, баланс — из той части, что в балансе участвует.
        #[test]
        fn fats_come_from_the_dish_profile() {
            let foods = BTreeMap::new();
            let mut dish = food("dish", 0.0, 20.0, None, None);
            dish.is_recipe = true;
            dish.fat_profile = Some(FatProfile {
                sfa_pct: 20.0, mufa_pct: 40.0, pufa_pct: 30.0, epa_dha_pct: 10.0,
            });
            // Половина жира блюда — молочная глобула, в баланс она не идёт: доли
            // вдвое меньше, но считаются от ТОГО ЖЕ общего жира.
            dish.balance_fat_profile = Some(FatProfile {
                sfa_pct: 10.0, mufa_pct: 20.0, pufa_pct: 15.0, epa_dha_pct: 5.0,
            });
            let f = recipe_flags(&[], &foods, 700.0, &dish);
            // (20+15)/10 = 3.5 — считается по балансовой части, а не по всему жиру.
            assert!((f.unsat_to_sat.unwrap() - 3.5).abs() < 1e-9);
            // 20 г жира × 10 % = 2 г ЭПК+ДГК на 100 г — тут ВЕСЬ жир.
            assert!((f.epa_dha_g.unwrap() - 2.0).abs() < 1e-9);
        }

        /// Молочный ингредиент не попадает в балансовую часть, но остаётся в общей.
        #[test]
        fn milk_globule_ingredient_leaves_the_balance() {
            let mut foods = BTreeMap::new();
            // Творог: весь жир — глобула.
            let mut curd = food("curd", 18.0, 5.0, Some(false), Some(false));
            curd.is_milk_globule = Some(true);
            curd.fat_profile = Some(FatProfile {
                sfa_pct: 60.0, mufa_pct: 25.0, pufa_pct: 4.0, epa_dha_pct: 0.0,
            });
            // Масло растительное: в балансе участвует целиком.
            let mut oil = food("oil", 0.0, 100.0, Some(false), Some(false));
            oil.is_milk_globule = Some(false);
            oil.fat_profile = Some(FatProfile {
                sfa_pct: 10.0, mufa_pct: 25.0, pufa_pct: 60.0, epa_dha_pct: 0.0,
            });
            foods.insert("curd".to_string(), curd);
            foods.insert("oil".to_string(), oil);
            // 200 г творога (10 г жира) + 10 г масла (10 г жира) → 20 г жира на 210 г.
            let n = recipe_nutrition(&[ing("curd", 200.0), ing("oil", 10.0)], &foods, 210.0);
            let all = n.fat_profile.unwrap();
            let bal = n.balance_fat_profile.unwrap();
            // Общий профиль: НЖК 6+1 = 7 г из 20 → 35 %.
            assert!((all.sfa_pct - 35.0).abs() < 1e-6, "{all:?}");
            // Балансовый: творога нет, остаётся только масло — 1 г НЖК из 20 → 5 %.
            assert!((bal.sfa_pct - 5.0).abs() < 1e-6, "{bal:?}");
            assert!((bal.pufa_pct - 30.0).abs() < 1e-6, "{bal:?}");
            // Отношение блюда по балансу: (2.5+6)/0.5 = 17 против общих (3.5+6.4)/7.
            let ratio = |p: FatProfile| (p.mufa_pct + p.pufa_pct) / p.sfa_pct;
            assert!(ratio(bal) > ratio(all));
        }

        /// Признак глобулы не выяснен у жирного ингредиента — балансовой части НЕТ.
        /// Сумма по остальным была бы не «меньше жира», а неправдой.
        #[test]
        fn unknown_globule_flag_leaves_no_balance_profile() {
            let mut foods = BTreeMap::new();
            let mut x = food("x", 5.0, 20.0, Some(false), Some(false));
            x.fat_profile = Some(FatProfile {
                sfa_pct: 30.0, mufa_pct: 40.0, pufa_pct: 20.0, epa_dha_pct: 0.0,
            });
            // is_milk_globule остаётся None.
            foods.insert("x".to_string(), x);
            let n = recipe_nutrition(&[ing("x", 100.0)], &foods, 100.0);
            assert!(n.fat_profile.is_some());
            assert_eq!(n.balance_fat_profile, None);
        }

        /// Нет конечного веса — делить не на что, ответов нет вовсе.
        #[test]
        fn no_total_weight_no_answers() {
            let foods = BTreeMap::new();
            let dish = food("dish", 0.0, 0.0, None, None);
            let f = recipe_flags(&[], &foods, 0.0, &dish);
            assert_eq!(f.veg_fruit_g, None);
            assert_eq!(f.heme_portions, None);
            assert_eq!(f.epa_dha_g, None);
        }
    }

    fn estimated(dir: Direction, slope_wk: f64, conf: f64) -> WeightTrend {
        WeightTrend::Estimated { direction: dir, slope_kg_per_week: slope_wk, confidence: conf, days: 14 }
    }

    #[test]
    fn steps_planka_bands() {
        // < 3000 → 7000 (start small).
        assert_eq!(steps_planka_for_avg(0.0), 7000);
        assert_eq!(steps_planka_for_avg(2999.0), 7000);
        // 3000..10000 → 10000.
        assert_eq!(steps_planka_for_avg(3000.0), 10000);
        assert_eq!(steps_planka_for_avg(9999.0), 10000);
        // ≥ 10000 → average rounded UP to the nearest 100.
        assert_eq!(steps_planka_for_avg(10000.0), 10000);
        assert_eq!(steps_planka_for_avg(10001.0), 10100);
        assert_eq!(steps_planka_for_avg(12345.0), 12400);
        assert_eq!(steps_planka_for_avg(15000.0), 15000);
    }

    #[test]
    fn steps_planka_weekly_ladder() {
        use super::{next_steps_planka, STEPS_PLANKA_MAX};
        use crate::services::indicators::IndicatorState::{Green, Orange, Red, Unknown};

        // Красная неделя — планка не растёт, сколько бы её ни было.
        assert_eq!(next_steps_planka(7000, Red), 7000);
        assert_eq!(next_steps_planka(12000, Red), 12000);
        // Нет данных — тоже держим.
        assert_eq!(next_steps_planka(10000, Unknown), 10000);

        // Жёлтая неделя — ровно +1000 на любой высоте.
        assert_eq!(next_steps_planka(7000, Orange), 8000);
        assert_eq!(next_steps_planka(10000, Orange), 11000);
        assert_eq!(next_steps_planka(12300, Orange), 13300);

        // Зелёная неделя — на следующую ступень лестницы.
        assert_eq!(next_steps_planka(7000, Green), 10000);
        assert_eq!(next_steps_planka(8000, Green), 10000); // промежуточная → на 10000
        assert_eq!(next_steps_planka(9999, Green), 10000);
        // От 10000 и выше — по тысяче.
        assert_eq!(next_steps_planka(10000, Green), 11000);
        assert_eq!(next_steps_planka(12300, Green), 13300);

        // Потолок 15000: дошли — стоим, перешагнуть нельзя.
        assert_eq!(next_steps_planka(14000, Green), STEPS_PLANKA_MAX);
        assert_eq!(next_steps_planka(14500, Green), STEPS_PLANKA_MAX);
        assert_eq!(next_steps_planka(15000, Green), STEPS_PLANKA_MAX);
        assert_eq!(next_steps_planka(15000, Orange), STEPS_PLANKA_MAX);
        // Планку выше потолка (из истории шагов) не ПОНИЖАЕМ.
        assert_eq!(next_steps_planka(16200, Green), 16200);
    }

    #[test]
    fn planka_factor_confident_loss_steers_to_comfort_band() {
        // 90 kg → comfortable 0.3..0.7 %/wk = 0.27..0.63 kg/wk.
        // In band (0.5 kg/wk) → hold.
        assert_eq!(planka_factor(&estimated(Direction::Down, -0.5, 0.9), 90.0), 1.0);
        // Too slow (0.1 kg/wk ≈ 0.11 %) → gentle cut.
        assert_eq!(planka_factor(&estimated(Direction::Down, -0.1, 0.9), 90.0), 1.0 - PLANKA_STEP);
        // Too fast (1.0 kg/wk ≈ 1.11 %) → ease up.
        assert_eq!(planka_factor(&estimated(Direction::Down, -1.0, 0.9), 90.0), 1.0 + PLANKA_STEP);
    }

    #[test]
    fn planka_factor_weak_signal_holds_no_premature_cut() {
        // Probably losing (0.71) but not confident → HOLD (this user's case).
        assert_eq!(planka_factor(&estimated(Direction::Down, -0.2, 0.71), 90.0), 1.0);
        // Just over the WEAK threshold (0.66) → still holds.
        assert_eq!(planka_factor(&estimated(Direction::Down, -0.1, 0.66), 90.0), 1.0);
    }

    #[test]
    fn planka_factor_flat_or_gaining_cuts() {
        // Down but low confidence (p_down 0.55 < WEAK) → plateau-ish → cut.
        assert_eq!(planka_factor(&estimated(Direction::Down, -0.05, 0.55), 90.0), 1.0 - PLANKA_STEP);
        // Confident gain (up 0.9 → p_down 0.1) → cut.
        assert_eq!(planka_factor(&estimated(Direction::Up, 0.4, 0.9), 90.0), 1.0 - PLANKA_STEP);
        // Weakly gaining (up 0.7 → p_down 0.3) → cut.
        assert_eq!(planka_factor(&estimated(Direction::Up, 0.2, 0.7), 90.0), 1.0 - PLANKA_STEP);
    }

    #[test]
    fn planka_factor_no_trend_holds() {
        assert_eq!(planka_factor(&WeightTrend::Insufficient { days: 1 }, 90.0), 1.0);
        assert_eq!(
            planka_factor(
                &WeightTrend::Tentative { direction: Direction::Down, slope_kg_per_week: -0.5, days: 2 },
                90.0,
            ),
            1.0
        );
    }

    #[test]
    fn calorie_planka_rounds_to_50() {
        // Hold (weak down) → avg unchanged, rounded to 50.
        let hold = estimated(Direction::Down, -0.2, 0.71);
        assert_eq!(calorie_planka(2600.0, &hold, 90.0), 2600.0);
        assert_eq!(calorie_planka(2490.0, &hold, 90.0), 2500.0); // 49.8 -> 50
        // Cut (plateau) → avg*0.95, rounded to 50.
        let cut = estimated(Direction::Down, -0.05, 0.55);
        assert_eq!(calorie_planka(2600.0, &cut, 90.0), 2450.0); // 2470 -> 49.4 -> 2450
        assert_eq!(calorie_planka(2000.0, &cut, 90.0), 1900.0);
    }
}

#[cfg(test)]
mod recipe_tests {
    use super::*;

    fn food(id: &str, kcal: f64, iron: Option<(f64, f64)>) -> Food {
        Food {
            id: id.to_string(),
            name: id.to_string(),
            kcal,
            protein: 0.0,
            fat: 0.0,
            carbs: 0.0,
            nutrients: BTreeMap::new(),
            package_weight: None,
            is_recipe: false,
            recipe_id: None,
            archived: false,
            is_restaurant: false,
            is_veg_fruit: None,
            is_heme: None,
            is_milk_globule: None,
            is_red_meat: None,
            is_processed_meat: None,
            is_egg: None,
            iron_mg: iron.map(|(mg, _)| mg),
            iron_absorption: iron.map(|(_, a)| a),
            fat_profile: None,
            balance_fat_profile: None,
            created_at: String::new(),
            updated_at: String::new(),
        }
    }

    fn ing(food_id: &str, grams: f64) -> RecipeIngredient {
        RecipeIngredient {
            id: format!("i-{food_id}"),
            recipe_id: "r".into(),
            food_id: food_id.to_string(),
            grams,
            created_at: String::new(),
            updated_at: String::new(),
        }
    }

    /// Тот самый случай: мидии в блюде, где их пятая часть массы.
    #[test]
    fn zhelezo_propocionalno_dole_i_uvarke() {
        let mut foods = BTreeMap::new();
        foods.insert("мидии".to_string(), food("мидии", 80.0, Some((4.5, 0.25))));
        foods.insert("сыр".to_string(), food("сыр", 350.0, Some((0.3, 0.05))));

        // 100 г мидий + 400 г сыра, уварилось до 400 г готового.
        let n = recipe_nutrition(&[ing("мидии", 100.0), ing("сыр", 400.0)], &foods, 400.0);

        // Всего железа: 4,5 + 1,2 = 5,7 мг на 500 г сырья → на 100 г готового 1,425.
        let mg = n.iron_mg.unwrap();
        assert!((mg - 1.425).abs() < 1e-6, "{mg}");
        // И это НЕ мидийные 4,5 — то, что подставляла модель по названию.
        assert!(mg < 4.5);

        // Доля усвоения — по вкладу усвоенного, а не среднее по ингредиентам.
        // Усвоено: 4,5×0,25 + 1,2×0,05 = 1,185 мг → доля 1,185/5,7 = 0,2079.
        let a = n.iron_absorption.unwrap();
        assert!((a - 1.185 / 5.7).abs() < 1e-6, "{a}");
        // Мидийные 25 % блюду не достаются.
        assert!(a < 0.25);
    }

    #[test]
    fn uvarka_menyaet_plotnost_no_ne_dolyu() {
        let mut foods = BTreeMap::new();
        foods.insert("мидии".to_string(), food("мидии", 80.0, Some((4.5, 0.25))));
        let ings = [ing("мидии", 200.0)];

        let wet = recipe_nutrition(&ings, &foods, 200.0);
        let dry = recipe_nutrition(&ings, &foods, 100.0); // уварилось вдвое

        // Уварилось вдвое → на 100 г вдвое плотнее.
        assert!((dry.iron_mg.unwrap() / wet.iron_mg.unwrap() - 2.0).abs() < 1e-6);
        // А доля усвоения от веса не зависит — это отношение.
        assert!((dry.iron_absorption.unwrap() - wet.iron_absorption.unwrap()).abs() < 1e-9);
    }

    /// «Не знаем» и «мало» — разные вещи, и путать их нельзя.
    #[test]
    fn nevyyasnennoe_zhelezo_ingredienta_delaet_blyudo_neizvestnym() {
        let mut foods = BTreeMap::new();
        foods.insert("мидии".to_string(), food("мидии", 80.0, Some((4.5, 0.25))));
        foods.insert("сыр".to_string(), food("сыр", 350.0, None));

        let n = recipe_nutrition(&[ing("мидии", 100.0), ing("сыр", 100.0)], &foods, 200.0);
        assert!(n.iron_mg.is_none());
        assert!(n.iron_absorption.is_none());
        // Калории при этом посчитаны: они известны у обоих.
        assert!((n.kcal - 215.0).abs() < 1e-6, "{}", n.kcal);
    }

    #[test]
    fn otsutstvuyushchij_ingredient_ne_vydayotsya_za_izvestnoe() {
        let foods = BTreeMap::new();
        let n = recipe_nutrition(&[ing("пропавший", 100.0)], &foods, 100.0);
        assert!(n.iron_mg.is_none());
        assert_eq!(n.kcal, 0.0);
    }

    #[test]
    fn blyudo_kak_ingredient_skladyvaetsya_kak_obychnyj_produkt() {
        // Внутреннее блюдо уже посчитано и лежит продуктом со своими значениями.
        let mut foods = BTreeMap::new();
        foods.insert("соус".to_string(), food("соус", 100.0, Some((1.425, 0.2079))));
        foods.insert("паста".to_string(), food("паста", 150.0, Some((1.0, 0.04))));

        let n = recipe_nutrition(&[ing("соус", 100.0), ing("паста", 300.0)], &foods, 400.0);
        let mg = n.iron_mg.unwrap();
        // 1,425 + 3,0 = 4,425 мг на 400 г → 1,10625 на 100 г.
        assert!((mg - 1.10625).abs() < 1e-6, "{mg}");
    }
}
