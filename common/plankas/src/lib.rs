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
/// сколько человек ел на самом деле.
///
/// Без `avg_kcal_7d` планка СТОИТ. Раньше отсутствие этой величины наоборот
/// снимало стопор и пускало вес двигать планку свободно — ровно наоборот тому,
/// ради чего стопор заводился: пока неизвестно, исполнялась ли планка, вес
/// говорит не о планке, а о том, сколько человек ел на самом деле.
pub fn suggest(
    s: &Snapshot,
    previous: f64,
    weight: &[api_types::WeightEntry],
    avg_kcal_7d: Option<f64>,
) -> Suggestion {
    let Some(avg) = avg_kcal_7d else {
        return Suggestion { calories: previous, protein: default_for(Kind::Protein, s) };
    };
    let trend = weight_trend::weight_trend(weight, DECISION_WINDOW_DAYS);
    let weight_kg = s.weight_kg.unwrap_or(0.0);
    let adh = adherence(avg, previous, CALORIE_BAND_KCAL);
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
//
// Куда двигать — решает ОДНА величина: темп снижения против комфортной полосы
// (0.3–0.7 % массы тела в неделю). Значимо быстрее полосы → +5 % (беречь мышцы),
// значимо медленнее → −5 % (создать дефицит), иначе → держим. «Стоит» и «растёт»
// отдельными случаями не нужны: и то и другое значимо медленнее полосы.
//
// Ключевое слово здесь — ЗНАЧИМО, и оно появилось не из любви к статистике.
// См. `pace`.
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

/// Темп снижения веса против комфортной полосы — то единственное, что решает,
/// куда двигать планку.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Pace {
    /// Значимо быстрее верхней границы полосы.
    Fast,
    /// Значимо медленнее нижней границы — сюда же попадают «стоит» и «растёт».
    Slow,
    /// Всё остальное: либо темп в полосе, либо данные не позволяют отличить его
    /// от полосы. Решение одно и то же — не трогать планку.
    Comfortable,
}

/// Как темп снижения соотносится с комфортной полосой — С ОГЛЯДКОЙ НА
/// ПОГРЕШНОСТЬ САМОЙ ОЦЕНКИ.
///
/// # Зачем оглядка
///
/// Полоса узкая: при 85 кг это 0.26–0.60 кг в неделю, ширина — треть килограмма.
/// Погрешность наклона по четырнадцати взвешиваниям на бытовых весах — те же
/// 0.2 кг/нед. То есть полоса УЖЕ, чем погрешность числа, которое с ней
/// сравнивают, и сравнение точечной оценки с границей — подбрасывание монеты.
///
/// Живой случай (85 кг, ежедневные взвешивания). 29 августа то же самое окно в
/// 14 дней дало −0.21 кг/нед — на волосок ниже нижней границы, планку срезали на
/// 5 %. Через три дня то же окно (11 дней из 14 — те же самые!) дало −0.75 кг/нед,
/// выше верхней границы, планку подняли на 5 %. Дальше это повторялось каждую
/// неделю: 2650 → 2800 → 2950 → 2800 → 2950. Человек видит дребезжание и не
/// понимает, чему верить; вес при этом всё время снижался ровно.
///
/// Ни один из тех двух шагов не был обоснован: погрешность в обоих случаях
/// накрывала всю полосу целиком. Поэтому граница сравнивается не с точкой, а с
/// распределением: двигаем планку, только если ВЕРОЯТНОСТЬ того, что истинный
/// темп вне полосы, дошла до `CONFIDENT` — той же планки уверенности, по которой
/// мы уже решаем, снижается ли вес вообще. Неуверенность теперь означает
/// «держим», а не «шагнём наугад».
///
/// На тех же данных: 29.08 → P(медленнее) = 0.58, держим; 01.09 → P(быстрее) =
/// 0.79, держим; 05.09 → P(быстрее) = 0.98, поднимаем. Дребезжания нет, а
/// настоящий сигнал проходит.
///
/// `None` — судить не по чему: окно меньше трёх дней взвешиваний либо вес
/// неизвестен. Вызывающий держит планку.
pub fn pace(trend: &crate::weight_trend::WeightTrend, weight_kg: f64) -> Option<Pace> {
    use crate::weight_trend::CONFIDENT;
    if weight_kg <= 0.0 {
        return None;
    }
    // Границы полосы в кг/нед и со ЗНАКОМ: снижение — это отрицательный наклон.
    let fast = -COMFORT_LOSS_MAX * weight_kg;
    let slow = -COMFORT_LOSS_MIN * weight_kg;
    let p_fast = trend.p_slope_below(fast)?; // P(истинный наклон ниже быстрой границы)
    let p_slow = 1.0 - trend.p_slope_below(slow)?; // P(истинный наклон выше медленной)
    if p_fast >= CONFIDENT {
        Some(Pace::Fast)
    } else if p_slow >= CONFIDENT {
        Some(Pace::Slow)
    } else {
        Some(Pace::Comfortable)
    }
}

