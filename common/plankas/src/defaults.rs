//! Двенадцать планок и наши правила по умолчанию.
//!
//! До сих пор эти нормы лежали в семи разных модулях — каждая рядом со своим
//! индикатором. Пока правило было одно («так у всех»), это работало. Теперь
//! планку может задать куратор, и появился вопрос «а какая она без него» —
//! который надо задавать в одном месте, а не искать ответ по семи файлам.
//!
//! Здесь только НОРМЫ. Вычисления по данным человека (сколько железа усвоилось,
//! сколько порций гема набрано) остаются в своих модулях: они про еду, а не про
//! то, к чему стремиться.

use crate::{protein_target_g, Sex};

/// Вид планки. Единственный список на всё приложение — он же ключ в истории и в
/// директиве куратора.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Calories,
    Protein,
    Steps,
    VegFruit,
    Calcium,
    Fiber,
    Iron,
    Heme,
    EpaDha,
    FatRatio,
    RedMeat,
    Egg,
}

/// Все виды в порядке показа.
pub const ALL: &[Kind] = &[
    Kind::Calories, Kind::Protein, Kind::Steps, Kind::VegFruit, Kind::Calcium,
    Kind::Fiber, Kind::Iron, Kind::Heme, Kind::EpaDha, Kind::FatRatio,
    Kind::RedMeat, Kind::Egg,
];

impl Kind {
    pub fn key(self) -> &'static str {
        match self {
            Kind::Calories => "calories",
            Kind::Protein => "protein",
            Kind::Steps => "steps",
            Kind::VegFruit => "veg_fruit",
            Kind::Calcium => "calcium",
            Kind::Fiber => "fiber",
            Kind::Iron => "iron",
            Kind::Heme => "heme",
            Kind::EpaDha => "epa_dha",
            Kind::FatRatio => "fat_ratio",
            Kind::RedMeat => "red_meat",
            Kind::Egg => "egg",
        }
    }

    pub fn from_key(key: &str) -> Option<Kind> {
        ALL.iter().copied().find(|k| k.key() == key)
    }

    /// Ведёт ли эту планку САМО приложение. Динамические три двигает недельный
    /// цикл (калории и шаги) и калорийная планка (белок); остальные девять стоят
    /// на месте, пока их не тронет куратор.
    ///
    /// Отсюда следует и правило отвязки: динамические остаются кураторскими до
    /// первого пересчёта, у остальных запись просто стирается.
    pub fn is_dynamic(self) -> bool {
        matches!(self, Kind::Calories | Kind::Protein | Kind::Steps)
    }

    /// Сколько знаков после запятой осмысленно.
    pub fn decimals(self) -> usize {
        match self {
            Kind::FatRatio | Kind::EpaDha | Kind::Heme => 1,
            _ => 0,
        }
    }

    /// Разумные пределы правки. Не формальность: число приходит из чужого
    /// приложения, и лишний ноль не должен становиться планкой, от которой потом
    /// посчитается ещё и белок.
    ///
    /// Потолок калорий — 10 000. Прежние 20 000 предел ставили формально и своей
    /// работы не делали: любая жизненная планка (1200–4000) с лишним нулём даёт
    /// 12 000–40 000, и половина таких опечаток проходила бы насквозь. Больше
    /// десяти тысяч не ест никто, кому мы считаем похудение.
    pub fn range(self) -> (f64, f64) {
        match self {
            Kind::Calories => (500.0, 10_000.0),
            Kind::Protein => (10.0, 500.0),
            Kind::Steps => (1_000.0, 100_000.0),
            Kind::VegFruit => (100.0, 5_000.0),
            Kind::Calcium => (100.0, 5_000.0),
            Kind::Fiber => (5.0, 200.0),
            Kind::Iron => (0.1, 100.0),
            Kind::Heme => (0.0, 21.0),
            Kind::EpaDha => (0.1, 50.0),
            Kind::FatRatio => (0.1, 20.0),
            Kind::RedMeat => (0.0, 10_000.0),
            Kind::Egg => (0.0, 70.0),
        }
    }

    /// Проходит ли значение по пределам.
    pub fn accepts(self, amount: f64) -> bool {
        let (lo, hi) = self.range();
        amount.is_finite() && amount >= lo && amount <= hi
    }
}

