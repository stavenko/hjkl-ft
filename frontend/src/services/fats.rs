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

/// App-flag: день, С КОТОРОГО недели железа засчитываются в гейт жиров.
///
/// Без него гейт смотрел на всю прошлую историю, и у человека, закрывшего планку по
/// железу до появления жиров, дверь открывалась в ту же секунду, что и обновление.
/// Правило должно быть «закрой планку и дождись конца недели», а не «когда-то
/// закрывал». Якорь ставится при первом запуске, когда железо уже открыто, а жиры
/// ещё нет, — и с этого дня считаются только те недели железа, что закончились ПОСЛЕ.
pub const FAT_GATE_ANCHOR_KEY: &str = "fat_gate_anchor";

/// День, с которого считается гейт жиров. `None` — якорь ещё не поставлен.
pub fn gate_anchor() -> Option<NaiveDate> {
    app_flags::get(FAT_GATE_ANCHOR_KEY)
        .and_then(|s| NaiveDate::parse_from_str(&s, "%Y-%m-%d").ok())
}

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

/// Кислоты за день ДЛЯ БАЛАНСА — без жира в целых молочно-жировых глобулах
/// (см. [`local::balance_acids_on`]).
pub async fn balance_acids_on(date: &str) -> FattyAcids {
    local::balance_acids_on(date).await
}

async fn fatty_acids_between(from: NaiveDate, to: NaiveDate) -> FattyAcids {
    acids_between(from, to, false).await
}

async fn balance_acids_between(from: NaiveDate, to: NaiveDate) -> FattyAcids {
    acids_between(from, to, true).await
}

async fn acids_between(from: NaiveDate, to: NaiveDate, for_balance: bool) -> FattyAcids {
    let mut total = FattyAcids::default();
    let mut d = from;
    while d <= to {
        let day = d.format("%Y-%m-%d").to_string();
        total += if for_balance { balance_acids_on(&day).await } else { fatty_acids_on(&day).await };
        d += Duration::days(1);
    }
    total
}

/// Ход текущей недели — для трёх шкал в виджете.
#[derive(Clone)]
pub struct WeeklyFats {
    /// ВЕСЬ жир недели — из него считается омега-3.
    pub acids: FattyAcids,
    /// Жир БЕЗ молочных глобул — из него считается баланс. Две суммы, потому что
    /// у двух индикаторов разные вопросы: сколько длинных омега-3 съедено и каков
    /// состав того жира, который на баланс влияет.
    pub balance_acids: FattyAcids,
    pub epa_dha_target: f64,
    /// 1…7 — какой сегодня день недели жира.
    pub day_of_week: u32,
}

impl WeeklyFats {
    /// Отношение (МНЖК+ПНЖК)/НЖК за неделю. Считается из СУММ за неделю, а не
    /// средним по продуктам: ложка оливкового масла с её отношением 5.7 иначе
    /// перевесила бы двести граммов сала.
    pub fn ratio(&self) -> Option<f64> {
        self.balance_acids.unsat_to_sat()
    }
}

