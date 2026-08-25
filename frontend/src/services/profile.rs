//! The user profile (biological sex, height, birth year), kept as a SYNCED
//! keyed-singleton row in the `profile` IndexedDB store (one row, key
//! "profile"), merged last-writer-wins by `updated_at` across devices — exactly
//! like the `story` flags.
//!
//! Reads stay SYNCHRONOUS via an in-memory cache (so the existing sync callers
//! in story/weight-modal/settings don't have to await). The cache is hydrated by
//! [`hydrate`] after every active-database switch (launch, login, pairing) —
//! before any reader runs. Writes read-modify-write the cache row, stamp
//! `updated_at`, persist to IndexedDB, and push to the server in the background.

use std::cell::RefCell;

use api_types::ProfileRow;

use crate::services::{db, sync};

/// The singleton row key.
const PROFILE_KEY: &str = "profile";

/// Legacy device-global localStorage keys, migrated once into the synced row.

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Sex {
    Male,
    Female,
}

/// The overall goal of the course. Defaults to weight loss; the user can switch
/// to maintenance only after the relevant chapter unlocks.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum CourseGoal {
    Lose,
    Gain,
    Maintain,
}

thread_local! {
    static CACHE: RefCell<Option<ProfileRow>> = const { RefCell::new(None) };
    /// Bumped whenever the cached row CHANGES (a sync brought another device's
    /// profile), so UI reading the synchronous getters re-renders.
    static VERSION: RefCell<Option<leptos::RwSignal<u32>>> = const { RefCell::new(None) };
}

/// Create the root reactivity signal. Call once from `main()` BEFORE the first
/// [`hydrate`] (which runs inside `db::init`).
pub fn init() {
    VERSION.with(|c| *c.borrow_mut() = Some(leptos::create_rw_signal(0u32)));
}

/// The signal UI subscribes to for profile changes (the getters below are
/// synchronous cache reads and are NOT reactive on their own).
pub fn version_signal() -> leptos::RwSignal<u32> {
    VERSION.with(|c| c.borrow().expect("profile::init() must run first"))
}

fn local_storage() -> Option<web_sys::Storage> {
    web_sys::window().and_then(|w| w.local_storage().ok().flatten())
}

/// Load the profile row from IndexedDB into the synchronous in-memory cache.
/// Called after the active database is switched (launch / login / pairing).
pub async fn hydrate() {
    let row = db::get::<ProfileRow>("profile", PROFILE_KEY).await;
    let changed = CACHE.with(|c| {
        let mut cur = c.borrow_mut();
        if *cur == row {
            return false;
        }
        *cur = row;
        true
    });
    // A profile arriving by SYNC must show at once: the persona screen and every
    // profile-derived widget read the cache synchronously, so without this bump
    // they keep the pre-sync state until the next launch.
    if changed {
        if let Some(sig) = VERSION.with(|c| *c.borrow()) {
            leptos::SignalUpdate::update(&sig, |v| *v += 1);
        }
    }
}

/// Read the cached row (a clone), or a fresh empty row keyed "profile".
fn row_or_default() -> ProfileRow {
    CACHE.with(|c| c.borrow().clone()).unwrap_or(ProfileRow {
        key: PROFILE_KEY.to_string(),
        sex: None,
        height_cm: None,
        birth_year: None,
        goal: None,
        cycle_start: None,
        steps_planka: None,
        updated_at: String::new(),
    })
}

/// Read-modify-write: apply `mutate` to the current row, stamp `updated_at`,
/// update the cache, persist to IndexedDB, and push in the background.
fn write(mutate: impl FnOnce(&mut ProfileRow)) {
    let mut row = row_or_default();
    mutate(&mut row);
    row.updated_at = chrono::Utc::now().to_rfc3339();
    CACHE.with(|c| *c.borrow_mut() = Some(row.clone()));
    leptos::spawn_local(async move {
        db::put("profile", &row).await;
        sync::push_background();
    });
}

pub fn get_sex() -> Option<Sex> {
    CACHE.with(|c| {
        c.borrow().as_ref().and_then(|r| match r.sex.as_deref() {
            Some("male") => Some(Sex::Male),
            Some("female") => Some(Sex::Female),
            _ => None,
        })
    })
}

pub fn set_sex(sex: Sex) {
    let v = match sex {
        Sex::Male => "male",
        Sex::Female => "female",
    };
    write(|r| r.sex = Some(v.to_string()));
}

