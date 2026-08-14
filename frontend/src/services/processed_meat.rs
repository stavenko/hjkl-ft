//! Мясо глубокой переработки — индикатор ЧАСТОТЫ, без шкалы.
//!
//! Колбасы, сосиски, ветчина, бекон, вяленое и копчёное мясо связаны с
//! колоректальным раком отдельно от красного мяса как такового — и связь эта
//! начинается с малых количеств. Поэтому меряется не сколько, а КАК ЧАСТО: важно
//! не взвесить сосиску, а заметить, что она в тарелке через день.
//!
//! Отсюда и механика — кальциевая, дневная, с одной разницей: там день зелёный,
//! когда норма НАБРАНА, здесь — когда человек этого не ел вовсе. Правило то же
//! ([`indicators::daily_state`]): ни одного такого дня из семи — зелёный, один-три —
//! оранжевый, четыре и больше — красный.
//!
//! Своя `indicator_state`, а не общий дневной путь: тот определяет «есть ли данные»
//! по `value > 0`, а здесь ноль — это и есть успех, и человек, ничего такого не
//! евший, получал бы «нет данных» вместо зелёного. Судим дни по наличию дневника.
//!
//! Граммы никуда не деваются: съеденное тратит недельную планку красного мяса —
//! колбаса остаётся мясом. Этим занимается [`super::red_meat`].

use chrono::Duration;

use super::local;
use api_types::Food;

/// Идёт ли продукт в этот индикатор.
pub fn counts(food: &Food) -> bool {
    food.is_processed_meat == Some(true)
}

/// Открыт ли индикатор. Вместе с неделей красного мяса — это две стороны одного
/// разговора, и порознь они не показываются.
pub fn unlocked() -> bool {
    super::red_meat::unlocked()
}

/// Ел ли человек переработанное мясо в этот день.
///
/// Мера — белок, как и у соседних мясных счётчиков: он один умеет раскрывать блюда
/// по составу (пицца с салями засчитывается салями, а не всей пиццей). Сколько
/// именно, здесь не важно — важен сам факт.
pub async fn eaten_on(date: &str) -> bool {
    local::tag_protein_g_on(date, counts).await > 0.0
}

/// Цвет индикатора по последним семи ЗАВЕРШЁННЫМ дням.
///
/// День без дневника не судится — его просто не было. Если таких дней вся неделя,
/// судить не о чем: `Unknown`.
///
/// Дни ДО открытия темы судятся наравне с остальными: связь переработанного мяса с
/// онкологией не начинается в день, когда мы о ней рассказали, а данные за прошлое
/// есть — признак собирается с первого дня. Пока фоновый проход не разметил старые
/// продукты, день может выглядеть чистым; разметка сбросит его сама
/// (`cache_food_flag` → `invalidate_food`).
pub async fn indicator_state() -> super::indicators::IndicatorState {
    use super::indicators::IndicatorState;
    if !unlocked() {
        return IndicatorState::Unknown;
    }
    let today = local::today_date();
    let diary_days: std::collections::HashSet<String> =
        local::list_diary_dates().await.into_iter().collect();

    let mut judged = 0u32;
    let mut misses = 0u32;
    for i in 1..=7 {
        let day = today - Duration::days(i);
        let date = day.format("%Y-%m-%d").to_string();
        if !diary_days.contains(&date) {
            continue;
        }
        judged += 1;
        if eaten_on(&date).await {
            misses += 1;
        }
    }
    if judged == 0 {
        return IndicatorState::Unknown;
    }
    super::indicators::daily_state(misses)
}

/// Ряд последних семи завершённых дней — для панели индикатора.
///
/// В отличие от остальных дневных рядов, величина здесь двоичная: столбик либо
/// есть (ел), либо нет. День без дневника идёт без доли — столбика нет, вердикта
/// нет, подпись остаётся.
pub async fn daily_series() -> super::indicators::IndicatorSeries {
    let today = local::today_date();
    let diary_days: std::collections::HashSet<String> =
        local::list_diary_dates().await.into_iter().collect();
    let mut points: Vec<(String, f64, Option<f64>)> = Vec::new();
    let mut labels: Vec<String> = Vec::new();
    let mut met: Vec<Option<bool>> = Vec::new();
    for back in (1..=7i64).rev() {
        let d = today - Duration::days(back);
        let date = d.format("%Y-%m-%d").to_string();
        let logged = diary_days.contains(&date);
        let ate = logged && eaten_on(&date).await;
        points.push((date, if ate { 1.0 } else { 0.0 }, logged.then_some(if ate { 1.0 } else { 0.0 })));
        labels.push(d.format("%d.%m").to_string());
        met.push(logged.then_some(!ate));
    }
    let missed = met.iter().filter(|m| **m == Some(false)).count() as u32;
    super::indicators::IndicatorSeries {
        key: "processed_meat",
        state: indicator_state().await,
        days: points,
        met_days: met,
        missed,
        labels,
    }
}