pub async fn weekly_progress() -> Option<WeeklyFats> {
    let today = local::today_date();
    let (start, _end) = week_bounds(today)?;
    Some(WeeklyFats {
        acids: fatty_acids_between(start, today).await,
        balance_acids: balance_acids_between(start, today).await,
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

    /// Значение за неделю по этому индикатору. `None` — судить НЕЧЕГО.
    ///
    /// Пустая неделя даёт `None`, а не ноль, и это принципиально: ноль означал бы
    /// «человек не ел омега-3», тогда как на самом деле у продуктов ещё нет профиля
    /// жира. На этом уже обожглись: после открытия жиров все прошлые недели
    /// покрасились красным, хотя мерить в них было нечего.
    fn weekly_value(self, acids: &FattyAcids) -> Option<f64> {
        if !acids.has_data() {
            return None;
        }
        match self {
            Fat::EpaDha => Some(acids.epa_dha_g),
            // Отношение без насыщенных не определено — судить нечего.
            Fat::Ratio => acids.unsat_to_sat(),
        }
    }

    /// Кислоты за отрезок ДЛЯ ЭТОГО индикатора: у баланса — без молочных глобул,
    /// у омега-3 — весь жир. Спрашивать «какие кислоты» в отрыве от «зачем» нельзя:
    /// два индикатора считают разные суммы.
    async fn acids_between(self, from: NaiveDate, to: NaiveDate) -> FattyAcids {
        match self {
            Fat::EpaDha => fatty_acids_between(from, to).await,
            Fat::Ratio => balance_acids_between(from, to).await,
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
        // Неделя без данных о жире не судится — как и неделя без дневника. Иначе
        // «профиль ещё не выяснен» читалось бы как «рацион плохой».
        let acids = which.acids_between(s, e).await;
        let Some(v) = which.weekly_value(&acids) else { continue };
        history.push(v >= which.target());
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
            let acids = which.acids_between(s, e).await;
            // У баланса столбик показывает ОТКЛОНЕНИЕ от нормы со знаком — ровно то
            // же, что и шкала в виджете. Показывать там сырое отношение значило бы
            // говорить о показателе двумя разными языками на соседних экранах.
            let raw = which.weekly_value(&acids);
            let value = match which {
                Fat::Ratio => raw.map(|v| v - which.target()).unwrap_or(0.0),
                Fat::EpaDha => raw.unwrap_or(0.0),
            };
            let ratio = logged.then(|| raw.map(|v| v / which.target())).flatten();
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

/// Была ли ХОТЬ ОДНА закрытая по ОМЕГА-3 неделя жира с тех пор, как тема открылась
/// — гейт для следующего звена цепочки.
///
/// Спрашивается только один индикатор из двух, и намеренно. Омега-3 человек закрывает
/// действием, которое от него зависит целиком: съесть рыбу дважды за неделю. Баланс —
/// свойство всего рациона; требовать его сразу значило бы держать человека взаперти за
/// то, что перестраивается месяцами, а не за то, что он сделал или не сделал на этой
/// неделе. Баланс при этом никуда не девается: он виден, судится и остаётся целью.
///
/// Отсчёт идёт от `fat_week_opened_at` — дня, когда тема жиров открылась ИМЕННО У
/// ЭТОГО человека. Никакого внешнего якоря по дате выката здесь не нужно и быть не
/// должно: правило самодостаточно и не зависит от того, когда мы выпустили
/// следующую главу. (У гейта жиров якорь был вынужденным — железо открылось раньше,
/// чем появился механизм его недель, и дату открытия взять было неоткуда.)
///
/// «Хотя бы одна», а не «последняя»: тема пройдена, когда человек однажды сделал то,
/// чему она учит. Требовать, чтобы он держал её и в последнюю неделю, значило бы
/// закрывать дальнейший путь за одну сорвавшуюся неделю — и тем сильнее, чем дольше
/// человек с нами.
///
/// Недели без дневника пропускаются: их не было, и провалом они быть не могут.
pub async fn week_closed_since_open() -> bool {
    let today = local::today_date();
    let (Some(open), Some((cur_start, _))) = (week_open_date(), week_bounds(today)) else {
        // Отказ по отсутствию даты открытия — не «человек не справился», а поломка
        // состояния, и молчать о ней нельзя: гейт тогда закрыт навсегда.
        crate::services::telemetry::report_internal(
            "gate.fats_week_closed",
            "",
            &format!("нет данных: open={:?}", week_open_date()),
        );
        return false;
    };
    let diary_days: std::collections::HashSet<String> =
        local::list_diary_dates().await.into_iter().collect();
    // От первой недели темы до последней ЗАВЕРШЁННОЙ. Текущая не судится: «ещё не
    // набрал» — не провал.
    let mut s = open;
    let mut seen = 0u32;
    let mut best = 0.0_f64;
    while s < cur_start {
        let e = s + Duration::days(6);
        let logged = (0..7)
            .any(|d| diary_days.contains(&(s + Duration::days(d)).format("%Y-%m-%d").to_string()));
        let acids = fatty_acids_between(s, e).await;
        if logged {
            seen += 1;
            best = best.max(acids.epa_dha_g);
            if Fat::EpaDha.met(&acids) {
                return true;
            }
        }
        s += Duration::days(7);
    }
    // Дошли до конца, не найдя ни одной закрытой недели. Отчёт — чтобы «у меня не
    // открылось» разбиралось по числам, а не по догадкам.
    crate::services::telemetry::report_internal(
        "gate.fats_week_closed",
        "",
        &format!(
            "закрытых недель нет: открыто {open}, текущая с {cur_start}, недель с дневником \
             {seen}, лучший EPA+DHA за неделю {best:.2} при норме {:.2}",
            EPA_DHA_PER_WEEK_G
        ),
    );
    false
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