/// Снимок человека, от которого зависят наши правила.
///
/// Собирается одинаково с обеих сторон: у худеющего — из профиля, у куратора — из
/// присланного отчёта. Ровно поэтому он и нужен: правило не должно знать, откуда
/// пришли эти пять чисел.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Snapshot {
    pub sex: Option<Sex>,
    pub age_years: Option<i32>,
    pub height_cm: Option<f64>,
    pub weight_kg: Option<f64>,
    /// Действующая планка по калориям — от неё считаются белок и клетчатка.
    pub kcal_planka: Option<f64>,
}

// ── Нормы, не зависящие от человека ──────────────────────────────────────────

/// Кальций: 1 г в сутки для всех.
pub const CALCIUM_PER_DAY_MG: f64 = 1000.0;
/// Гем: три порции в неделю.
pub const HEME_WEEKLY_PORTIONS: f64 = 3.0;
/// Порция гемового железа, выраженная в граммах белка.
pub const HEME_PORTION_PROTEIN_G: f64 = 25.0;
/// Длинные морские омега-3 за неделю, г.
pub const EPA_DHA_PER_WEEK_G: f64 = 1.75;
/// Минимальное отношение (МНЖК+ПНЖК)/НЖК.
pub const UNSAT_TO_SAT_MIN: f64 = 2.0;
/// Недельный предел красного мяса, г сырого.
pub const RED_MEAT_WEEKLY_LIMIT_RAW_G: f64 = 700.0;
/// Яйца за неделю, штук.
pub const EGG_WEEKLY_MIN: f64 = 7.0;

/// Овощи и фрукты, г/сут: мужчинам 800, остальным 600. Пол неизвестен → 600
/// (меньшее, чтобы норма не оказалась заведомо непосильной до заполнения профиля).
pub fn veg_fruit_per_day_g(sex: Option<Sex>) -> f64 {
    match sex {
        Some(Sex::Male) => 800.0,
        _ => 600.0,
    }
}

// ── Клетчатка ────────────────────────────────────────────────────────────────

/// Граммов клетчатки на 1000 ккал рациона — IOM AI, те же 14 г в действующих
/// Dietary Guidelines.
pub const G_PER_1000_KCAL: f64 = 14.0;

/// Нижняя граница суточной нормы, г. Минимум ВОЗ для взрослого: ниже не опускаемся,
/// какой бы скромной ни была калорийная планка.
pub const MIN_G_PER_DAY: f64 = 25.0;

/// Суточная норма от калорийной планки. Без планки — минимум ВОЗ: выдумывать
/// калорийность, чтобы посчитать от неё клетчатку, нечестно.
pub fn daily_target_g(planka_kcal: Option<f64>) -> f64 {
    let from_kcal = planka_kcal.unwrap_or(0.0) / 1000.0 * G_PER_1000_KCAL;
    from_kcal.max(MIN_G_PER_DAY)
}

// ── Железо ───────────────────────────────────────────────────────────────────

// ── The target ───────────────────────────────────────────────────────────────
// Source: Dietary Reference Intakes (Institute of Medicine), the table NIH ODS
// publishes — RDA in mg/day by life stage:
//
//   1–3 y   7  ·  4–8 y  10  ·  9–13 y  8   (both sexes)
//   14–18 y   male 11   female 15
//   19–50 y   male  8   female 18
//   51+ y     male  8   female  8
//
// The female 19–50 figure (18 mg) covers menstrual losses; it drops to the male
// level after menopause, which the 51-year boundary stands in for. We do not model
// pregnancy (27 mg) or lactation (9 mg) — the app has no such flag, and guessing
// would be worse than using the baseline.

/// Daily iron RDA in mg for this sex/age. Unknown sex → the higher (female) figure:
/// under-stating an intake target is the harmful direction.
pub fn rda_mg_per_day(sex: Option<Sex>, age_years: Option<i32>) -> f64 {
    // Age unknown → assume an adult.
    let age = age_years.unwrap_or(30);
    match age {
        0..=3 => 7.0,
        4..=8 => 10.0,
        9..=13 => 8.0,
        14..=18 => match sex {
            Some(Sex::Male) => 11.0,
            _ => 15.0,
        },
        19..=50 => match sex {
            Some(Sex::Male) => 8.0,
            _ => 18.0,
        },
        _ => 8.0,
    }
}

/// The bioavailability the DRI itself assumes for a mixed Western diet (~18 %).
/// The published RDA is stated in TOTAL milligrams eaten under that assumption, so
/// to express the same requirement in ABSORBED milligrams — the currency we can
/// actually measure per food — we scale it by this factor.
pub const RDA_BIOAVAILABILITY: f64 = 0.18;

