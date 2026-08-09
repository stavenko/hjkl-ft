//! Жиры: два недельных индикатора — EPA+DHA и баланс жира.
//!
//! Меряется не «сколько жира», а КАКОГО жира. Два вопроса, на которые по отдельности
//! не ответить:
//!
//! * **EPA+DHA** — длинные морские омега-3. Организм делает их из растительной АЛК с
//!   конверсией в единицы процентов, поэтому растительным маслом их не закрыть.
//! * **Баланс** — отношение (МНЖК+ПНЖК)/НЖК. Не «сколько», а «какой»: то самое
//!   отношение, по которому средиземноморский рацион отличается от обычного.
//!
//! Индикатора по АЛК здесь НЕТ: он был написан, но выпускать его не стали. Вместе с
//! ним из запроса профиля убрана и доля АЛК — спрашивать у модели то, чего никто не
//! читает, незачем.
//!
//! Оба НЕДЕЛЬНЫЕ. Съесть недельную норму EPA+DHA за один приём — нормально
//! (порция скумбрии), требовать её ежедневно бессмысленно.
//!
//! Величины берутся из профиля жира продукта (`api_types::FatProfile`): доли от жира
//! × наш собственный `Food::fat`. Ни одного числа, полученного от модели дважды.
//!
//! Неделя и момент открытия — СВОИ, отсчитываются от дня, когда жиры открылись
//! (после закрытой планки железа). Границы недель железа тут не годятся: жиры
//! открываются позже, и первая их неделя началась бы задним числом.

use chrono::{Duration, NaiveDate};

use super::{app_flags, indicators, local};
use api_types::FattyAcids;

/// App-flag: жиры открыты (три шкалы и три индикатора видны).
pub const FAT_UNLOCKED_KEY: &str = "fat_week_unlocked";

/// App-flag: день, от которого катится сетка недель жира.
pub const FAT_WEEK_OPEN_KEY: &str = "fat_week_opened_at";

// ── Нормы ────────────────────────────────────────────────────────────────────
//
// Проверены на типичном средиземноморском дне НАШЕЙ ЖЕ таблицей профилей — той
// проверкой, которой не прошло железо и из-за которой женская планка там оказалась
// недостижимой. День (оливковое масло, грецкий орех, курица, крупы, сыр) даёт НЖК
// 12.7, МНЖК 27.4, ПНЖК 17.5 — отношение 3.5. Две рыбные трапезы дают 7.6 г
// EPA+DHA. Обе нормы закрываются с запасом.

/// Недельная норма EPA+DHA, граммы. 250 мг/сут — AI, а не RDA: у длинных омега-3
/// нормируется достаточное потребление, верхнего края потребности тут нет.
pub const EPA_DHA_PER_WEEK_G: f64 = 1.75;

/// Минимальное отношение (МНЖК+ПНЖК)/НЖК.
pub const UNSAT_TO_SAT_MIN: f64 = 2.0;

// ── Открытие и неделя ────────────────────────────────────────────────────────

/// Открыты ли жиры.
pub fn unlocked() -> bool {
    app_flags::get_bool(FAT_UNLOCKED_KEY)
}

fn week_open_date() -> Option<NaiveDate> {
    app_flags::get(FAT_WEEK_OPEN_KEY)
        .and_then(|s| NaiveDate::parse_from_str(&s, "%Y-%m-%d").ok())
}

/// Неделя жира, в которую попадает `today`, как `(первый день, последний)`
/// включительно. Недели идут семидневными шагами от дня открытия, поэтому день 1
/// каждой недели — тот же день недели, с которого человек начал.
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

/// Жирные кислоты за день, в граммах. Продукты без профиля не вносят ничего — их
/// не додумывают. Блюда раскрываются по составу общим механизмом.
pub async fn fatty_acids_on(date: &str) -> FattyAcids {
    local::fatty_acids_on(date).await
}

async fn fatty_acids_between(from: NaiveDate, to: NaiveDate) -> FattyAcids {
    let mut total = FattyAcids::default();
    let mut d = from;
    while d <= to {
        total += fatty_acids_on(&d.format("%Y-%m-%d").to_string()).await;
        d += Duration::days(1);
    }
    total
}