/// Комфортная полоса в кг/нед для этого веса — те самые границы, с которыми
/// сравнивается наклон, только положительные (сколько килограммов в неделю).
///
/// Публична ради письма: оно рассказывает человеку, где лежит его темп, и вторая
/// копия этих двух констант разошлась бы с первой. `None` — вес неизвестен.
pub fn comfort_band_kg_per_week(weight_kg: f64) -> Option<(f64, f64)> {
    if weight_kg <= 0.0 {
        return None;
    }
    Some((COMFORT_LOSS_MIN * weight_kg, COMFORT_LOSS_MAX * weight_kg))
}

/// Multiplier applied to the average intake, chosen from the weight trend +
/// current body weight. Pure (no I/O) so it is unit-tested. See the block comment.
pub fn planka_factor(trend: &crate::weight_trend::WeightTrend, weight_kg: f64) -> f64 {
    match pace(trend, weight_kg) {
        Some(Pace::Fast) => 1.0 + PLANKA_STEP, // худеет быстрее комфортного → поднимаем
        Some(Pace::Slow) => 1.0 - PLANKA_STEP, // медленнее (или стоит, или растёт) → срезаем
        Some(Pace::Comfortable) | None => 1.0,
    }
}

// ── Окно решения ─────────────────────────────────────────────────────────────
//
// Виджет веса показывает тренд за 14 дней — человек смотрит на «что происходит
// сейчас», и короткое окно там уместно. ПЛАНКА судится по 28 дням, и это разные
// величины, а не одна с разными настройками.
//
// Почему шире. Четырнадцатидневный наклон у живого человека гуляет от +0.3 до
// −1.2 кг/нед от недели к неделе — и это НЕ погрешность оценки, а настоящие
// многодневные качели воды в ±1.5 кг, которые прямая честно ловит. Критерий
// значимости от этого не спасает: четырнадцать дней действительно значимо
// направлены вверх одну неделю и вниз другую.
//
// Замер на живом ряде (61 день подряд, снижение 0.73 % массы в неделю — чуть
// быстрее комфортной полосы, то есть правильный ответ «один подъём и стоять»):
//
//   окно 14 дн. — 4 хода, 2 разворота
//   окно 21 дн. — 3 хода, 1 разворот
//   окно 28 дн. — 1 ход,  0 разворотов
//
// И решающее: сдвиг расписания пересчёта на ОДИН день (человек поставил первую
// планку во вторник, а не в понедельник) уводил итог четырнадцатидневного
// правила с 2400 на 2950 — 550 ккал разницы на одних и тех же взвешиваниях.
// Двадцативосьмидневное держится в пределах одного шага при любом расписании.
// Правило, ответ которого зависит от дня недели, — не правило.
//
// Окно — ПОТОЛОК ОБЗОРА, а не требование: нет 28 дней истории, считаем по тому,
// что есть. Отдельного режима прогрева не нужно — меньше точек даёт шире SE, а
// шире SE значит «держим». Заглядывать за день первой планки при этом можно и
// нужно: первая планка калибруется по среднему потреблению, значит дни до неё —
// тот же пищевой режим, просто неназванный. Обрезать окно по последней ПРАВКЕ
// планки пробовали — стало хуже: окно ужимается до восьми дней ровно в тот
// момент, когда нужна ясная голова, и правило начинает гоняться за водой.

/// Окно, по которому судится ПЛАНКА. Шире, чем окно виджета
/// ([`weight_trend::DEFAULT_WINDOW_DAYS`]) — см. блок выше.
pub const DECISION_WINDOW_DAYS: i64 = 28;