/// Суточное потребление, из которого строится ПЛАНКА. Не всегда равно RDA.
///
/// RDA — это не средняя потребность. DRI берёт требование в усвоенном железе на
/// **97,5-м процентиле** (запас на самые обильные менструации) и делит на верхнюю
/// оценку усвоения. Для мужчин разброс потребности мал, и 97,5-й процентиль почти
/// совпадает со средним: RDA 8 против EAR 6. У женщин разброс огромный — RDA 18
/// против EAR 8,1, вдвое с лишним.
///
/// Планка по RDA означала бы 3,24 мг усвоенного в сутки. Живой рацион столько не
/// даёт: даже западный смешанный с мясом по нашей же таблице долей выходит около
/// 1,7 мг. Индикатор у женщин горел бы красным всегда и ничему не учил.
///
/// Поэтому у менструирующих женщин планка строится от EAR — СРЕДНЕЙ потребности.
/// Той, у кого потери выше средних, планку надо поднимать отдельно и осознанно
/// (планируется отметка «обильные менструации»), а не держать всех на верхнем крае.
///
/// Остальные возрастные группы — по RDA, как было: там разница между средним и
/// верхним краем невелика, и менять устоявшееся без нужды незачем.
pub fn intake_basis_mg_per_day(sex: Option<Sex>, age_years: Option<i32>) -> f64 {
    let age = age_years.unwrap_or(30);
    let menstruating = matches!(age, 19..=50) && !matches!(sex, Some(Sex::Male));
    if menstruating {
        // EAR по IOM для женщин 19–50: 8,1 мг/сут.
        8.1
    } else {
        rda_mg_per_day(sex, age_years)
    }
}

/// The weekly target in ABSORBED mg of iron: the daily intake basis for a week,
/// converted from "eaten" to "absorbed" by the bioavailability the DRI assumes.
pub fn weekly_absorbed_target_mg(sex: Option<Sex>, age_years: Option<i32>) -> f64 {
    intake_basis_mg_per_day(sex, age_years) * 7.0 * RDA_BIOAVAILABILITY
}

// ── Правило по умолчанию ─────────────────────────────────────────────────────