/// The daily steps planka (activity target), if set. System-set on the activity
/// week — there is no manual editing UI. Lives here (not in `goals`) so no
/// nutrient-iterating food UI can ever pick it up.
pub fn get_steps_planka() -> Option<f64> {
    crate::services::curator_plankas::or_ours_opt("steps", our_steps_planka())
}

/// Планка шагов, которую поставило САМО приложение, — без кураторского
/// приоритета (см. [`crate::services::local::our_calorie_goal_amount`]).
pub fn our_steps_planka() -> Option<f64> {
    CACHE.with(|c| c.borrow().as_ref().and_then(|r| r.steps_planka).filter(|p| *p > 0.0))
}

/// Store the steps planka. A non-positive value clears it.
pub fn set_steps_planka(planka: f64) {
    write(|r| r.steps_planka = if planka > 0.0 { Some(planka) } else { None });
    // Установка попадает в ИСТОРИЮ: индикатор судит день по планке, действовавшей
    // именно в тот день. Профиль хранит только текущее значение, и по нему прошлое
    // не восстановить.
    if planka > 0.0 {
        leptos::spawn_local(async move {
            crate::services::local::record_planka(crate::services::local::PLANKA_STEPS, planka)
                .await;
        });
    }
}

/// The user's height in centimetres, if set (and a positive number).
pub fn get_height_cm() -> Option<f64> {
    CACHE.with(|c| c.borrow().as_ref().and_then(|r| r.height_cm).filter(|h| *h > 0.0))
}

/// Store the height (cm). A non-positive value clears it.
pub fn set_height_cm(cm: f64) {
    write(|r| r.height_cm = if cm > 0.0 { Some(cm) } else { None });
}

/// The user's year of birth, if set and within a sane range.
pub fn get_birth_year() -> Option<i32> {
    let current_year = chrono::Utc::now().format("%Y").to_string().parse::<i32>().unwrap_or(2026);
    CACHE.with(|c| {
        c.borrow()
            .as_ref()
            .and_then(|r| r.birth_year)
            .filter(|y| (1900..=current_year).contains(y))
    })
}

/// Store the year of birth. A value of 0 (or out of range) clears it.
pub fn set_birth_year(year: i32) {
    let current_year = chrono::Utc::now().format("%Y").to_string().parse::<i32>().unwrap_or(2026);
    write(|r| r.birth_year = if (1900..=current_year).contains(&year) { Some(year) } else { None });
}

/// The course goal. Defaults to `Lose` when unset.
pub fn get_goal() -> CourseGoal {
    CACHE.with(|c| {
        match c.borrow().as_ref().and_then(|r| r.goal.as_deref()) {
            Some("maintain") => CourseGoal::Maintain,
            Some("gain") => CourseGoal::Gain,
            _ => CourseGoal::Lose,
        }
    })
}

/// Store the course goal. A CHANGE flags the calorie planka as needing a recompute
/// (the old planka was derived for the previous goal).
pub fn set_goal(goal: CourseGoal) {
    let changed = get_goal() != goal;
    let v = match goal {
        CourseGoal::Lose => "lose",
        CourseGoal::Gain => "gain",
        CourseGoal::Maintain => "maintain",
    };
    write(|r| r.goal = Some(v.to_string()));
    if changed {
        crate::services::local::set_planka_stale(true);
    }
}

/// First day of the current menstrual cycle (YYYY-MM-DD), if set.
pub fn get_cycle_start() -> Option<String> {
    CACHE.with(|c| c.borrow().as_ref().and_then(|r| r.cycle_start.clone()))
}

/// Store the first day of the cycle (YYYY-MM-DD). An empty string clears it.
pub fn set_cycle_start(date: &str) {
    let v = if date.is_empty() { None } else { Some(date.to_string()) };
    write(|r| r.cycle_start = v);
}

/// Body Mass Index = weight(kg) / height(m)². `None` if height is not a positive
/// value. Used as a coarse read on how much of the body mass is fat.
pub fn bmi(weight_kg: f64, height_cm: f64) -> Option<f64> {
    if height_cm <= 0.0 {
        return None;
    }
    let m = height_cm / 100.0;
    Some(weight_kg / (m * m))
}