/// Ход текущей недели — для трёх шкал в виджете.
#[derive(Clone)]
pub struct WeeklyFats {
    pub acids: FattyAcids,
    pub epa_dha_target: f64,
    /// 1…7 — какой сегодня день недели жира.
    pub day_of_week: u32,
}

impl WeeklyFats {
    /// Отношение (МНЖК+ПНЖК)/НЖК за неделю. Считается из СУММ за неделю, а не
    /// средним по продуктам: ложка оливкового масла с её отношением 5.7 иначе
    /// перевесила бы двести граммов сала.
    pub fn ratio(&self) -> Option<f64> {
        self.acids.unsat_to_sat()
    }
}

pub async fn weekly_progress() -> Option<WeeklyFats> {
    let today = local::today_date();
    let (start, _end) = week_bounds(today)?;
    Some(WeeklyFats {
        acids: fatty_acids_between(start, today).await,
        epa_dha_target: EPA_DHA_PER_WEEK_G,
        day_of_week: (today - start).num_days() as u32 + 1,
    })
}

/// Какой из трёх индикаторов считаем.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Fat {
    EpaDha,
    Ratio,
}

impl Fat {
    pub fn key(self) -> &'static str {
        match self {
            Fat::EpaDha => "epa_dha",
            Fat::Ratio => "fat_ratio",
        }
    }

    /// Значение за неделю по этому индикатору.
    fn weekly_value(self, acids: &FattyAcids) -> Option<f64> {
        match self {
            Fat::EpaDha => Some(acids.epa_dha_g),
            // Отношение без насыщенных не определено — судить нечего.
            Fat::Ratio => acids.unsat_to_sat(),
        }
    }

    /// Норма, с которой сравнивается значение.
    pub fn target(self) -> f64 {
        match self {
            Fat::EpaDha => EPA_DHA_PER_WEEK_G,
            Fat::Ratio => UNSAT_TO_SAT_MIN,
        }
    }

    /// Закрыта ли неделя по этому индикатору.
    fn met(self, acids: &FattyAcids) -> bool {
        matches!(self.weekly_value(acids), Some(v) if v >= self.target())
    }
}

// ── Индикаторы ───────────────────────────────────────────────────────────────

/// Цвет индикатора. Меряется своё, но вердикт выносит ОБЩЕЕ недельное правило — то
/// же, что у железа и гема: по ЗАВЕРШЁННЫМ неделям, окно восемь недель.
///
/// Неделя, в которую человек не вёл дневник, не судится: её просто не было.
pub async fn indicator_state(which: Fat) -> indicators::IndicatorState {
    let today = local::today_date();
    let Some((cur_start, _)) = week_bounds(today) else {
        return indicators::IndicatorState::Unknown;
    };

    let diary_days: std::collections::HashSet<String> =
        local::list_diary_dates().await.into_iter().collect();
    let mut history: Vec<bool> = Vec::new();
    let mut s = cur_start;
    for _ in 0..indicators::WEEKLY_WINDOW {
        s -= Duration::days(7);
        let e = s + Duration::days(6);
        let logged = (0..7).any(|d| {
            diary_days.contains(&(s + Duration::days(d)).format("%Y-%m-%d").to_string())
        });
        if !logged {
            continue;
        }
        history.push(which.met(&fatty_acids_between(s, e).await));
    }
    history.reverse();
    indicators::weekly_state(&history)
}