// ── На чём позволительно решать ──────────────────────────────────────────────
//
// Планку двигают только тогда, когда известны ОБЕ стороны: и что происходит с
// весом, и исполнялась ли планка. Это не новое правило, а доведение до конца
// того, ради чего заводился стопор по исполнению (см. `calorie_planka_weekly`):
// вес сам по себе о планке не говорит ничего.
//
// Раньше формулировка соблюдалась наполовину. Исполнение спрашивалось, но
// принималось любое — одного залогированного дня из семи хватало, чтобы объявить
// его известным. А вес не проверялся на актуальность вовсе: окно тренда
// закреплено за последним ЗАМЕРОМ, не за сегодня, поэтому у бросившего весы
// тренд не протухал, а ЗАМИРАЛ и продолжал выдаваться как текущий. Замер: человек
// перестал взвешиваться, дневник ведёт — планка ехала +5 % в неделю бесконечно
// (2500 → 2650 → … → 3750 за семь недель), а письмо каждую неделю сообщало
// «ваш вес уверенно снижается» про замеры полуторамесячной давности. Разгон
// останавливал стопор по исполнению — но случайно, он писался против другой петли.

/// Свежесть веса: замер старше этого — не данные.
pub const WEIGHT_FRESH_DAYS: i64 = 3;
/// Столько дней со взвешиваниями должно быть в окне решения. На трёх-четырёх
/// точках остаток случайно ложится в ноль, SE схлопывается, и критерий значимости
/// объявляет уверенность там, где её нет.
pub const WEIGHT_MIN_DAYS: usize = 4;
/// Перерыв во взвешиваниях, после которого считается, что человек бросал.
pub const WEIGHT_GAP_DAYS: i64 = 7;
/// Сколько ждать после возобновления: полную неделю, чтобы накопились замеры.
pub const RESUME_DAYS: i64 = 7;
/// Столько из семи завершённых дней должно быть с записями в дневнике.
pub const DIARY_MIN_DAYS: usize = 4;

/// Почему пересчёт не состоялся. Не ошибка — отсрочка: данных не хватает, и
/// появятся они сами.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NoDecision {
    /// Последнее взвешивание слишком давно.
    WeightStale { days: i64 },
    /// Взвешиваться начали снова после перерыва — ждём полную неделю.
    WeightJustResumed { days: i64 },
    /// Взвешиваний в окне слишком мало.
    WeightSparse { days: usize },
    /// Дневник вёлся слишком редко — исполнение планки неизвестно.
    DiarySparse { days: usize },
}

impl NoDecision {
    /// Причина ОДНОЙ строкой, без чисел. Журнал ошибок дедуплицирует записи по
    /// точному совпадению текста, и число внутри («прошло 9 дн.») делало запись
    /// новой каждый день — отсрочка плодила по строке на запуск.
    pub fn reason(self) -> &'static str {
        match self {
            NoDecision::WeightStale { .. } => {
                "пересчёт отложен: давно не было взвешиваний. Встаньте на весы — \
                 пересчёт случится сам"
            }
            NoDecision::WeightJustResumed { .. } => {
                "пересчёт отложен: взвешивания возобновились после перерыва, ждём \
                 полную неделю замеров"
            }
            NoDecision::WeightSparse { .. } => {
                "пересчёт отложен: взвешиваний за последний месяц слишком мало, \
                 чтобы судить о тренде"
            }
            NoDecision::DiarySparse { .. } => {
                "пересчёт отложен: дневник за последнюю неделю почти пуст — \
                 неизвестно, исполнялась ли планка"
            }
        }
    }
}