/// Наша норма для этого вида — то, что действует, пока куратор ничего не задал.
///
/// `None` означает «правила пока нет»: планка шагов до открытия темы, белок без
/// веса. Это не ноль и не провал — судить нечем, и притворяться, что есть чем,
/// было бы враньём.
pub fn default_for(kind: Kind, s: &Snapshot) -> Option<f64> {
    Some(match kind {
        // Калорийную и шаговую планку приложение НЕ выводит из профиля — их ведёт
        // недельный цикл, а до первой установки их попросту нет.
        Kind::Calories | Kind::Steps => return None,
        Kind::Protein => {
            let (w, h, age, sex) = (s.weight_kg?, s.height_cm?, s.age_years?, s.sex?);
            protein_target_g(s.kcal_planka, w, h, age, sex)? as f64
        }
        Kind::VegFruit => veg_fruit_per_day_g(s.sex),
        Kind::Calcium => CALCIUM_PER_DAY_MG,
        Kind::Fiber => daily_target_g(s.kcal_planka),
        Kind::Iron => weekly_absorbed_target_mg(s.sex, s.age_years),
        Kind::Heme => HEME_WEEKLY_PORTIONS,
        Kind::EpaDha => EPA_DHA_PER_WEEK_G,
        Kind::FatRatio => UNSAT_TO_SAT_MIN,
        Kind::RedMeat => RED_MEAT_WEEKLY_LIMIT_RAW_G,
        Kind::Egg => EGG_WEEKLY_MIN,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn adult() -> Snapshot {
        Snapshot {
            sex: Some(Sex::Female),
            age_years: Some(35),
            height_cm: Some(165.0),
            weight_kg: Some(70.0),
            kcal_planka: Some(1800.0),
        }
    }

    // ── Нормы, переехавшие сюда вместе со своими тестами ──────────────────
    // Тесты приехали БЕЗ ПРАВОК: неизменённый тест на переехавшей функции — это и
    // есть доказательство, что переезд ничего не поменял.

    #[test]
    fn rda_follows_the_dri_table() {
        // Children — same for both sexes.
        assert_eq!(rda_mg_per_day(Some(Sex::Male), Some(2)), 7.0);
        assert_eq!(rda_mg_per_day(Some(Sex::Female), Some(2)), 7.0);
        assert_eq!(rda_mg_per_day(Some(Sex::Male), Some(6)), 10.0);
        assert_eq!(rda_mg_per_day(Some(Sex::Female), Some(11)), 8.0);
        // Teens diverge: menstrual losses.
        assert_eq!(rda_mg_per_day(Some(Sex::Male), Some(16)), 11.0);
        assert_eq!(rda_mg_per_day(Some(Sex::Female), Some(16)), 15.0);
        // Adults.
        assert_eq!(rda_mg_per_day(Some(Sex::Male), Some(35)), 8.0);
        assert_eq!(rda_mg_per_day(Some(Sex::Female), Some(35)), 18.0);
        // After menopause the female figure drops to the male one.
        assert_eq!(rda_mg_per_day(Some(Sex::Female), Some(51)), 8.0);
        assert_eq!(rda_mg_per_day(Some(Sex::Female), Some(70)), 8.0);
        // Boundaries.
        assert_eq!(rda_mg_per_day(Some(Sex::Female), Some(50)), 18.0);
        assert_eq!(rda_mg_per_day(Some(Sex::Male), Some(18)), 11.0);
        assert_eq!(rda_mg_per_day(Some(Sex::Male), Some(19)), 8.0);
        // Unknown sex → the higher figure (under-stating the target is the harmful way).
        assert_eq!(rda_mg_per_day(None, Some(35)), 18.0);
        // Unknown age → treated as an adult.
        assert_eq!(rda_mg_per_day(Some(Sex::Male), None), 8.0);
    }

    #[test]
    fn weekly_target_is_the_intake_basis_in_absorbed_terms() {
        // Женщина 35: планка от СРЕДНЕЙ потребности (EAR 8,1), а не от RDA 18.
        // 8,1 × 7 × 0,18 ≈ 10,206 мг усвоенного в неделю.
        let t = weekly_absorbed_target_mg(Some(Sex::Female), Some(35));
        assert!((t - 10.206).abs() < 1e-9, "{t}");
        // По RDA было бы 22,68 — недостижимо на живой еде.
        assert!(t < 18.0 * 7.0 * RDA_BIOAVAILABILITY);
        // Man 35: 8 × 7 × 0.18 ≈ 10.08 — по RDA, как было.
        let t = weekly_absorbed_target_mg(Some(Sex::Male), Some(35));
        assert!((t - 10.08).abs() < 1e-9, "{t}");
    }

    #[test]
    fn snizhenie_kasaetsya_tolko_menstruiruyushchih() {
        // Подросток 16 и женщина после менопаузы — по-прежнему по RDA.
        assert_eq!(
            weekly_absorbed_target_mg(Some(Sex::Female), Some(16)),
            15.0 * 7.0 * RDA_BIOAVAILABILITY
        );
        assert_eq!(
            weekly_absorbed_target_mg(Some(Sex::Female), Some(60)),
            8.0 * 7.0 * RDA_BIOAVAILABILITY
        );
        // Пол неизвестен в 19–50 — считаем как женщину: занижать вреднее.
        assert_eq!(
            weekly_absorbed_target_mg(None, Some(35)),
            8.1 * 7.0 * RDA_BIOAVAILABILITY
        );
    }

    #[test]
    fn norma_rastyot_vmeste_s_planko() {
        // 2600 ккал → 36.4 г/сут.
        assert!((daily_target_g(Some(2600.0)) - 36.4).abs() < 1e-9);
        // 3500 ккал → 49 г/сут.
        assert!((daily_target_g(Some(3500.0)) - 49.0).abs() < 1e-9);
    }

    #[test]
    fn nizhe_minimuma_vo_z_ne_opuskaemsya() {
        // 1500 ккал дали бы 21 г — но ВОЗ говорит не меньше 25.
        assert!((daily_target_g(Some(1500.0)) - MIN_G_PER_DAY).abs() < 1e-9);
        // Планки ещё нет — тоже минимум, а не ноль.
        assert!((daily_target_g(None) - MIN_G_PER_DAY).abs() < 1e-9);
    }

    #[test]
    fn nedelnaya_planka_eto_sem_sutochnyh() {
        assert!((daily_target_g(Some(2600.0)) * 7.0 - 254.8).abs() < 1e-9);
    }

    /// Ключи и виды обязаны отображаться друг в друга без потерь: ключ едет в
    /// историю и в директиву, и разъехавшись, они молча потеряли бы планку.
    #[test]
    fn klyuchi_i_vidy_vzaimno_odnoznachny() {
        for k in ALL {
            assert_eq!(Kind::from_key(k.key()), Some(*k), "ключ {} не вернулся", k.key());
        }
        assert_eq!(ALL.len(), 12);
        assert!(Kind::from_key("processed_meat").is_none(), "у колбас планки нет");
        assert!(Kind::from_key("чушь").is_none());
    }

    /// У каждого вида есть правило по умолчанию — кроме двух, которые ведёт
    /// недельный цикл и которых до первой установки не существует.
    #[test]
    fn pravilo_est_u_vseh_krome_vedomyh_ciklom() {
        let s = adult();
        for k in ALL {
            let v = default_for(*k, &s);
            if matches!(k, Kind::Calories | Kind::Steps) {
                assert!(v.is_none(), "{} не выводится из профиля", k.key());
            } else {
                assert!(v.is_some(), "у {} нет правила по умолчанию", k.key());
            }
        }
    }

    /// Неполный профиль не выдумывает норму белка.
    #[test]
    fn belok_bez_profilya_ne_pridumyvaetsya() {
        let s = Snapshot { weight_kg: None, ..adult() };
        assert!(default_for(Kind::Protein, &s).is_none());
    }

    /// Клетчатка идёт за калорийной планкой, а без неё держит минимум ВОЗ.
    #[test]
    fn kletchatka_sleduet_za_kaloriyami() {
        let low = Snapshot { kcal_planka: Some(1500.0), ..adult() };
        let high = Snapshot { kcal_planka: Some(3500.0), ..adult() };
        assert_eq!(default_for(Kind::Fiber, &low), Some(MIN_G_PER_DAY));
        assert!(default_for(Kind::Fiber, &high).unwrap() > MIN_G_PER_DAY);
        let none = Snapshot { kcal_planka: None, ..adult() };
        assert_eq!(default_for(Kind::Fiber, &none), Some(MIN_G_PER_DAY));
    }

    /// Пол и возраст меняют норму железа и овощей — на то они и в снимке.
    #[test]
    fn pol_i_vozrast_menyayut_normy() {
        let male = Snapshot { sex: Some(Sex::Male), ..adult() };
        assert!(default_for(Kind::VegFruit, &male) > default_for(Kind::VegFruit, &adult()));
        // Менструирующая женщина считается от EAR, мужчина — от RDA.
        assert_ne!(default_for(Kind::Iron, &male), default_for(Kind::Iron, &adult()));
    }

    /// Пределы ловят опечатку, но пропускают жизненные значения.
    #[test]
    fn predely_lovyat_opechatku() {
        assert!(Kind::Calories.accepts(1800.0));
        assert!(!Kind::Calories.accepts(18_000.0));
        assert!(!Kind::Calories.accepts(50.0));
        assert!(!Kind::Calories.accepts(f64::NAN));
        for k in ALL {
            let (lo, hi) = k.range();
            assert!(lo < hi, "{}: пустой диапазон", k.key());
        }
    }

    /// Правило отвязки: у КАЖДОГО постоянного вида есть, к чему возвращаться.
    ///
    /// Отвязка стирает записи девяти постоянных видов — и на этом останавливается,
    /// ничего не записывая. Это работает только потому, что для каждого из них
    /// правило по умолчанию существует ВСЕГДА: иначе человек остался бы без
    /// планки вовсе, а индикатор — без нормы, по которой судить.
    ///
    /// Неполный профиль ничего не меняет: девять норм от него не зависят.
    #[test]
    fn otvyazke_est_kuda_vernutsya() {
        let bare = Snapshot::default();
        for k in ALL.iter().filter(|k| !k.is_dynamic()) {
            assert!(
                default_for(*k, &bare).is_some(),
                "{}: стереть запись было бы некуда — правила нет",
                k.key()
            );
        }
        // Девять — ровно столько и должно остаться после трёх динамических.
        assert_eq!(ALL.iter().filter(|k| !k.is_dynamic()).count(), 9);
    }

    /// Динамических ровно три — те, что ведёт приложение.
    #[test]
    fn dinamicheskih_rovno_tri() {
        let dyn_count = ALL.iter().filter(|k| k.is_dynamic()).count();
        assert_eq!(dyn_count, 3);
        assert!(Kind::Calories.is_dynamic() && Kind::Steps.is_dynamic() && Kind::Protein.is_dynamic());
        assert!(!Kind::Calcium.is_dynamic());
    }
}