/// Current age in whole years from the stored birth year (approximate — no
/// birthday tracking). `None` if the birth year is unset/out of range.
pub fn get_age_years() -> Option<i32> {
    let current_year = chrono::Utc::now().format("%Y").to_string().parse::<i32>().unwrap_or(2026);
    get_birth_year().map(|by| current_year - by)
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

/// Convenience over [`protein_target_g`]: pulls height/age/sex from the profile
/// and the current calorie planka, and computes the target for `weight_kg`.
/// Returns 0 when any profile field is unset (the setup section captures them
/// before protein ever matters), so the task simply shows «0 г» until the profile
/// is complete — the same fallback the weight-only formula used.
pub async fn protein_target_from_profile(weight_kg: f64) -> u32 {
    // Кураторское число поверх нашей кривой: белок выводится из калорийной
    // планки, но куратор вправе назвать своё.
    if let Some(g) = crate::services::curator_plankas::get("protein") {
        return g.max(0.0).round() as u32;
    }
    let kcal = crate::services::local::calorie_goal_amount().await;
    match (get_height_cm(), get_age_years(), get_sex()) {
        (Some(h), Some(age), Some(sex)) => {
            protein_target_g(kcal, weight_kg, h, age, sex).unwrap_or(0)
        }
        _ => 0,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn deurenberg_worked_example_man() {
        // Man, 35 y, 180 cm, 90 kg → BMI 27.8, BF% ≈ 25%, FFM ≈ 67.3 kg.
        // Без калорийной планки берётся пол: 1.6 г/кг FFM ≈ 108 г.
        let g = protein_target_g(None, 90.0, 180.0, 35, Sex::Male).unwrap();
        assert_eq!(g, 108);
    }

    #[test]
    fn deurenberg_woman_higher_bf_lower_target() {
        // Same anthropometrics but female (sex term 0) → higher BF%, less FFM,
        // so a lower protein target than the man.
        let woman = protein_target_g(None, 90.0, 180.0, 35, Sex::Female).unwrap();
        let man = protein_target_g(None, 90.0, 180.0, 35, Sex::Male).unwrap();
        assert!(woman < man, "woman {woman} should be < man {man}");
    }

    #[test]
    fn protein_target_needs_positive_weight_and_height() {
        assert!(protein_target_g(None, 0.0, 180.0, 35, Sex::Male).is_none());
        assert!(protein_target_g(None, 90.0, 0.0, 35, Sex::Male).is_none());
    }

    #[test]
    fn protein_is_thirty_percent_of_the_calorie_planka() {
        // Женщина 42 г., 180 см, 64.5 кг при планке 1800 ккал — это ровно точка
        // перегиба: 30 % от неё, 135 г. ВЫШЕ пола (74 г) и НИЖЕ потолка
        // (2.2·64.5 ≈ 142 г), так что берётся сама кривая.
        let g = protein_target_g(Some(1800.0), 64.5, 180.0, 42, Sex::Female).unwrap();
        assert_eq!(g, 135);
    }

    #[test]
    fn low_calorie_planka_cannot_push_protein_below_the_ffm_floor() {
        // Экстремально низкая планка: кривая даёт от 900 ккал 68 г, ниже пола.
        let target = protein_target_g(Some(900.0), 64.5, 180.0, 42, Sex::Female).unwrap();
        let floor = protein_target_g(None, 64.5, 180.0, 42, Sex::Female).unwrap();
        assert_eq!(target, floor);
        assert!(protein_from_kcal(900.0) < floor as f64, "проверка должна упираться в пол");
    }

    #[test]
    fn high_calorie_planka_cannot_push_protein_above_the_bodyweight_ceiling() {
        // Крупная планка у некрупного человека: кривая даёт от 3000 ккал 167 г,
        // выше потолка 2.2 г на кг полного веса.
        let target = protein_target_g(Some(3000.0), 64.5, 180.0, 42, Sex::Female).unwrap();
        assert_eq!(target, (2.2 * 64.5_f64).round() as u32);
    }

    #[test]
    fn floor_never_exceeds_ceiling() {
        // Зажим корректен при любом составе тела: 1.6·FFM ≤ 1.6·вес < 2.2·вес.
        for &(w, h, age, sex) in &[
            (45.0, 175.0, 20, Sex::Male),
            (64.5, 180.0, 42, Sex::Female),
            (95.0, 165.0, 35, Sex::Female),
            (120.0, 178.0, 40, Sex::Male),
            (200.0, 170.0, 60, Sex::Female),
        ] {
            let ffm = fat_free_mass_kg(w, h, age, sex).unwrap();
            assert!(
                PROTEIN_MIN_PER_KG_FFM * ffm <= PROTEIN_MAX_PER_KG_BW * w,
                "пол выше потолка при {w} кг / {h} см"
            );
        }
    }

    /// Шаг обхода диапазона: монотонность проверяется сплошь, а не по контрольным
    /// точкам — на них немонотонность как раз и не видна.
    const STEP_KCAL: f64 = 10.0;

    #[test]
    fn grammy_ne_ubyvayut_na_vsyom_diapazone() {
        let mut kcal = 1200.0;
        let mut prev = protein_from_kcal(kcal);
        while kcal <= 4000.0 {
            let g = protein_from_kcal(kcal);
            assert!(g >= prev, "белок упал при {kcal} ккал: {prev} → {g}");
            prev = g;
            kcal += STEP_KCAL;
        }
    }

    #[test]
    fn dolya_ne_vozrastaet_na_vsyom_diapazone() {
        let mut kcal = 1200.0;
        let mut prev = protein_share_pct(kcal);
        while kcal <= 4000.0 {
            let p = protein_share_pct(kcal);
            // Плато до точки перегиба — это тоже «не возрастает»; допуск покрывает
            // накопленную ошибку f64 на плоском участке.
            assert!(p <= prev + 1e-9, "доля выросла при {kcal} ккал: {prev} → {p}");
            prev = p;
            kcal += STEP_KCAL;
        }
    }

    #[test]
    fn kontrolnye_tochki_krivoy() {
        for &(kcal, want_g, want_pct) in &[
            (1600.0, 120.0, 30.0),
            (1800.0, 135.0, 30.0),
            (2000.0, 141.0, 28.2),
            (2400.0, 152.0, 25.4),
            (2800.0, 162.0, 23.2),
            (3600.0, 180.0, 20.0),
        ] {
            let g = protein_from_kcal(kcal);
            assert!((g - want_g).abs() <= 1.0, "{kcal} ккал: белок {g}, ждали {want_g}");
            let pct = protein_share_pct(kcal);
            assert!((pct - want_pct).abs() <= 0.1, "{kcal} ккал: доля {pct}, ждали {want_pct}");
        }
    }

    #[test]
    fn vetvi_shodyatsya_v_tochke_peregiba() {
        // Ниже перегиба — прямая, выше — степень; в самой точке обе дают P0.
        let below = PROTEIN_ANCHOR_G * PROTEIN_ANCHOR_KCAL / PROTEIN_ANCHOR_KCAL;
        let above = PROTEIN_ANCHOR_G
            * (PROTEIN_ANCHOR_KCAL / PROTEIN_ANCHOR_KCAL).powf(1.0 + PROTEIN_CURVE_K);
        assert!((below - PROTEIN_ANCHOR_G).abs() < 1e-9);
        assert!((above - PROTEIN_ANCHOR_G).abs() < 1e-9);
        assert!((protein_from_kcal(PROTEIN_ANCHOR_KCAL) - PROTEIN_ANCHOR_G).abs() < 1e-9);
        // И подходя к точке с обеих сторон вплотную — без скачка.
        let eps = 1e-6;
        let l = protein_from_kcal(PROTEIN_ANCHOR_KCAL - eps);
        let r = protein_from_kcal(PROTEIN_ANCHOR_KCAL + eps);
        assert!((l - r).abs() < 1e-6, "разрыв в точке перегиба: {l} против {r}");
    }

    #[test]
    fn pokazatel_v_dopustimom_diapazone() {
        // При k = 0 доля постоянна, при k = −1 граммы перестают расти, при k < −1
        // они бы убывали. Кривая осмысленна строго внутри.
        assert!(
            (-1.0..0.0).contains(&PROTEIN_CURVE_K),
            "k = {PROTEIN_CURVE_K} вне [−1, 0)"
        );
    }

    #[test]
    fn pokazatel_vyveden_iz_yakorey() {
        // k = ln(p1/p0) / ln(E1/E0) для якорей 30 % при 1800 и 20 % при 3600.
        let k = (0.20_f64 / 0.30).ln() / (3600.0_f64 / PROTEIN_ANCHOR_KCAL).ln();
        assert!((k - PROTEIN_CURVE_K).abs() < 5e-5, "из якорей выходит {k}");
    }
}