/// Хватает ли данных, чтобы двигать планку.
///
/// `today` приходит параметром: крейт чистый, часов у него нет. `diary_days` —
/// сколько из семи ЗАВЕРШЁННЫХ дней имеют записи в дневнике; `None` — дневник не
/// спрашивали (путь, где исполнение не при чём).
pub fn check_evidence(
    today: chrono::NaiveDate,
    weight: &[api_types::WeightEntry],
    diary_days: Option<usize>,
) -> Result<(), NoDecision> {
    if let Some(d) = diary_days {
        if d < DIARY_MIN_DAYS {
            return Err(NoDecision::DiarySparse { days: d });
        }
    }
    let mut wdays: Vec<chrono::NaiveDate> = weight
        .iter()
        .filter_map(|e| chrono::NaiveDate::parse_from_str(&e.date, "%Y-%m-%d").ok())
        .collect();
    wdays.sort_unstable();
    wdays.dedup();
    let Some(&latest) = wdays.last() else {
        return Err(NoDecision::WeightSparse { days: 0 });
    };

    let stale = (today - latest).num_days();
    if stale > WEIGHT_FRESH_DAYS {
        return Err(NoDecision::WeightStale { days: stale });
    }

    // Возобновление после перерыва: ищем ПОСЛЕДНИЙ разрыв длиннее `WEIGHT_GAP_DAYS`
    // и смотрим, сколько прожито после него. Дни до разрыва в счёт не идут — это
    // другая жизнь, и вес в ней о нынешней планке не говорит.
    if let Some(resumed) = wdays
        .windows(2)
        .filter(|w| (w[1] - w[0]).num_days() > WEIGHT_GAP_DAYS)
        .map(|w| w[1])
        .next_back()
    {
        let since = (today - resumed).num_days();
        if since < RESUME_DAYS {
            return Err(NoDecision::WeightJustResumed { days: since });
        }
    }

    // Плотность замеров в окне решения. Окно закреплено за последним замером —
    // так же, как его считает `daily_means`.
    let window_start = latest - chrono::Duration::days(DECISION_WINDOW_DAYS - 1);
    let in_window = wdays.iter().filter(|d| **d >= window_start).count();
    if in_window < WEIGHT_MIN_DAYS {
        return Err(NoDecision::WeightSparse { days: in_window });
    }
    Ok(())
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
        let trend = weight_trend::weight_trend(&w, DECISION_WINDOW_DAYS);
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

    /// Неизвестное исполнение ДЕРЖИТ планку. Раньше оно её освобождало — и это
    /// был самый опасный случай из всех: не зная, что человек ел, мы двигали
    /// планку по весу, который как раз и говорил о том, что человек ел.
    #[test]
    fn bez_dnevnika_planka_stoit() {
        let w: Vec<_> = (1..=28)
            .map(|i| weigh(&format!("2026-03-{i:02}"), 75.0 - i as f64 * 0.15))
            .collect();
        // Вес валится на 1.05 кг/нед — правило само по себе кричит «поднимай».
        let trend = weight_trend::weight_trend(&w, DECISION_WINDOW_DAYS);
        assert_eq!(pace(&trend, 70.0), Some(Pace::Fast));
        // Но без съеденного планка стоит.
        assert_eq!(suggest(&person(), 2000.0, &w, None).calories, 2000.0);
        // А с ним — двигается.
        assert!(suggest(&person(), 2000.0, &w, Some(2000.0)).calories > 2000.0);
    }

    // ── Гейт по данным ───────────────────────────────────────────────────────

    /// Ряд ежедневных взвешиваний за `n` дней, заканчивающийся `last`.
    fn daily(last: &str, n: i64) -> Vec<api_types::WeightEntry> {
        let last = chrono::NaiveDate::parse_from_str(last, "%Y-%m-%d").unwrap();
        (0..n)
            .map(|i| {
                let d = last - chrono::Duration::days(i);
                weigh(&d.format("%Y-%m-%d").to_string(), 80.0 + i as f64 * 0.1)
            })
            .collect()
    }

    fn day(s: &str) -> chrono::NaiveDate {
        chrono::NaiveDate::parse_from_str(s, "%Y-%m-%d").unwrap()
    }

    #[test]
    fn dannyh_hvataet_kogda_vse_na_meste() {
        let w = daily("2026-03-30", 28);
        assert_eq!(check_evidence(day("2026-03-30"), &w, Some(7)), Ok(()));
        // Замер вчерашний, дневник на четырёх днях — всё ещё считаем.
        assert_eq!(check_evidence(day("2026-03-31"), &w, Some(4)), Ok(()));
    }

    /// Протухший вес. Тот самый случай, где окно тренда ЗАМИРАЛО и продолжало
    /// выдаваться как текущее, а планка ехала на 5 % в неделю в никуда.
    #[test]
    fn protuhshij_ves_otkladyvaet_pereschet() {
        let w = daily("2026-03-30", 28);
        assert_eq!(check_evidence(day("2026-04-02"), &w, Some(7)), Ok(())); // 3 дня — ещё свежо
        assert_eq!(
            check_evidence(day("2026-04-03"), &w, Some(7)),
            Err(NoDecision::WeightStale { days: 4 })
        );
        // Через месяц молчания — та же причина, не «мало взвешиваний».
        assert!(matches!(
            check_evidence(day("2026-05-01"), &w, Some(7)),
            Err(NoDecision::WeightStale { .. })
        ));
    }

    /// Возобновил после перерыва — ждём полную неделю, чтобы накопились замеры.
    #[test]
    fn posle_pereryva_zhdem_nedelyu() {
        // Взвешивался в феврале, бросил, вернулся 2026-03-25.
        let mut w = daily("2026-02-20", 20);
        w.extend(daily("2026-03-28", 4));
        assert_eq!(
            check_evidence(day("2026-03-28"), &w, Some(7)),
            Err(NoDecision::WeightJustResumed { days: 3 })
        );
        // Ещё четыре дня взвешиваний — неделя с возобновления набралась.
        w.extend(daily("2026-04-01", 4));
        assert_eq!(check_evidence(day("2026-04-01"), &w, Some(7)), Ok(()));
    }

    /// Редкие взвешивания: на трёх-четырёх точках SE схлопывается и правило
    /// «уверенно» разворачивает планку на воде.
    #[test]
    fn redkih_vzveshivanij_ne_hvataet() {
        let w = vec![weigh("2026-03-28", 80.0), weigh("2026-03-30", 80.4)];
        assert_eq!(
            check_evidence(day("2026-03-30"), &w, Some(7)),
            Err(NoDecision::WeightSparse { days: 2 })
        );
        assert_eq!(
            check_evidence(day("2026-03-30"), &[], Some(7)),
            Err(NoDecision::WeightSparse { days: 0 })
        );
    }

    /// Пустой дневник. Одного залогированного дня из семи раньше хватало, чтобы
    /// объявить исполнение известным и двинуть планку.
    #[test]
    fn pustoj_dnevnik_otkladyvaet_pereschet() {
        let w = daily("2026-03-30", 28);
        assert_eq!(
            check_evidence(day("2026-03-30"), &w, Some(1)),
            Err(NoDecision::DiarySparse { days: 1 })
        );
        assert_eq!(
            check_evidence(day("2026-03-30"), &w, Some(3)),
            Err(NoDecision::DiarySparse { days: 3 })
        );
        // Дневник не спрашивали — не наше дело.
        assert_eq!(check_evidence(day("2026-03-30"), &w, None), Ok(()));
    }

    /// Одинокий старый замер не должен читаться как «только что вернулся»: разрыв
    /// закончился давно, и неделя с возобновления набрана с запасом. (Это ровно
    /// раскладка сеятеля из `scripts/check-calorie-planka-weekly.mjs`: строка
    /// старого формата тридцатидневной давности плюс три недели ежедневных.)
    #[test]
    fn davnij_odinokij_zamer_ne_meshaet() {
        let mut w = vec![weigh("2026-03-01", 93.0)];
        w.extend(daily("2026-03-31", 21));
        assert_eq!(check_evidence(day("2026-03-31"), &w, Some(7)), Ok(()));
    }

    /// Причина для журнала — без чисел: журнал дедуплицирует записи по тексту, и
    /// «прошло 9 дн.» внутри давало новую строку каждый день.
    #[test]
    fn prichina_dlya_zhurnala_bez_chisel() {
        for r in [
            NoDecision::WeightStale { days: 9 },
            NoDecision::WeightJustResumed { days: 2 },
            NoDecision::WeightSparse { days: 1 },
            NoDecision::DiarySparse { days: 0 },
        ] {
            assert!(!r.reason().chars().any(|c| c.is_ascii_digit()), "{}", r.reason());
        }
    }

    /// Ряд взвешиваний из живого случая, на котором планка задребезжала:
    /// 85–87 кг, ежедневные утренние взвешивания, ровное снижение.
    fn drebezg_series() -> Vec<api_types::WeightEntry> {
        [
            ("2026-08-16", 86.2), ("2026-08-17", 87.5), ("2026-08-18", 86.2),
            ("2026-08-19", 86.7), ("2026-08-20", 87.0), ("2026-08-21", 86.6),
            ("2026-08-22", 86.6), ("2026-08-23", 86.6), ("2026-08-24", 87.1),
            ("2026-08-25", 87.1), ("2026-08-26", 86.1), ("2026-08-27", 86.8),
            ("2026-08-28", 86.3), ("2026-08-29", 85.9), ("2026-08-30", 85.9),
            ("2026-08-31", 85.9), ("2026-09-01", 85.0), ("2026-09-02", 85.4),
            ("2026-09-03", 86.0), ("2026-09-04", 85.4), ("2026-09-05", 85.0),
        ]
        .iter()
        .map(|(d, kg)| weigh(d, *kg))
        .collect()
    }

    /// РЕГРЕССИЯ на живых данных. Прежнее правило сравнивало точечную оценку с
    /// границей полосы и на этом ряду дало −5 % 29 августа и +5 % 1 сентября —
    /// по окнам, совпадающим на 11 дней из 14. Планка пошла 2800 → 2950 → 2800.
    ///
    /// Теперь 29-е держит (погрешность накрывает полосу целиком), а 5-е поднимает
    /// (сигнал вырос до значимого). Ни одного разворота.
    #[test]
    fn drebezg_ne_povtoryaetsya() {
        let all = drebezg_series();
        let upto = |d: &str| -> Vec<_> {
            all.iter().filter(|e| e.date.as_str() <= d).cloned().collect()
        };

        // 29.08 — точечная оценка НИЖЕ полосы (то самое основание для среза).
        let t29 = weight_trend::weight_trend(&upto("2026-08-29"), DEFAULT_WINDOW_DAYS);
        let WeightTrend::Estimated { slope_kg_per_week: s29, .. } = t29 else {
            panic!("ожидали оценку, получили {t29:?}");
        };
        assert!(s29.abs() / 85.9 < 0.003, "темп {} — ожидали ниже полосы", s29.abs() / 85.9);
        // …но значимости нет, и планка стоит.
        assert_eq!(pace(&t29, 85.9), Some(Pace::Comfortable));
        assert_eq!(planka_factor(&t29, 85.9), 1.0);

        // 05.09 по окну ВИДЖЕТА снижение выглядит быстрым и значимым.
        let t5 = weight_trend::weight_trend(&upto("2026-09-05"), DEFAULT_WINDOW_DAYS);
        assert_eq!(pace(&t5, 85.0), Some(Pace::Fast));

        // А по окну РЕШЕНИЯ — нет: те же дни плюс предыдущая неделя дают −0.58
        // кг/нед (0.68 % — внутри полосы), и планка стоит. Это и есть правильный
        // ответ: за весь период человек снижался на 0.73 % в неделю, ровно.
        let d5 = weight_trend::weight_trend(&upto("2026-09-05"), DECISION_WINDOW_DAYS);
        assert_eq!(pace(&d5, 85.0), Some(Pace::Comfortable));
        assert_eq!(planka_factor(&d5, 85.0), 1.0);

        // Ни одного разворота на всём ряде — ни по одному окну.
        assert_eq!(planka_factor(&t29, 85.9), planka_factor(&d5, 85.0));
    }

    /// Без данных о съеденном планка стоит — см. `bez_dnevnika_planka_stoit`.
    #[test]
    fn bez_sedennogo_planka_ne_dvizhetsya() {
        let s = person();
        assert_eq!(suggest(&s, 2000.0, &[], None).calories, 2000.0);
    }
}

