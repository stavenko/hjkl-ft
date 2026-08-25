//! Красное мясо — недельная планка ГРАММОВ, которую нельзя перебрать.
//!
//! Это первый наш индикатор про ограничение, а не про цель: у железа, гема и
//! омега-3 надо НАБРАТЬ, здесь — не превысить. Планка 700 г сырого мяса в неделю:
//! связь красного мяса с колоректальным раком растёт с дозой, и выше этой величины
//! риск повышается тем сильнее, чем больше съедено.
//!
//! **Считается через БЕЛОК, а не через вес съеденного.** Планка задана в СЫРОМ
//! мясе, а человек ест готовое, и граммы тут несопоставимы: при варке и жарке мясо
//! теряет четверть-треть веса, поэтому 150 г тушёной говядины — это заметно больше
//! 150 г сырой. Белок при готовке никуда не девается, значит по нему сырой вес
//! восстанавливается однозначно: у сырого мяса около 20 г белка на 100 г, отсюда
//! 700 г сырого = 140 г белка. Тот же приём, что у гема с его порциями, и по той же
//! причине.
//!
//! Заодно это само собой решает вопрос с блюдами: в тушёной картошке с мясом
//! засчитывается мясо, а не всё блюдо, — блюда раскрываются по составу общим
//! механизмом [`local::tag_protein_g_on`].
//!
//! **Переработанное мясо тратит те же граммы.** Колбаса — красное мясо, как бы её
//! ни переработали, и на недельный счёт она идёт наравне со стейком. Отдельно от
//! неё живёт свой индикатор — [`uper::processed_meat`], который меряет не
//! количество, а частоту.

use chrono::{Duration, NaiveDate};

use super::{app_flags, local};
use api_types::Food;

/// App-flag: неделя красного мяса открыта (шкала и два индикатора видны).
pub const RED_MEAT_UNLOCKED_KEY: &str = "red_meat_week_unlocked";

/// App-flag: день, от которого катится сетка недель красного мяса.
pub const RED_MEAT_WEEK_OPEN_KEY: &str = "red_meat_week_opened_at";

/// Недельная планка в граммах СЫРОГО мяса. Живёт в общем крейте.
pub use plankas::defaults::RED_MEAT_WEEKLY_LIMIT_RAW_G as WEEKLY_LIMIT_RAW_G;

/// Действующий недельный предел, г сырого — наш, пока куратор не назвал свой.
pub fn weekly_limit_g() -> f64 {
    use crate::services::plankas;
    plankas::constant(plankas::Kind::RedMeat)
}

/// Сколько белка в 100 г сырого красного мяса. Говядина, свинина и баранина дают
/// 19–21 г — берётся круглая середина, точность здесь не важнее прозрачности.
pub const RAW_PROTEIN_PER_100G: f64 = 20.0;

/// Недельная планка в граммах БЕЛКА — то, в чём на самом деле идёт счёт.
pub fn weekly_limit_protein_g() -> f64 {
    WEEKLY_LIMIT_RAW_G * RAW_PROTEIN_PER_100G / 100.0
}

/// Граммы сырого мяса, которым соответствует столько-то граммов его белка.
pub fn raw_grams_from_protein(protein_g: f64) -> f64 {
    protein_g * 100.0 / RAW_PROTEIN_PER_100G
}

/// Идёт ли продукт в недельный счёт: красное мясо и всё, что из него сделано.
///
/// Переработанное проверяется ОТДЕЛЬНО, а не только через `is_red_meat`: разметка
/// двух признаков независима, и сосиска, помеченная переработанной, но не красной,
/// иначе просто выпала бы из счёта.
pub fn counts(food: &Food) -> bool {
    food.is_red_meat == Some(true) || food.is_processed_meat == Some(true)
}

/// Открыта ли неделя красного мяса.
pub fn unlocked() -> bool {
    app_flags::get_bool(RED_MEAT_UNLOCKED_KEY)
}

/// День, с которого началась тема.
fn week_open_date() -> Option<NaiveDate> {
    app_flags::get(RED_MEAT_WEEK_OPEN_KEY)
        .and_then(|s| NaiveDate::parse_from_str(&s, "%Y-%m-%d").ok())
}

/// Неделя красного мяса, в которую попадает `today`. Своя сетка, от дня открытия —
/// как у жиров и железа.
pub fn week_bounds(today: NaiveDate) -> Option<(NaiveDate, NaiveDate)> {
    let open = week_open_date()?;
    if today < open {
        return None;
    }
    let elapsed = (today - open).num_days();
    let start = open + Duration::days(elapsed / 7 * 7);
    Some((start, start + Duration::days(6)))
}

// ── Замер ────────────────────────────────────────────────────────────────────

