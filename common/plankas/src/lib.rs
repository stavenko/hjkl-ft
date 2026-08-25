//! Правила планок: формулы, по которым приложение их считает.
//!
//! Крейт общий у приложения худеющего и у кураторского. Куратор должен ВИДЕТЬ
//! число до отправки — значит считать его надо у него, по присланному отчёту, тем
//! же кодом, каким считает приложение. Вторая копия этих формул разошлась бы с
//! первой на первой же правке, и разошлась бы молча: числа выглядят правдоподобно
//! в обоих случаях.
//!
//! Здесь только ЧИСТЫЕ функции — ни базы, ни сети, ни времени. Всё, что зависит от
//! данных человека, приходит параметрами.

pub mod defaults;
pub mod weight_trend;

pub use defaults::{default_for, Kind, Snapshot, ALL};
pub use weight_trend::{Direction, WeightTrend, CONFIDENT, DEFAULT_WINDOW_DAYS, WEAK};

/// Пол. Нужен нормам, которые от него зависят (овощи-фрукты, железо, безжировая
/// масса). Живёт здесь, а не в профиле приложения, потому что этими нормами
/// пользуются оба приложения.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Sex {
    Male,
    Female,
}

/// Цвет индикатора. Переехал сюда вместе с `next_steps_planka`: подъём планки
/// шагов решается по нему, и без него формула не переносима.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum IndicatorState {
    Green,
    Orange,
    Red,
    Unknown,
}

// ── Что предложить куратору ──────────────────────────────────────────────────

/// Ширина коридора, в котором день по калориям считается зелёным. Она же — порог
/// «держался планки» при недельном пересчёте.
pub const CALORIE_BAND_KCAL: f64 = 50.0;

/// Пересчёт планок по последним данным человека — то, что куратор видит, нажав
/// «Рассчитать».
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Suggestion {
    pub calories: f64,
    /// Белок следует за калориями. `None` — профиль неполон, и выдумывать норму
    /// не из чего.
    pub protein: Option<f64>,
}

/// Посчитать так же, как посчитал бы недельный цикл у худеющего.
///
/// Это НЕ отдельное правило для куратора: он видит ровно то число, к которому
/// приложение пришло бы само, и решает, согласен ли он с ним. Второе правило
/// означало бы, что человек и его куратор ведут разные программы.
///
/// `previous` — планка, от которой отталкиваемся (действующая). `avg_kcal_7d` —
/// сколько человек ел на самом деле; без него исполнение неизвестно, и стопор не
/// срабатывает ни в какую сторону.
pub fn suggest(
    s: &Snapshot,
    previous: f64,
    weight: &[api_types::WeightEntry],
    avg_kcal_7d: Option<f64>,
) -> Suggestion {
    let trend = weight_trend::weight_trend(weight, DEFAULT_WINDOW_DAYS);
    let weight_kg = s.weight_kg.unwrap_or(0.0);
    let adh = adherence(avg_kcal_7d.unwrap_or(previous), previous, CALORIE_BAND_KCAL);
    let calories = calorie_planka_weekly(previous, &trend, weight_kg, adh);
    // Белок считается уже от НОВОЙ калорийности: куратор отправит их вместе, и
    // показывать норму от старой планки значило бы показывать неправду.
    let after = Snapshot { kcal_planka: Some(calories), ..*s };
    Suggestion { calories, protein: default_for(Kind::Protein, &after) }
}