#[cfg(test)]
mod tests {
    use super::{
        adherence, calorie_planka, calorie_planka_weekly, next_steps_planka, pace, planka_factor,
        steps_planka_for_avg, Adherence, IndicatorState, Pace, PLANKA_STEP, STEPS_PLANKA_MAX,
    };
    use crate::weight_trend::{Direction, WeightTrend};

    /// Готовая оценка тренда. Погрешность задаётся ЯВНО: с появлением `pace` она
    /// такой же участник решения, как и сам наклон, и прятать её за «правдоподобным
    /// значением по умолчанию» значило бы не проверять главного.
    fn est(slope_wk: f64, se_wk: f64) -> WeightTrend {
        WeightTrend::Estimated {
            direction: if slope_wk < 0.0 { Direction::Down } else { Direction::Up },
            slope_kg_per_week: slope_wk,
            // Уверенность в ЗНАКЕ здесь уже никем не читается (её место занял
            // `p_slope_below`), но поле обязано остаться правдоподобным.
            confidence: 0.9,
            slope_se_kg_per_week: se_wk,
            days: 14,
        }
    }

    /// Погрешность, при которой решает наклон, а не шум: вдесятеро уже полосы.
    const TIGHT: f64 = 0.03;

    // ── Исполнение планки как стопор недельного пересчёта ────────────────────