/// Столбики за последние завершённые недели. Неделя без дневника остаётся в ряду
/// БЕЗ доли: столбика нет, вердикта нет, подпись на месте — иначе сетки трёх
/// индикаторов разъехались бы между собой и с железом.
pub async fn weekly_series(which: Fat) -> indicators::IndicatorSeries {
    let today = local::today_date();
    let mut points: Vec<(String, f64, Option<f64>)> = Vec::new();
    let mut labels: Vec<String> = Vec::new();
    let mut met: Vec<Option<bool>> = Vec::new();
    if let Some((cur_start, _)) = week_bounds(today) {
        let diary_days: std::collections::HashSet<String> =
            local::list_diary_dates().await.into_iter().collect();
        let window = indicators::WEEKLY_WINDOW as i64;
        for back in (1..=window).rev() {
            let s = cur_start - Duration::days(7 * back);
            let e = s + Duration::days(6);
            let logged = (0..7).any(|d| {
                diary_days.contains(&(s + Duration::days(d)).format("%Y-%m-%d").to_string())
            });
            let acids = fatty_acids_between(s, e).await;
            let value = which.weekly_value(&acids).unwrap_or(0.0);
            let ratio = logged
                .then(|| which.weekly_value(&acids).map(|v| v / which.target()))
                .flatten();
            points.push((s.format("%Y-%m-%d").to_string(), value, ratio));
            labels.push(format!("−{back}"));
            met.push(ratio.map(|r| r >= 1.0));
        }
    }
    let missed = met.iter().filter(|m| **m == Some(false)).count() as u32;
    indicators::IndicatorSeries {
        key: which.key(),
        state: indicator_state(which).await,
        days: points,
        met_days: met,
        missed,
        labels,
    }
}

/// Закрыта ли ПОСЛЕДНЯЯ завершённая неделя жира по ОМЕГА-3 — гейт для следующего
/// звена цепочки.
///
/// Спрашивается только один индикатор из двух, и намеренно. Омега-3 человек закрывает
/// действием, которое от него зависит целиком: съесть рыбу дважды за неделю. Баланс —
/// свойство всего рациона; требовать его сразу значило бы держать человека взаперти за
/// то, что перестраивается месяцами, а не за то, что он сделал или не сделал на этой
/// неделе. Баланс при этом никуда не девается: он виден, судится и остаётся целью.
pub async fn week_closed() -> bool {
    let today = local::today_date();
    let Some((cur_start, _)) = week_bounds(today) else {
        return false;
    };
    let s = cur_start - Duration::days(7);
    let e = s + Duration::days(6);
    let diary_days: std::collections::HashSet<String> =
        local::list_diary_dates().await.into_iter().collect();
    let logged = (0..7)
        .any(|d| diary_days.contains(&(s + Duration::days(d)).format("%Y-%m-%d").to_string()));
    if !logged {
        return false;
    }
    Fat::EpaDha.met(&fatty_acids_between(s, e).await)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn acids(sfa: f64, mufa: f64, pufa: f64, epa_dha: f64) -> FattyAcids {
        FattyAcids { sfa_g: sfa, mufa_g: mufa, pufa_g: pufa, epa_dha_g: epa_dha }
    }

    #[test]
    fn otnoshenie_schitaetsya_iz_summ_a_ne_srednim() {
        // Ложка оливкового масла (отношение 5.7) и двести граммов сала (1.4).
        // Среднее отношений дало бы 3.5 — «хорошо». Отношение сумм даёт 1.6.
        let olive = acids(4.5, 21.0, 4.5, 0.0);
        let lard = acids(73.0, 92.0, 24.0, 0.0);
        let sum = olive + lard;
        let by_sums = sum.unsat_to_sat().unwrap();
        assert!(by_sums < 2.0, "по суммам {by_sums:.2} — планка не закрыта");
    }

    #[test]
    fn bez_nasyshchennyh_otnoshenie_ne_opredeleno() {
        assert!(acids(0.0, 10.0, 5.0, 0.0).unsat_to_sat().is_none());
    }

    #[test]
    fn epa_dha_zakryvaetsya_dvumya_rybnymi_trapezami() {
        // Скумбрия 150 г: жир 13.9 г на 100 г, EPA+DHA 18 % жира.
        let per_meal = 13.9 * 1.5 * 0.18;
        assert!(per_meal * 2.0 > EPA_DHA_PER_WEEK_G);
    }

    #[test]
    fn indikatory_sudyat_raznoe() {
        // Много рыбы, но жир при этом плохой: отношение ниже двух. Омега-3 закрыта,
        // баланс — нет. Индикаторы отвечают на разные вопросы, и один не искупает
        // другого; при этом СЛЕДУЮЩУЮ главу открывает только омега-3 (см.
        // `week_closed`), потому что она зависит от одного поступка, а баланс — от
        // всего рациона.
        let bad_ratio = acids(50.0, 40.0, 30.0, 3.0);
        assert!(Fat::EpaDha.met(&bad_ratio));
        assert!(!Fat::Ratio.met(&bad_ratio));
    }
}