// ── Планка по калориям ───────────────────────────────────────────────────────

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
pub fn planka_factor(trend: &crate::weight_trend::WeightTrend, weight_kg: f64) -> f64 {
    use crate::weight_trend::{Direction, WeightTrend, CONFIDENT, WEAK};
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
    trend: &crate::weight_trend::WeightTrend,
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
    trend: &crate::weight_trend::WeightTrend,
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

// ── Планка по шагам ──────────────────────────────────────────────────────────

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
pub fn next_steps_planka(current: u32, state: IndicatorState) -> u32 {
    use IndicatorState;
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

// ── Норма белка ──────────────────────────────────────────────────────────────

/// Body Mass Index = weight(kg) / height(m)². `None` if height is not a positive
/// value. Used as a coarse read on how much of the body mass is fat.
pub fn bmi(weight_kg: f64, height_cm: f64) -> Option<f64> {
    if height_cm <= 0.0 {
        return None;
    }
    let m = height_cm / 100.0;
    Some(weight_kg / (m * m))
}

/// Точка перегиба: до неё белок берётся постоянной долей калорий, после — растёт
/// медленнее калорий.
pub const PROTEIN_ANCHOR_KCAL: f64 = 1800.0;
/// Сколько граммов белка приходится на точку перегиба. 135 г = ровно 30 % от 1800
/// ккал: рекомендации для похудения называют 25–35 % калорий из белка, и на
/// умеренном калораже мы берём середину. Смысл планки — не «покрыть потребность»
/// (её закрывают куда меньшие цифры), а НАСЫТИТЬ: белок утоляет голод лучше
/// остальных макронутриентов, и заниженная планка делает показатель бесполезным.
pub const PROTEIN_ANCHOR_G: f64 = 135.0;
/// Показатель степени, с которой ДОЛЯ белка убывает после точки перегиба.
///
/// Считается из пары якорей: `k = ln(p1/p0) / ln(E1/E0)`. Здесь — из 30 % при 1800
/// ккал и 20 % при 3600: `ln(0.20/0.30) / ln(3600/1800) = −0.5850`.
///
/// Допустимый диапазон — `−1 ≤ k < 0`, и он не формальность, а условие
/// осмысленности: при `k = 0` доля постоянна, при `k = −1` граммы перестают расти
/// вовсе, а при `k < −1` они бы УБЫВАЛИ с ростом калоража. Проверяется тестом
/// [`tests::pokazatel_v_dopustimom_diapazone`].
pub const PROTEIN_CURVE_K: f64 = -0.5850;
/// Калорийность белка.
const KCAL_PER_G_PROTEIN: f64 = 4.0;
/// Нижняя граница: столько граммов на кг БЕЗЖИРОВОЙ массы. Страхует случай
/// экстремально низкой калорийной планки — доля от маленького числа не должна
/// опускать белок ниже физиологического минимума.
pub const PROTEIN_MIN_PER_KG_FFM: f64 = 1.6;
/// Верхняя граница: столько граммов на кг ПОЛНОГО веса. Страхует обратный случай
/// (высокая планка у некрупного человека) — дальше это уже не еда, а задание.
pub const PROTEIN_MAX_PER_KG_BW: f64 = 2.2;

/// Оценка БЕЗЖИРОВОЙ массы тела (кг) по уравнению Deurenberg (1991): процент жира
/// выводится из ИМТ, возраста и пола, то есть в предположении обычного,
/// НЕтренированного состава тела.
///
/// ```text
/// BF%  = 1.2·BMI + 0.23·age − 10.8·sex − 5.4      (sex: 1 муж., 0 жен.)
/// FFM  = weight · (1 − BF%/100)
/// ```
///
/// `None`, если вес/рост неположительны (ИМТ не определён). Процент жира зажат в
/// физиологические [3, 60] %, чтобы экстраполяция за пределы применимости не дала
/// нелепую (или отрицательную) массу.
pub fn fat_free_mass_kg(weight_kg: f64, height_cm: f64, age_years: i32, sex: Sex) -> Option<f64> {
    if weight_kg <= 0.0 {
        return None;
    }
    let bmi = bmi(weight_kg, height_cm)?;
    let sex_term = match sex {
        Sex::Male => 1.0,
        Sex::Female => 0.0,
    };
    let bf_pct = (1.2 * bmi + 0.23 * age_years as f64 - 10.8 * sex_term - 5.4).clamp(3.0, 60.0);
    Some(weight_kg * (1.0 - bf_pct / 100.0))
}

/// Сколько граммов белка полагается на калорийную планку `kcal` — БЕЗ поправок на
/// тело.
///
/// ```text
/// база = P0 · kcal / E0                     при kcal ≤ E0
/// база = P0 · (kcal / E0)^(1 + k)           при kcal >  E0
/// ```
///
/// Постоянная доля ломается на краях: на низком калораже 30 % дают завышенные
/// граммы, а если долю просто снижать ступенями, граммы становятся НЕмонотонными —
/// человек с бо́льшим калоражем получает меньше белка. Причина арифметическая:
/// граммы равны `E · доля / 4`, и стоит доле убывать быстрее, чем `1/E`, как
/// произведение начинает падать.
///
/// Отсюда степенная зависимость: доля убывает, а граммы всё равно растут — ровно
/// пока `k` лежит в `[−1, 0)`. Ниже точки перегиба доля постоянна (`P0/E0 · 4` =
/// 30 %), выше — падает до 20 % к 3600 ккал.
///
/// В самой точке перегиба ветви сходятся: обе дают `P0`. Излом первой производной
/// там остаётся (плато переходит в спад) — на цифры он не влияет, сглаживание
/// отдельной задачей, если понадобится.
pub fn protein_from_kcal(kcal: f64) -> f64 {
    if kcal <= PROTEIN_ANCHOR_KCAL {
        PROTEIN_ANCHOR_G * kcal / PROTEIN_ANCHOR_KCAL
    } else {
        PROTEIN_ANCHOR_G * (kcal / PROTEIN_ANCHOR_KCAL).powf(1.0 + PROTEIN_CURVE_K)
    }
}

/// Какой ДОЛЕЙ калорийной планки оказалась планка по белку, в процентах.
///
/// Величина производная: доля больше не задана числом, а получается из граммов.
/// Нужна, чтобы объяснение на дашборде называло тот процент, который вышел на
/// самом деле, а не заученные 30 %.
pub fn protein_share_pct(kcal: f64) -> f64 {
    if kcal <= 0.0 {
        return 0.0;
    }
    KCAL_PER_G_PROTEIN * 100.0 * protein_from_kcal(kcal) / kcal
}

/// Дневная планка по белку (граммы) — от КАЛОРИЙНОЙ ПЛАНКИ по кривой
/// [`protein_from_kcal`], зажатая между двумя границами, считающимися от тела:
///
/// ```text
/// база   = protein_from_kcal(планка_ккал)
/// пол    = 1.6 · FFM          (безжировая масса, Deurenberg)
/// потолок = 2.2 · вес
/// target = round(clamp(база, пол, потолок))
/// ```
///
/// Пол всегда ниже потолка (1.6·FFM ≤ 1.6·вес < 2.2·вес), так что зажим корректен
/// при любом составе тела.
///
/// `kcal_planka` = `None` (планки по калориям ещё нет — до конца онбординга), либо
/// неположительна → берём пол: это ровно прежнее правило 1.6 г на кг безжировой
/// массы, то есть до появления калорийной планки поведение не меняется.
///
/// `None`, если вес/рост неположительны.
pub fn protein_target_g(
    kcal_planka: Option<f64>,
    weight_kg: f64,
    height_cm: f64,
    age_years: i32,
    sex: Sex,
) -> Option<u32> {
    let ffm = fat_free_mass_kg(weight_kg, height_cm, age_years, sex)?;
    let floor = PROTEIN_MIN_PER_KG_FFM * ffm;
    let ceiling = PROTEIN_MAX_PER_KG_BW * weight_kg;
    let base = match kcal_planka {
        Some(k) if k > 0.0 => protein_from_kcal(k),
        _ => floor,
    };
    Some(base.clamp(floor, ceiling).round() as u32)
}

#[cfg(test)]
mod suggest_tests {
    use super::*;
    use api_types::WeightEntry;

    fn weigh(date: &str, kg: f64) -> WeightEntry {
        WeightEntry {
            id: date.to_string(),
            date: date.to_string(),
            weight_kg: kg,
            no_water: false,
            no_food: false,
            no_wash: false,
            used_toilet: false,
            morning: true,
            created_at: String::new(),
            updated_at: String::new(),
        }
    }

    fn person() -> Snapshot {
        Snapshot {
            sex: Some(Sex::Female),
            age_years: Some(35),
            height_cm: Some(165.0),
            weight_kg: Some(70.0),
            kcal_planka: Some(2000.0),
        }
    }

    /// Предложение куратору обязано совпадать с тем, что посчитал бы недельный
    /// цикл. Иначе человек и его куратор ведут разные программы.
    #[test]
    fn predlozhenie_sovpadaet_s_nedelnym_pereschetom() {
        let w: Vec<_> = (1..=14)
            .map(|i| weigh(&format!("2026-03-{i:02}"), 71.0 - i as f64 * 0.05))
            .collect();
        let trend = weight_trend::weight_trend(&w, DEFAULT_WINDOW_DAYS);
        let adh = adherence(1900.0, 2000.0, CALORIE_BAND_KCAL);
        let expected = calorie_planka_weekly(2000.0, &trend, 70.0, adh);
        assert_eq!(suggest(&person(), 2000.0, &w, Some(1900.0)).calories, expected);
    }

    /// Белок считается от НОВОЙ калорийности, а не от прежней: отправляются они
    /// вместе, и показать норму от старой планки значило бы показать неправду.
    #[test]
    fn belok_schitaetsya_ot_novoj_kalorijnosti() {
        let s = person();
        let sg = suggest(&s, 2000.0, &[], Some(2000.0));
        let after = Snapshot { kcal_planka: Some(sg.calories), ..s };
        assert_eq!(sg.protein, default_for(Kind::Protein, &after));
    }

    /// Без данных о съеденном стопор не срабатывает ни в какую сторону: неизвестное
    /// исполнение — не повод ни поднимать, ни опускать.
    #[test]
    fn bez_sedennogo_stopor_ne_srabatyvaet() {
        let s = person();
        let no_data = suggest(&s, 2000.0, &[], None);
        let on_target = suggest(&s, 2000.0, &[], Some(2000.0));
        assert_eq!(no_data.calories, on_target.calories);
    }
}

#[cfg(test)]
mod tests {
    use super::{
        adherence, calorie_planka, calorie_planka_weekly, next_steps_planka, planka_factor,
        steps_planka_for_avg, Adherence, IndicatorState, PLANKA_STEP, STEPS_PLANKA_MAX,
    };
    use crate::weight_trend::{Direction, WeightTrend};


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
                use IndicatorState::{Green, Orange, Red, Unknown};

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