/// Красное мясо за день, в граммах СЫРОГО эквивалента.
pub async fn raw_grams_on(date: &str) -> f64 {
    raw_grams_from_protein(local::tag_protein_g_on(date, counts).await)
}

async fn raw_grams_between(from: NaiveDate, to: NaiveDate) -> f64 {
    let mut total = 0.0;
    let mut d = from;
    while d <= to {
        total += raw_grams_on(&d.format("%Y-%m-%d").to_string()).await;
        d += Duration::days(1);
    }
    total
}

/// Ход текущей недели — для шкалы в виджете.
#[derive(Clone)]
pub struct WeeklyRedMeat {
    /// Съедено за неделю, граммы сырого эквивалента.
    pub grams: f64,
    /// Планка — 700 г.
    pub limit: f64,
    /// 1…7 — какой сегодня день недели красного мяса.
    pub day_of_week: u32,
}

impl WeeklyRedMeat {
    /// Цвет шкалы. Смысл ОБРАТНЫЙ шкалам-целям: чем полнее, тем хуже.
    ///
    /// * красная — планка выбрана целиком, дальше каждый грамм сверх неё;
    /// * оранжевая — человек идёт БЫСТРЕЕ равномерного темпа и при таком темпе
    ///   выберет её до конца недели (на третий день съедено больше трёх седьмых);
    /// * зелёная — идёт в темпе или медленнее.
    ///
    /// Предупреждение по темпу, а не по половине планки: съесть 400 г в понедельник
    /// и больше не есть — это уложиться, а 120 г каждый день — это перебрать, и
    /// сказать об этом надо во вторник, а не в воскресенье.
    pub fn state(&self) -> super::indicators::IndicatorState {
        use super::indicators::IndicatorState;
        if self.grams >= self.limit {
            return IndicatorState::Red;
        }
        let pace = self.limit * f64::from(self.day_of_week.clamp(1, 7)) / 7.0;
        if self.grams > pace {
            IndicatorState::Orange
        } else {
            IndicatorState::Green
        }
    }
}

pub async fn weekly_progress() -> Option<WeeklyRedMeat> {
    let today = local::today_date();
    let (start, _end) = week_bounds(today)?;
    Some(WeeklyRedMeat {
        grams: raw_grams_between(start, today).await,
        limit: weekly_limit_g(),
        day_of_week: (today - start).num_days() as u32 + 1,
    })
}

/// Закрыта ли хотя бы одна ЗАВЕРШЁННАЯ неделя красного мяса с тех пор, как тема
/// открылась, — условие перехода к следующей главе, неделе яиц.
///
/// Тот же гейт, что у жиров (`fats::week_closed_since_open`), и считается так же:
/// от дня открытия темы до последней завершённой недели. Текущая не судится — «ещё
/// не перебрал» не то же самое, что «уложился».
///
/// Неделя, в которую человек не вёл дневник, не идёт в счёт: пустой дневник даёт
/// ноль граммов, и неделя выглядела бы удержанной, хотя её просто не было.
///
/// НЕДЕЛЯ ЯИЦ ЕЩЁ НЕ СДЕЛАНА, и открывать по этому условию пока нечего: гейт
/// показывается человеку счётчиком, который на нуле и останавливается.
pub async fn week_closed_since_open() -> bool {
    let today = local::today_date();
    let (Some(open), Some((cur_start, _))) = (week_open_date(), week_bounds(today)) else {
        return false;
    };
    let diary_days: std::collections::HashSet<String> =
        local::list_diary_dates().await.into_iter().collect();
    let mut s = open;
    while s < cur_start {
        let e = s + Duration::days(6);
        let logged = (0..7).any(|d| {
            diary_days.contains(&(s + Duration::days(d)).format("%Y-%m-%d").to_string())
        });
        if logged && raw_grams_between(s, e).await <= weekly_limit_g() {
            return true;
        }
        s += Duration::days(7);
    }
    false
}