    /// Уверенное быстрое похудение — тот случай, когда правило зовёт ПОДНЯТЬ планку.
    fn losing_fast() -> WeightTrend {
        est(-1.2, TIGHT) // при 80 кг это 1.5 %/нед — сильно выше комфортных 0.7
    }

    /// Вес стоит — правило зовёт ОПУСТИТЬ планку.
    fn flat() -> WeightTrend {
        est(0.05, TIGHT)
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

    /// Полоса — про ТЕМП, и при точной оценке правило то же, что и раньше:
    /// 90 кг → комфортные 0.27..0.63 кг/нед.
    #[test]
    fn tochnaya_ocenka_vedet_k_polose() {
        // В полосе (0.5 кг/нед) → держим.
        assert_eq!(planka_factor(&est(-0.5, TIGHT), 90.0), 1.0);
        // Слишком медленно (0.1 кг/нед) → мягко срезаем.
        assert_eq!(planka_factor(&est(-0.1, TIGHT), 90.0), 1.0 - PLANKA_STEP);
        // Слишком быстро (1.0 кг/нед) → приподнимаем.
        assert_eq!(planka_factor(&est(-1.0, TIGHT), 90.0), 1.0 + PLANKA_STEP);
        // Вес стоит и вес растёт — оба «значимо медленнее полосы», отдельных
        // случаев для них не нужно.
        assert_eq!(planka_factor(&est(0.0, TIGHT), 90.0), 1.0 - PLANKA_STEP);
        assert_eq!(planka_factor(&est(0.4, TIGHT), 90.0), 1.0 - PLANKA_STEP);
    }

    /// СУТЬ ФИКСА. Тот же наклон, что срезал бы планку при точной оценке, но
    /// погрешность соизмерима с полосой — значит про темп не известно ничего, и
    /// планка стоит. Именно на этом месте она дребезжала.
    #[test]
    fn shirokaya_pogreshnost_derzhit_planku() {
        // −0.21 кг/нед при 86 кг — на волосок ниже полосы (0.26..0.60).
        // При точной оценке это срез.
        assert_eq!(planka_factor(&est(-0.21, TIGHT), 86.0), 1.0 - PLANKA_STEP);
        // С реальной погрешностью бытовых весов (0.21 кг/нед) — держим.
        assert_eq!(planka_factor(&est(-0.21, 0.21), 86.0), 1.0);
        assert_eq!(pace(&est(-0.21, 0.21), 86.0), Some(Pace::Comfortable));
        // И зеркально: −0.75 кг/нед выше полосы, но погрешность 0.19 не даёт
        // назвать это «слишком быстро» — на следующей неделе как раз и вышел бы
        // обратный шаг.
        assert_eq!(planka_factor(&est(-0.75, 0.19), 85.0), 1.0);
        // А настоящий сигнал через ту же погрешность проходит.
        assert_eq!(planka_factor(&est(-0.99, 0.17), 85.0), 1.0 + PLANKA_STEP);
    }

    /// Шум вокруг нуля больше НЕ повод срезать планку: «неизвестно» — это не
    /// «стоит на месте». Настоящее плато при ежедневных взвешиваниях даёт узкую
    /// погрешность и срезается как прежде.
    #[test]
    fn shum_ne_prinimaetsya_za_plato() {
        assert_eq!(planka_factor(&est(-0.05, 0.4), 90.0), 1.0);
        assert_eq!(planka_factor(&est(-0.05, 0.1), 90.0), 1.0 - PLANKA_STEP);
    }

    /// Без оценки (меньше трёх дней взвешиваний) и без веса судить не по чему.
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
        assert_eq!(pace(&est(-1.0, TIGHT), 0.0), None);
        assert_eq!(planka_factor(&est(-1.0, TIGHT), 0.0), 1.0);
    }

    #[test]
    fn calorie_planka_rounds_to_50() {
        // Hold (темп в полосе) → avg unchanged, rounded to 50.
        let hold = est(-0.4, TIGHT);
        assert_eq!(calorie_planka(2600.0, &hold, 90.0), 2600.0);
        assert_eq!(calorie_planka(2490.0, &hold, 90.0), 2500.0); // 49.8 -> 50
        // Cut (plateau) → avg*0.95, rounded to 50.
        let cut = est(-0.05, TIGHT);
        assert_eq!(calorie_planka(2600.0, &cut, 90.0), 2450.0); // 2470 -> 49.4 -> 2450
        assert_eq!(calorie_planka(2000.0, &cut, 90.0), 1900.0);
    }
}