/// Цвет индикатора по ЗАВЕРШЁННЫМ неделям — общее недельное правило, то же, что у
/// железа, гема и омега-3. Неделя закрыта, если человек в неё уложился.
///
/// Неделя, в которую человек не вёл дневник, не судится: её просто не было. Иначе
/// пустая неделя выглядела бы образцовой — ноль граммов мяса, планка не перебрана.
///
/// Недели ДО открытия темы судятся наравне с остальными, и это намеренно. Планка в
/// 700 г — универсальная, она действовала всегда — мы лишь молчали о ней; данные за прошлое
/// есть, потому что мясные признаки собираются с первого дня. Индикатор для того и
/// заведён, чтобы честно показать, что происходит, а не начинать с чистого листа.
/// (У калорий правило обратное — там планка ИНДИВИДУАЛЬНАЯ и до выдачи её не
/// существовало.)
///
/// Пока фоновый проход не разметил старые продукты, граммы занижены и неделя может
/// выглядеть закрытой. Это само поправится: разметка признака сбрасывает дни, где
/// ел этот продукт (`cache_food_flag` → `invalidate_food`), а здешний счёт вообще
/// идёт по дневнику на лету.
pub async fn indicator_state() -> super::indicators::IndicatorState {
    use super::indicators::IndicatorState;
    let today = local::today_date();
    let Some((cur_start, _)) = week_bounds(today) else {
        return IndicatorState::Unknown;
    };

    let diary_days: std::collections::HashSet<String> =
        local::list_diary_dates().await.into_iter().collect();
    let mut history: Vec<bool> = Vec::new();
    let mut s = cur_start;
    for _ in 0..super::indicators::WEEKLY_WINDOW {
        s -= Duration::days(7);
        let e = s + Duration::days(6);
        let logged = (0..7).any(|d| {
            diary_days.contains(&(s + Duration::days(d)).format("%Y-%m-%d").to_string())
        });
        if !logged {
            continue;
        }
        history.push(raw_grams_between(s, e).await <= weekly_limit_g());
    }
    history.reverse();
    super::indicators::weekly_state(&history)
}

/// Столбики за последние завершённые недели — как у гема.
pub async fn weekly_series() -> super::indicators::IndicatorSeries {
    let today = local::today_date();
    let mut points: Vec<(String, f64, Option<f64>)> = Vec::new();
    let mut labels: Vec<String> = Vec::new();
    let mut met: Vec<Option<bool>> = Vec::new();
    if let Some((cur_start, _)) = week_bounds(today) {
        let diary_days: std::collections::HashSet<String> =
            local::list_diary_dates().await.into_iter().collect();
        let window = super::indicators::WEEKLY_WINDOW as i64;
        for back in (1..=window).rev() {
            let s = cur_start - Duration::days(7 * back);
            let e = s + Duration::days(6);
            let logged = (0..7).any(|d| {
                diary_days.contains(&(s + Duration::days(d)).format("%Y-%m-%d").to_string())
            });
            let g = raw_grams_between(s, e).await;
            let ratio = logged.then(|| g / weekly_limit_g());
            points.push((s.format("%Y-%m-%d").to_string(), g, ratio));
            labels.push(format!("−{back}"));
            // Закрыта — значит УЛОЖИЛСЯ: доля не больше единицы.
            met.push(ratio.map(|r| r <= 1.0));
        }
    }
    let missed = met.iter().filter(|m| **m == Some(false)).count() as u32;
    super::indicators::IndicatorSeries {
        key: "red_meat",
        state: indicator_state().await,
        days: points,
        met_days: met,
        missed,
        labels,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::services::indicators::IndicatorState;

    #[test]
    fn planka_v_belke_sootvetstvuet_semisti_grammam() {
        assert_eq!(weekly_limit_protein_g(), 140.0);
        assert_eq!(raw_grams_from_protein(140.0), 700.0);
    }

    /// Готовое мясо плотнее по белку, и в сырой эквивалент оно разворачивается
    /// БОЛЬШИМ весом — ради этого счёт и идёт через белок.
    #[test]
    fn gotovoe_myaso_vesit_bolshe_v_syrom_ekvivalente() {
        // 150 г тушёной говядины, 27 г белка на 100 г → 40.5 г белка.
        let raw = raw_grams_from_protein(150.0 * 0.27);
        assert!((raw - 202.5).abs() < 1e-9, "получилось {raw}");
    }

    fn week(grams: f64, day: u32) -> WeeklyRedMeat {
        WeeklyRedMeat { grams, limit: weekly_limit_g(), day_of_week: day }
    }

    #[test]
    fn shkala_krasneet_kogda_planka_vybrana() {
        assert_eq!(week(700.0, 3).state(), IndicatorState::Red);
        assert_eq!(week(900.0, 7).state(), IndicatorState::Red);
    }

    #[test]
    fn shkala_preduprezhdaet_pri_bystrom_tempe() {
        // Третий день, съедено больше трёх седьмых планки (300 > 300 — нет, 350 > 300 — да).
        assert_eq!(week(350.0, 3).state(), IndicatorState::Orange);
        // Тот же вес, но к концу недели — уложился, предупреждать не о чем.
        assert_eq!(week(350.0, 7).state(), IndicatorState::Green);
    }

    #[test]
    fn rovnyj_temp_zelenyj() {
        assert_eq!(week(100.0, 1).state(), IndicatorState::Green);
        assert_eq!(week(300.0, 3).state(), IndicatorState::Green);
        assert_eq!(week(0.0, 5).state(), IndicatorState::Green);
    }
}
