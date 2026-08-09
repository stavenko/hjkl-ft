//! The Dashboard — the app's default screen and first nav tab.
//!
//! Layout: an 8-COLUMN square-cell grid. A unit is 1×1 cell; widgets occupy a
//! rectangle of units (the weight/steps widgets will be 4×3). Widgets are revealed
//! progressively.
//!
//! The Persona widget comes FIRST and is OPEN by default: while the profile is
//! incomplete its editor fills the whole dashboard (above the nav, in-flow — never a
//! bottom sheet that fights the menu), and the other widgets (the notifications bell)
//! are hidden behind it. Once every field is filled it collapses to a 1×1 tile and
//! the bell appears and jiggles; tapping the tile re-opens the full-screen editor.
//!
//! This first increment ships the framework + the Persona and Notifications widgets.
//! Weight/steps tiles (4×3) will slot into the same grid next.

use leptos::*;

use crate::components::cycle_widget::{CycleLine, CyclePanel};
use crate::components::notify_panel::NotifyPanel;
use crate::components::day_bars::DayBars;
use crate::components::gauge::Gauge;
use crate::components::info_hint::InfoHint;
use crate::components::progress_widget::{self, ProgressWidget};
use crate::components::steps_panel::StepsPanel;
use crate::components::steps_widget::StepsWidget;
use crate::components::weight_panel::WeightPanel;
use crate::components::weight_widget::WeightWidget;
use std::cell::RefCell;

use api_types::{StepEntry, WeightEntry};

use crate::services::errors::AppError;
use crate::services::i18n::t;
use crate::services::profile::{self, CourseGoal, Sex};
use crate::services::indicators::{self, IndicatorState};
use crate::services::sticky::sticky;
use crate::services::weight_trend;
use crate::services::{app_flags, db, local, net};

/// Персона подтверждена кнопкой «Готово». Синкается: заполнив её на одном
/// устройстве, человек не должен отвечать на те же вопросы на втором.
const PERSONA_CONFIRMED: &str = "persona_confirmed";

// ── Expanded-view helpers (gauge labels / colours + "?" explanations) ─────────

/// Everything the expanded view needs once the planka is set.
#[derive(Clone)]
struct DetailData {
    planka: Option<f64>,
    eaten: f64,
    gauges: Vec<indicators::DailyGauge>,
    /// Недельное железо — в развёрнутом виде оно должно быть ровно так же, как в
    /// свёрнутом виджете. `None`, пока неделя железа не открыта.
    iron: Option<crate::services::iron::WeeklyIron>,
    /// Порции гемовых продуктов за текущую неделю. Неделя та же, что у железа.
    heme: Option<crate::services::heme::WeeklyHeme>,
    fats: Option<crate::services::fats::WeeklyFats>,
    series: Vec<indicators::IndicatorSeries>,
    calorie_hint: String,
    protein_hint: String,
    veg_hint: String,
    /// Пояснение про сам показатель, по одному на индикатор. Собирается здесь, а не
    /// в разметке: часть текстов асинхронна (норма белка читает вес, планка калорий
    /// — цель), а разметка синхронна.
    bodies: std::collections::HashMap<&'static str, String>,
}

fn gauge_label(key: &str) -> &'static str {
    match key {
        "protein" => "Белок",
        "veg_fruit" => "Фр/овощи",
        "calcium" => "Кальций",
        _ => "Клетчатка",
    }
}

/// The calorie "?" text — depends on the course goal AND the current weight trend,
/// and shows the actual average intake + resulting planka.
async fn calorie_hint_text(planka: Option<f64>) -> String {
    let avg = local::avg_daily_kcal(7).await;
    let entries = local::list_weight_entries().await;
    let trend = weight_trend::weight_trend(&entries, weight_trend::DEFAULT_WINDOW_DAYS);
    let weight_kg = entries
        .iter()
        .max_by(|a, b| a.date.cmp(&b.date))
        .map(|e| e.weight_kg)
        .unwrap_or(0.0);
    let goal = match profile::get_goal() {
        CourseGoal::Lose => "похудение",
        CourseGoal::Maintain => "удержание веса",
        CourseGoal::Gain => "набор веса",
    };
    // Describe the ACTUAL adjustment via the single-source `planka_factor` (no drift).
    let f = local::planka_factor(&trend, weight_kg);
    let why = if f > 1.001 {
        "а вес снижается слишком быстро — мы приподняли планку на 5% для комфорта"
    } else if f < 0.999 {
        "а вес пока не снижается уверенно — мы снизили планку на 5%, чтобы создать мягкий дефицит"
    } else {
        "а вес снижается комфортно (или мы ещё уточняем данные за неделю) — мы держим планку на вашем среднем"
    };
    let avg_s = avg.map(|a| format!("{a:.0}")).unwrap_or_else(|| "—".to_string());
    let planka_s = planka.map(|p| format!("{p:.0}")).unwrap_or_else(|| "—".to_string());
    format!(
        "Поскольку ваша цель — {goal}, {why}.\n\nВаше среднее потребление за 7 дней: {avg_s} ккал.\n\
         И поэтому ваша планка: {planka_s} ккал."
    )
}

/// The protein "?" text — the fat-free-mass derivation, with the user's numbers.
async fn protein_hint_text() -> String {
    let Some(w) = local::list_weight_entries().await.last().map(|e| e.weight_kg) else {
        return "Заполните вес и профиль, чтобы рассчитать норму белка.".to_string();
    };
    let target = profile::protein_target_from_profile(w);
    if target == 0 {
        return "Заполните профиль (рост, вес, возраст, пол), чтобы рассчитать норму белка."
            .to_string();
    }
    let ffm = target as f64 / 1.6;
    let h = profile::get_height_cm().map(|h| format!("{h:.0} см")).unwrap_or_default();
    let age = profile::get_age_years().map(|a| format!("{a}")).unwrap_or_default();
    format!(
        "Норму белка считаем от безжировой массы тела. По вашим данным (рост {h}, возраст \
         {age}, вес {w:.0} кг) безжировая масса ≈ {ffm:.0} кг, а планка — 1,6 г белка на кг.\n\n\
         Ваша планка по белку: {target} г."
    )
}

/// Пояснение «?» для недельного железа: откуда взялась норма и почему она
/// считается в УСВОЕННЫХ миллиграммах.
fn iron_hint_text() -> String {
    use crate::services::iron;
    let sex = profile::get_sex();
    let age = profile::get_age_years();
    // Показываем ИМЕННО то число, от которого построена планка. У менструирующих
    // женщин это средняя потребность, а не RDA: см. `iron::intake_basis_mg_per_day`.
    // Назвать здесь RDA значило бы объяснять планку тем, из чего она не сделана.
    let basis = iron::intake_basis_for_profile();
    let target = iron::weekly_absorbed_target_mg(sex, age);
    let who = match sex {
        Some(Sex::Female) => "женщинам",
        Some(Sex::Male) => "мужчинам",
        None => "людям",
    };
    let age_s = age.map(|a| format!(" в {a} лет")).unwrap_or_default();
    let menstrual = matches!(age, Some(19..=50)) && !matches!(sex, Some(Sex::Male));
    let note = if menstrual {
        "\n\nЭто СРЕДНЯЯ потребность. Официальная рекомендация выше — она рассчитана \
         на самые обильные менструации, и набрать столько из обычной еды почти \
         невозможно. Если ваши месячные обильные, норму стоит поднять."
    } else {
        ""
    };
    format!(
        "По таблице норм потребления {who}{age_s} нужно {basis:.1} мг железа в сутки.{note}\n\n\
         Из еды усваивается лишь часть железа: из мяса и печени — заметно больше, чем из \
         растений. Поэтому мы считаем УСВОЕННОЕ железо, и недельная норма в этом счёте — \
         {target:.1} мг.\n\n\
         Точки на полосе делят неделю на семь дней: они показывают, где вы были бы, \
         если бы набирали норму равномерно."
    )
}

/// Пояснение «?» для гема: что считается порцией и зачем это отдельно от железа.
fn heme_hint_text() -> String {
    use crate::services::heme;
    format!(
        "Одна порция — {:.0} г белка из печени, красного мяса или моллюсков. Это \
         примерно 120 г говядины, 100 г печени или 200 г мидий.\n\n\
         Считаем отдельно от железа, потому что с этими продуктами приходит не только \
         оно: B12, цинк, селен и полноценный белок усваиваются из них заметно лучше, \
         чем из растительной пищи.\n\n\
         Цель — {:.0} порции в неделю. Точки на полосе делят неделю на семь дней.",
        heme::PORTION_PROTEIN_G,
        heme::WEEKLY_PORTIONS,
    )
}

/// Пояснения «?» для трёх жировых шкал.
fn epa_dha_hint_text() -> String {
    format!(
        "EPA и DHA — длинные омега-3. Они есть только в рыбе холодных морей, \
         морепродуктах и рыбьем жире: в растениях их нет вовсе.\n\n\
         Считаем их отдельно от АЛК, потому что организм делает EPA и DHA из АЛК с \
         конверсией в единицы процентов — льняным маслом эту норму не закрыть.\n\n\
         Цель — {:.2} г в неделю. Это закрывается двумя рыбными трапезами: порция \
         скумбрии или сельди даёт около двух граммов.",
        crate::services::fats::EPA_DHA_PER_WEEK_G,
    )
}


fn fat_ratio_hint_text() -> String {
    format!(
        "Это отношение ненасыщенных жиров к насыщенным: (МНЖК + ПНЖК) / НЖК. Оно \
         говорит не сколько жира вы съели, а КАКОЙ.\n\n\
         Цель — не меньше {:.0}. Так устроен средиземноморский рацион: оливковое масло, \
         орехи, рыба и авокадо поднимают отношение, а сливочное масло, сало, сыр, \
         шоколад и кокосовое масло опускают.\n\n\
         Считается по суммам за неделю, а не по каждому продукту: ложка оливкового \
         масла не искупает двести граммов сала.",
        crate::services::fats::UNSAT_TO_SAT_MIN,
    )
}

/// Пояснение «?» для кальция.
fn calcium_hint_text() -> String {
    "Кальций — это кости, и запас в них набирается годами, а тратится десятилетиями. \
     Планка — 1000 мг в день.\n\n\
     Больше всего его в твёрдых сырах, твороге и молочном, в кунжуте и тахини, в консервах \
     с костями (сардины, шпроты), в тофу и миндале. Из зелени он усваивается хуже — щавель \
     и шпинат связывают его собственными оксалатами.\n\n\
     Кальций и железо делят один транспорт: когда кальция много, железа усваивается меньше. \
     Большинству об этом можно не думать, но если с железом туго — разведите их по разным \
     приёмам пищи."
        .to_string()
}

/// Пояснение «?» для шагов.
fn steps_hint_text() -> String {
    "Планка по шагам — не общая рекомендация, а ВАША: она посчитана по вашей же истории и \
     поднимается по мере того, как вы к ней привыкаете.\n\n\
     Ходьба — единственная нагрузка, которая почти ничего не требует и работает каждый день: \
     она тратит калории, не разгоняя аппетит так, как тяжёлые тренировки.\n\n\
     День без записи о шагах не судится: это не провал, а отсутствие данных."
        .to_string()
}

/// The veg/fruit "?" text — sex-specific.
fn veg_hint_text() -> String {
    let who = match profile::get_sex() {
        Some(Sex::Female) => "Женщинам",
        _ => "Мужчинам",
    };
    format!(
        "{who} рекомендуется употреблять 400–600 г овощей и фруктов в день. Они могут быть в \
         готовом или сыром виде."
    )
}

/// Что именно закрывается за неделю — у НЕДЕЛЬНЫХ индикаторов. `None` у дневных.
///
/// Недельные судятся не по дням: съесть недельную норму «сегодня» нельзя и не нужно,
/// поэтому у них считаются ЗАКРЫТЫЕ недели, а `missed` означает недели, а не дни.
fn weekly_metric_name(key: &str) -> Option<&'static str> {
    match key {
        "iron" => Some("норму железа"),
        "heme" => Some("норму порций гемовых продуктов"),
        "epa_dha" => Some("норму морских омега-3"),
        "fat_ratio" => Some("нужное отношение ненасыщенных жиров к насыщенным"),
        _ => None,
    }
}

/// У дневных — как назвать норму в родительном падеже: «не добрали норму …».
fn daily_metric_name(key: &str) -> &'static str {
    match key {
        "protein" => "белка",
        "veg_fruit" => "овощей и фруктов",
        "calcium" => "кальция",
        "steps" => "шагов",
        "fiber" => "клетчатки",
        _ => "нормы",
    }
}

/// ПЕРВЫЙ абзац пояснения «?»: чем вызван цвет. Дальше идёт текст про сам показатель
/// — свой у каждого индикатора, см. [`indicator_body`].
fn indicator_verdict(key: &str, state: IndicatorState, missed: u32) -> String {
    use IndicatorState::*;
    if let Some(what) = weekly_metric_name(key) {
        return match state {
            Green => format!(
                "Зелёный: все недели, которые можно оценить, вы закрывали {what}. Считаются \
                 только ЗАВЕРШЁННЫЕ недели, текущая не судится — это набирается неделей, а не \
                 за день."
            ),
            Orange => format!(
                "Оранжевый: {missed} недель из последних оценённых остались незакрытыми \
                 (хотя бы одна → оранжевый). Окно — последние восемь недель; если их прошло \
                 меньше, берутся все, что есть."
            ),
            Red => format!(
                "Красный: {missed} недель остались незакрытыми — это половина или больше из \
                 тех, что можно оценить."
            ),
            Unknown => "Пока не завершилась ни одна неделя — оценивать нечего.".to_string(),
        };
    }
    // Калории судятся КОРИДОРОМ (±50 ккал), а не «добрать норму»: перебор и недобор
    // одинаково выводят из планки.
    if key == "calories" {
        return match state {
            Green => "Зелёный: за последнюю неделю вы каждый день попадали в свою планку по \
                      калориям (±50 ккал)."
                .to_string(),
            Orange => format!(
                "Оранжевый: за последнюю неделю вы {missed} из 7 дней не попали в планку по \
                 калориям (±50 ккал; 1–3 дня → оранжевый)."
            ),
            Red => format!(
                "Красный: за последнюю неделю вы {missed} из 7 дней не попали в планку по \
                 калориям (±50 ккал; 4 и более → красный)."
            ),
            Unknown => "Пока недостаточно данных, чтобы оценить.".to_string(),
        };
    }
    let metric = daily_metric_name(key);
    match state {
        Green => format!("Зелёный: за последнюю неделю вы каждый день добирали норму {metric}."),
        Orange => format!(
            "Оранжевый: за последнюю неделю вы {missed} из 7 дней не добрали норму {metric} \
             (1–3 дня → оранжевый)."
        ),
        Red => format!(
            "Красный: за последнюю неделю вы {missed} из 7 дней не добрали норму {metric} \
             (4 и более → красный)."
        ),
        Unknown => "Пока недостаточно данных, чтобы оценить.".to_string(),
    }
}

/// Текст про САМ показатель: что меряем, откуда норма, где это взять.
///
/// Свой у каждого индикатора. Общего текста тут быть не должно: правило цвета
/// одинаковое у всех и уже сказано выше, а «не добрали норму нормы» — это не
/// объяснение. Один и тот же текст показывается и под «?» у шкалы, и под «?» у
/// индикатора: показатель один, значит и объяснение одно.
async fn indicator_body(key: &str, planka: Option<f64>) -> String {
    match key {
        "calories" => calorie_hint_text(planka).await,
        "protein" => protein_hint_text().await,
        "veg_fruit" => veg_hint_text(),
        "calcium" => calcium_hint_text(),
        "steps" => steps_hint_text(),
        "iron" => iron_hint_text(),
        "heme" => heme_hint_text(),
        "epa_dha" => epa_dha_hint_text(),
        "fat_ratio" => fat_ratio_hint_text(),
        // Нового индикатора без своего текста быть не должно — падаем громко, а не
        // показываем человеку пустоту.
        _ => panic!("indicator_body: нет пояснения для индикатора {key:?}"),
    }
}

fn indicator_reason(key: &str, state: IndicatorState, missed: u32, body: &str) -> String {
    let verdict = indicator_verdict(key, state, missed);
    if body.is_empty() {
        return verdict;
    }
    format!("{verdict}\n\n{body}")
}

/// Current network problems as error-log entries, so they appear in the SAME list
/// as background errors (and drive the ⚠ tile) instead of a banner over the UI.
/// Reads the `net` signals, so it auto-clears when connectivity is restored — no
/// mutation of the persistent error log. `None` (unprobed) is not a problem.
fn net_problem_entries() -> Vec<AppError> {
    let mut v = Vec::new();
    if net::is_online().get() == Some(false) {
        v.push(AppError {
            context: t("net.offline_title").to_string(),
            message: t("net.offline_body_vpn").to_string(),
            cause: None,
            kind: String::new(),
        });
    }
    let down = net::degraded().get();
    if !down.is_empty() {
        let names = down.iter().map(|w| t(w.label_key())).collect::<Vec<_>>().join(", ");
        v.push(AppError {
            context: t("net.degraded_title").to_string(),
            message: format!("{} {}", t("net.degraded_body"), names),
            cause: None,
            kind: String::new(),
        });
    }
    v
}

// Process-lifetime caches for the async widget data, so re-navigating to the
// dashboard paints the widgets with the last-known values on the FIRST frame
// instead of flashing an empty/placeholder state (see `services::sticky`).
thread_local! {
    static WEIGHT_CACHE: RefCell<Option<Vec<WeightEntry>>> = const { RefCell::new(None) };
    static STEPS_CACHE: RefCell<Option<Vec<StepEntry>>> = const { RefCell::new(None) };
}

// Bare 4×3 tile wrapper: the weight/steps widgets bring their own card, so this
// button is transparent and just fills the grid area to open the chart modal.
const WIDGET_TILE: &str = "appearance: none; -webkit-appearance: none; border: none; background: none; \
    padding: 0; margin: 0; cursor: pointer; font: inherit; color: inherit; text-align: left; display: block;";

/// Which widget's editor is open over the grid (None = just the grid).
#[derive(Clone, Copy, PartialEq)]
enum Overlay {
    None,
    Persona,
    Notifications,
    Cycle,
    Errors,
    Mail,
    Progress,
    Weight,
    Steps,
}

// 8 columns; each cell is a square whose side `--u` is derived from the viewport
// width minus the app-shell's 0.75rem side padding and the inter-cell gaps.
const GRID: &str = "--gap: 6px; --u: calc((100vw - 1.5rem - 7 * var(--gap)) / 8); \
    display: grid; grid-template-columns: repeat(8, 1fr); grid-auto-rows: var(--u); gap: var(--gap);";

const TILE: &str = "appearance: none; -webkit-appearance: none; border: none; font: inherit; \
    color: inherit; text-align: left; cursor: pointer; background: var(--bulma-scheme-main); \
    border-radius: 16px; box-shadow: 0 2px 10px rgba(0,0,0,0.06); overflow: hidden; \
    display: flex; flex-direction: column; align-items: center; justify-content: center; padding: 10px;";

// An open editor fills the dashboard area; min-height keeps it "full-screen" while
// still sitting inside the scroll container (so the bottom nav stays clear).
const EDITOR: &str = "display: flex; flex-direction: column; gap: 14px; min-height: calc(100dvh - 5.5rem);";

#[component]
pub fn DashboardPage() -> impl IntoView {
    // Profile reads are synchronous (cached); bump this to re-read after an edit.
    let bump = create_rw_signal(0u32);
    // …and re-read when SYNC brings a profile from another device.
    let profile_ver = profile::version_signal();

    // Уже заполненную персону не спрашивают заново. Сверка ОДНОРАЗОВАЯ, на входе,
    // и намеренно не реактивная: иначе экран закрывался бы сам в тот момент, когда
    // человек дозаполнил последнее поле, — то есть кнопка «Готово» никогда бы и не
    // понадобилась. Сюда попадают те, кто заполнил персону до появления этой
    // кнопки, и те, кому профиль приехал синком с другого устройства.
    if !app_flags::get_bool(PERSONA_CONFIRMED)
        && profile::get_height_cm().is_some()
        && profile::get_birth_year().is_some()
        && profile::get_sex().is_some()
    {
        app_flags::set_bool(PERSONA_CONFIRMED, true);
    }

    // Первый заход: человек ещё ни разу не подтвердил персону кнопкой «Готово».
    // Отметка синкается — подтвердив на одном устройстве, на втором не спрашивают.
    let persona_first_run = move || {
        bump.get();
        profile_ver.get();
        !app_flags::get_bool(PERSONA_CONFIRMED)
    };

    let overlay = create_rw_signal(Overlay::None);
    // Persona takes over the whole screen on the first run OR when re-opened.
    let persona_full = move || persona_first_run() || overlay.get() == Overlay::Persona;
    // The cycle widget is female-only.
    let is_female = move || {
        bump.get();
        profile::get_sex() == Some(Sex::Female)
    };

    // Notifications state for the bell: `configured` (a test notification was
    // received → stop jiggling; owned by the push service) and `disabled` (the
    // master kill-switch → cross the bell out; re-read when the schedule changes).
    let meta_ver = db::version("_sync_meta");
    let notif_configured = crate::services::push::received_signal();
    let notif_disabled = create_rw_signal(false);
    create_effect(move |_| {
        meta_ver.get();
        spawn_local(async move {
            let d = db::get::<serde_json::Value>("_sync_meta", "notification_schedule")
                .await
                .and_then(|v| v.get("disabled").and_then(|x| x.as_bool()))
                .unwrap_or(false);
            notif_disabled.set(d);
        });
    });

    // Weight & steps widgets (moved here from the diary). Resources refresh when
    // their stores change; the tiles open the same chart modals.
    let weight_ver = db::version("weight_entries");
    let weight_res = create_resource(move || weight_ver.get(), |_| async { local::list_weight_entries().await });
    // `_data` is `None` only before the first-ever load (→ render nothing); after
    // that it's the fresh-or-last-known Vec, so switching panels shows data at once.
    let weight_data = move || sticky(&WEIGHT_CACHE, weight_res.get());
    let weight_entries = move || weight_data().unwrap_or_default();
    let steps_ver = db::version("step_entries");
    let steps_res = create_resource(move || steps_ver.get(), |_| async { local::list_step_entries().await });
    let steps_data = move || sticky(&STEPS_CACHE, steps_res.get());
    let steps_entries = move || steps_data().unwrap_or_default();

    // The expanded «Калории» view (opened by tapping the widget): the same gauges as
    // the widget + a breakdown of the daily indicators + the "?" explanations.
    // Fetched ONLY while the overlay is open, refreshed when the diary / foods /
    // weight / goals change.
    let diary_ver = db::version("diary");
    let foods_ver = db::version("foods");
    let goals_ver = db::version("goals");
    let detail_res = create_local_resource(
        move || {
            (
                overlay.get() == Overlay::Progress,
                diary_ver.get(),
                foods_ver.get(),
                weight_ver.get(),
                goals_ver.get(),
            )
        },
        |(open, _, _, _, _)| async move {
            if !open {
                return None;
            }
            let today = chrono::Local::now().format("%Y-%m-%d").to_string();
            let planka = local::calorie_goal_amount().await;
            let mut bodies = std::collections::HashMap::new();
            for key in indicators::displayed_indicators() {
                bodies.insert(key, indicator_body(key, planka).await);
            }
            Some(DetailData {
                planka,
                eaten: local::kcal_on(&today).await,
                gauges: indicators::daily_gauges().await,
                iron: crate::services::iron::weekly_progress().await,
                heme: crate::services::heme::weekly_progress().await,
                fats: crate::services::fats::weekly_progress().await,
                series: indicators::unlocked_indicator_series().await,
                calorie_hint: calorie_hint_text(planka).await,
                protein_hint: protein_hint_text().await,
                veg_hint: veg_hint_text(),
                bodies,
            })
        },
    );

    // Problems tile: the ⚠ tile (left of the bell) appears when there are
    // background errors OR a network problem. Network problems are shown in the
    // SAME list (ErrorsPanel) — no banner covering the interface.
    let errs = crate::services::errors::signal();
    let has_errors = move || !errs.get().is_empty() || !net_problem_entries().is_empty();

    // Mail tile: a program-letters inbox that permanently occupies col 7. A red dot
    // marks unread letters. The version signal bumps when a letter is added/read.
    let letters_ver = crate::services::letters::version_signal();
    let has_unread_mail = move || {
        letters_ver.get();
        crate::services::letters::has_unread()
    };

    view! {
        {move || {
            if persona_full() {
                // Первый заход объясняет, зачем спрашивают, и заканчивается
                // кнопкой «Готово»: она разбирает форму и называет незаполненное.
                // Открытый заново экран — прежний виджет: человек уже знает, что
                // это, и уходит с него кнопкой в шапке.
                let first = persona_first_run();
                view! {
                    <div style=EDITOR>
                        <EditorHead title="dashboard.persona_title"
                            show_done=Signal::derive(move || !first)
                            on_done=move || overlay.set(Overlay::None)/>
                        <PersonaEditor bump first_run=first
                            on_done=Callback::new(move |_| overlay.set(Overlay::None))/>
                    </div>
                }.into_view()
            } else if overlay.get() == Overlay::Notifications {
                view! {
                    <div style=EDITOR>
                        <EditorHead title="dashboard.notifications_title"
                            show_done=Signal::derive(|| true)
                            on_done=move || overlay.set(Overlay::None)/>
                        <NotifyPanel hide_check_after_received=true/>
                    </div>
                }.into_view()
            } else if overlay.get() == Overlay::Cycle {
                view! {
                    <div style=EDITOR>
                        <EditorHead title="cycle.title"
                            show_done=Signal::derive(|| true)
                            on_done=move || overlay.set(Overlay::None)/>
                        <CyclePanel/>
                    </div>
                }.into_view()
            } else if overlay.get() == Overlay::Errors {
                view! {
                    <div style=EDITOR>
                        <EditorHead title="errors.title"
                            show_done=Signal::derive(|| true)
                            on_done=move || overlay.set(Overlay::None)/>
                        <ErrorsPanel/>
                    </div>
                }.into_view()
            } else if overlay.get() == Overlay::Mail {
                view! {
                    <div style=EDITOR>
                        <EditorHead title="mail.title"
                            show_done=Signal::derive(|| true)
                            on_done=move || overlay.set(Overlay::None)/>
                        <LettersPanel/>
                    </div>
                }.into_view()
            } else if overlay.get() == Overlay::Progress {
                view! {
                    <div style=EDITOR>
                        <EditorHead title="dashboard.calories_title"
                            show_done=Signal::derive(|| true)
                            on_done=move || overlay.set(Overlay::None)/>
                        {move || {
                            // Wait for the detail data. Still loading → spinner.
                            let Some(d) = detail_res.get().flatten() else {
                                return view! {
                                    <div style="display: flex; justify-content: center; padding: 2rem;">
                                        <div class="ft-spinner"></div>
                                    </div>
                                }.into_view();
                            };
                            let Some(target) = d.planka else {
                                // No planka yet (still collecting the week).
                                return view! {
                                    <p class="is-size-7 has-text-grey" style="padding: 1.5rem 0.5rem; text-align: center;">
                                        "Планка ещё не рассчитана — ведите дневник неделю."
                                    </p>
                                }.into_view();
                            };
                            // Planka set → the same gauges as the widget (each with a "?"
                            // explaining where its target came from) + an advisory breakdown
                            // of the daily indicators. Order the hint by gauge key.
                            let cal_color =
                                if d.eaten > target { "#e0304f" } else { "#1fa463" }.to_string();
                            let (protein_hint, veg_hint) = (d.protein_hint.clone(), d.veg_hint.clone());
                            let daily = d.gauges.iter().map(|g| {
                                let hint = if g.key == "protein" { protein_hint.clone() } else { veg_hint.clone() };
                                // At-least goals: neutral until met, green when met.
                                let (bar, val) = crate::components::gauge::at_least_colors(g.value, g.target);
                                view! {
                                    <Gauge value=g.value target=g.target
                                        label=gauge_label(g.key).to_string()
                                        unit=g.unit.to_string()
                                        color=bar.to_string()
                                        hint=hint
                                        value_color=val.map(String::from)/>
                                }
                            }).collect_view();
                            // Недельное железо — та же полоса с суточными точками и
                            // обводкой темпа, что в свёрнутом виджете.
                            let iron = d.iron.clone().map(|w| {
                                let (bar, val) =
                                    crate::components::gauge::at_least_colors(w.absorbed_mg, w.target_mg);
                                let pace = crate::components::gauge::GaugePace {
                                    segments: 7,
                                    passed: w.day_of_week.saturating_sub(1),
                                };
                                view! {
                                    <Gauge value=w.absorbed_mg target=w.target_mg
                                        label="Железо/нед".to_string()
                                        unit="мг".to_string()
                                        color=bar.to_string()
                                        height=12.0
                                        decimals=1
                                        pace=Some(pace)
                                        hint=iron_hint_text()
                                        value_color=val.map(String::from)/>
                                }
                            });
                            // Порции гемовых продуктов — своя шкала под железной.
                            // Дробное значение намеренно: «1,2 из 3» честнее, чем
                            // округлённое «1», когда съеден кусок мяса поменьше.
                            let heme = d.heme.as_ref().map(|w| {
                                let (bar, val) =
                                    crate::components::gauge::at_least_colors(w.portions, w.target);
                                let pace = crate::components::gauge::GaugePace {
                                    segments: 7,
                                    passed: w.day_of_week.saturating_sub(1),
                                };
                                view! {
                                    <Gauge value=w.portions target=w.target
                                        label="Гем/нед".to_string()
                                        // Без единицы — как и в самом виджете.
                                        unit=String::new()
                                        color=bar.to_string()
                                        height=12.0
                                        decimals=2
                                        pace=Some(pace)
                                        hint=heme_hint_text()
                                        value_color=val.map(String::from)/>
                                }
                            });
                            // Три жировые шкалы: морские омега-3, растительная АЛК и
                            // качество жира в целом.
                            let fats = d.fats.as_ref().map(|w| {
                                let pace = crate::components::gauge::GaugePace {
                                    segments: 7,
                                    passed: w.day_of_week.saturating_sub(1),
                                };
                                // Точки темпа — только у накопительных величин, см.
                                // тот же разбор в progress_widget.
                                let cell = |value: f64, target: f64, label: &'static str,
                                            unit: &'static str, decimals: usize, paced: bool,
                                            hint: String| {
                                    let (bar, val) =
                                        crate::components::gauge::at_least_colors(value, target);
                                    let pace = paced.then(|| pace.clone());
                                    view! {
                                        <Gauge value=value target=target
                                            label=label.to_string()
                                            unit=unit.to_string()
                                            color=bar.to_string()
                                            height=12.0
                                            decimals=decimals
                                            pace=pace
                                            hint=hint
                                            value_color=val.map(String::from)/>
                                    }
                                };
                                view! {
                                    {cell(w.acids.epa_dha_g, w.epa_dha_target, "Омега-3/нед", "г", 2,
                                          true, epa_dha_hint_text())}
                                    // Баланс — расходящаяся шкала от середины, см.
                                    // `BalanceGauge`: у отношения есть сторона, а не
                                    // «сколько набрано».
                                    <crate::components::gauge::BalanceGauge
                                        value=w.ratio()
                                        target=crate::services::fats::UNSAT_TO_SAT_MIN
                                        label="Баланс жира".to_string()
                                        height=12.0
                                        hint=fat_ratio_hint_text()/>
                                }
                            });
                            let detail = d.series.iter().map(|s| {
                                let (paths, name) = progress_widget::icon_for(s.key);
                                let (stroke, tint) = progress_widget::state_colors(s.state);
                                let body = d.bodies.get(s.key).map(String::as_str).unwrap_or("");
                                let reason = indicator_reason(s.key, s.state, s.missed, body);
                                let days = s.days.clone();
                                // Each indicator on its own bordered panel: a header row
                                // [icon] [name] … [?] naming which indicator this is, with
                                // the histogram full-width below.
                                view! {
                                    <div attr:data-ind-panel=s.key
                                        style="display: flex; flex-direction: column; gap: 6px; \
                                            border: 0.5px solid var(--bulma-border-weak); border-radius: 12px; \
                                            padding: 8px 10px; background: var(--bulma-scheme-main-bis);">
                                        <div style="display: flex; align-items: center; gap: 8px;">
                                            <div style=format!("width: 28px; height: 28px; min-width: 28px; border-radius: 50%; \
                                                    background: {tint}; display: flex; align-items: center; justify-content: center;")>
                                                <svg xmlns="http://www.w3.org/2000/svg" width="16" height="16" viewBox="0 0 24 24"
                                                    fill="none" stroke=stroke stroke-width="2" stroke-linecap="round"
                                                    stroke-linejoin="round" inner_html=paths></svg>
                                            </div>
                                            <span class="has-text-weight-semibold" style="flex: 1; min-width: 0;">{name}</span>
                                            <InfoHint text=reason/>
                                        </div>
                                        <DayBars series=Signal::derive(move || days.clone())
                                            unit={match s.key {
                                                "calories" => "ккал",
                                                "iron" => "мг",
                                                "heme" => "",
                                                "epa_dha" => "г",
                                                "fat_ratio" => "",
                                                "steps" => "",
                                                _ => "г",
                                            }.to_string()}
                                            miss_color=stroke.to_string()
                                            labels=s.labels.clone()
                                            met=s.met_days.clone()/>
                                    </div>
                                }
                            }).collect_view();
                            view! {
                                <div style="display: flex; flex-direction: column; gap: 18px; padding: 4px 2px;">
                                    <Gauge value=d.eaten target=target
                                        label="Калории".to_string() unit="ккал".to_string()
                                        color=cal_color height=12.0
                                        hint=d.calorie_hint.clone()
                                        value_color={(d.eaten > target).then(|| "#e0304f".to_string())}/>
                                    {daily}
                                    {iron}
                                    {heme}
                                    {fats}
                                    <div>
                                        <div style="border-top: 0.5px solid var(--bulma-border-weak); margin-bottom: 14px;"></div>
                                        <div style="display: flex; flex-direction: column; gap: 12px;">
                                            {detail}
                                        </div>
                                    </div>
                                </div>
                            }.into_view()
                        }}
                    </div>
                }.into_view()
            } else if overlay.get() == Overlay::Weight {
                view! {
                    <div style=EDITOR>
                        <EditorHead title="weight.widget_title"
                            show_done=Signal::derive(|| true)
                            on_done=move || overlay.set(Overlay::None)/>
                        <WeightPanel entries=Signal::derive(weight_entries)
                            on_close=Callback::new(move |_| overlay.set(Overlay::None))/>
                    </div>
                }.into_view()
            } else if overlay.get() == Overlay::Steps {
                view! {
                    <div style=EDITOR>
                        <EditorHead title="steps.title"
                            show_done=Signal::derive(|| true)
                            on_done=move || overlay.set(Overlay::None)/>
                        <StepsPanel entries=Signal::derive(steps_entries)
                            on_close=Callback::new(move |_| overlay.set(Overlay::None))/>
                    </div>
                }.into_view()
            } else {
                // Collapsed grid: persona 1×1 + notifications bell 1×1.
                view! {
                    <div style="display: flex; flex-direction: column; gap: 12px;">
                        // Top row: persona (col 1) · error (col 7) · notifications (col 8).
                        <div style=GRID>
                            <button style=format!("{TILE} grid-column: 1 / 2; grid-row: span 1;")
                                on:click=move |_| overlay.set(Overlay::Persona)>
                                {icon_user()}
                            </button>
                            // Error tile (⚠, orange) — col 6, one cell left of the mail
                            // tile. Shown only when the background queue recorded errors
                            // or there is a network problem.
                            {move || has_errors().then(|| view! {
                                <button style=format!("{TILE} grid-column: 6 / 7; grid-row: span 1;")
                                    attr:data-testid="dash-errors-widget"
                                    on:click=move |_| overlay.set(Overlay::Errors)>
                                    {icon_alert()}
                                </button>
                            })}

                            // Mail tile (col 7) — ALWAYS present (unlike the errors tile).
                            // A red dot marks unread program letters.
                            <button style=format!("{TILE} grid-column: 7 / 8; grid-row: span 1; position: relative;")
                                attr:data-testid="dash-mail-widget"
                                on:click=move |_| overlay.set(Overlay::Mail)>
                                {icon_mail()}
                                {move || has_unread_mail().then(|| view! {
                                    <span style="position: absolute; top: 8px; right: 8px; width: 9px; height: 9px; \
                                        border-radius: 50%; background: #e0304f; \
                                        box-shadow: 0 0 0 2px var(--bulma-scheme-main);"></span>
                                })}
                            </button>

                            // Notifications bell lives in the FAR-RIGHT cell (col 8).
                            // It jiggles only until notifications are configured, and
                            // is drawn crossed-out (bell-off) while the kill-switch is on.
                            <button style=format!("{TILE} grid-column: 8 / 9; grid-row: span 1;")
                                on:click=move |_| overlay.set(Overlay::Notifications)>
                                <span class=move || if notif_configured.get() || notif_disabled.get() { "" } else { "dash-bell-jiggle" }
                                    style="display: inline-flex; transform-origin: 50% 10%;">
                                    {move || if notif_disabled.get() { icon_bell_off().into_view() } else { icon_bell().into_view() }}
                                </span>
                            </button>
                        </div>

                        // Stories tray — under the persona/notifications/warning row,
                        // above the weight/steps/nutrition widgets.
                        <crate::components::story_tray::StoryTray/>

                        // Weight & steps widgets: 4×3 tiles side by side.
                        <div style=GRID>
                            <button style=format!("{WIDGET_TILE} grid-column: 1 / 5; grid-row: 1 / 4;")
                                attr:data-testid="dash-weight-widget"
                                on:click=move |_| overlay.set(Overlay::Weight)>
                                // Empty until the first data load (then sticky keeps it
                                // filled across navigations) — no placeholder flash.
                                {move || weight_data().map(|_| view! { <WeightWidget entries=Signal::derive(weight_entries)/> })}
                            </button>
                            <button style=format!("{WIDGET_TILE} grid-column: 5 / 9; grid-row: 1 / 4;")
                                attr:data-testid="dash-steps-widget"
                                on:click=move |_| overlay.set(Overlay::Steps)>
                                {move || steps_data().map(|_| view! { <StepsWidget entries=Signal::derive(steps_entries)/> })}
                            </button>
                        </div>

                        // Progress widget + cycle flow BELOW the grid (not inside it),
                        // so the card grows to fit its content — the indicators row
                        // sits at the very bottom instead of being clipped.
                        //
                        // Tapping the card opens the expanded «Калории» view. Uses
                        // `pointerup` (native, fires on iOS unlike a delegated <div>
                        // click); the help link/buttons inside stop propagation so
                        // they keep their own action.
                        <div on:pointerup=move |_| overlay.set(Overlay::Progress)>
                            <ProgressWidget/>
                        </div>

                        // Cycle widget (female only): full-width single line.
                        {move || is_female().then(|| view! {
                            <button style=WIDGET_TILE
                                on:click=move |_| overlay.set(Overlay::Cycle)>
                                <CycleLine/>
                            </button>
                        })}
                    </div>
                }.into_view()
            }
        }}
    }
}

/// Editor header: the widget title + a "Done" button (shown only when `show_done`).
#[component]
fn EditorHead(
    title: &'static str,
    #[prop(into)] show_done: MaybeSignal<bool>,
    on_done: impl Fn() + 'static + Copy,
) -> impl IntoView {
    view! {
        <div style="display: flex; align-items: center; justify-content: space-between; margin: 4px 4px 0;">
            <h1 class="is-size-4 has-text-weight-bold" style="margin: 0;">{move || t(title)}</h1>
            {move || show_done.get().then(|| view! {
                <button class="button is-small is-light" on:click=move |_| on_done()>
                    {move || t("dashboard.close")}
                </button>
            })}
        </div>
    }
}

/// Persona editor: sex, height, birth year and course goal. Every control writes
/// straight to the profile and bumps the dashboard so completeness re-evaluates.
///
/// `first_run` — экран, который человек видит сразу после установки. Тогда сверху
/// объяснение, зачем это спрашивают, а снизу кнопка «Готово»: она разбирает форму,
/// называет незаполненное и подсвечивает виноватые поля. Отсутствие такой кнопки
/// раньше выглядело поломкой — заполнил, а дальше хода нет.
#[component]
fn PersonaEditor(
    bump: RwSignal<u32>,
    #[prop(optional)] first_run: bool,
    #[prop(optional)] on_done: Option<Callback<()>>,
) -> impl IntoView {
    // Initial values captured once. We deliberately DON'T reactively control the
    // <select> value: a reactive `prop:value` fought the native selection and
    // reverted the shown option even though the value was already saved. The editor
    // is recreated every time it opens, so a one-time `selected` is enough.
    let sex0 = profile::get_sex();
    let goal0 = profile::get_goal();
    let pick_sex = move |s: Sex| {
        profile::set_sex(s);
        bump.update(|v| *v += 1);
    };
    let pick_goal = move |g: CourseGoal| {
        profile::set_goal(g);
        bump.update(|v| *v += 1);
    };

    // Виноватые поля после разбора формы. До первого нажатия «Готово» ничего не
    // красное: человек ещё не заявлял, что закончил, и ругать его не за что.
    let sex_bad = create_rw_signal(false);
    let height_bad = create_rw_signal(false);
    let year_bad = create_rw_signal(false);
    // Что именно не так — по одной строке на поле, словами.
    let problems = create_rw_signal(Vec::<&'static str>::new());

    // Разобрать форму. Пустой список — можно уходить.
    let check = move || {
        bump.get();
        let no_sex = profile::get_sex().is_none();
        let no_height = profile::get_height_cm().is_none();
        let no_year = profile::get_birth_year().is_none();
        sex_bad.set(no_sex);
        height_bad.set(no_height);
        year_bad.set(no_year);
        let mut out: Vec<&'static str> = Vec::new();
        if no_sex {
            out.push(t("persona.need_sex"));
        }
        if no_height {
            out.push(t("persona.need_height"));
        }
        if no_year {
            out.push(t("persona.need_year"));
        }
        out
    };

    let submit = move |_| {
        let found = check();
        let ok = found.is_empty();
        problems.set(found);
        if ok {
            app_flags::set_bool(PERSONA_CONFIRMED, true);
            bump.update(|v| *v += 1);
            if let Some(cb) = on_done {
                cb.call(());
            }
        }
    };

    // Красная рамка на поле, к которому есть вопрос.
    let bad_ring = |bad: RwSignal<bool>| {
        move || {
            if bad.get() {
                "box-shadow: inset 0 0 0 2px var(--bulma-danger);"
            } else {
                ""
            }
        }
    };

    // Right-aligned number field on its row.
    let field = "background: var(--bulma-scheme-main); border: none; border-radius: 10px; \
                 padding: 10px 12px; width: 110px; text-align: right; color: var(--bulma-text); font: inherit;";
    // Compact native select for the goal.
    let select = "background: var(--bulma-scheme-main); border: none; border-radius: 10px; \
                  padding: 9px 10px; color: var(--bulma-text); font: inherit;";
    // Each field is one row: label on the left, control on the right.
    let row = "display: flex; align-items: center; justify-content: space-between; gap: 12px; min-height: 44px;";
    let label = "margin: 0;";

    view! {
        <div style="display: flex; flex-direction: column; gap: 8px;">
            {first_run.then(|| view! {
                <p class="has-text-grey" style="line-height: 1.55; margin: 2px 0 14px;">
                    {move || t("persona.intro")}
                </p>
            })}

            <div style=row>
                <span class="is-size-6" style=label>{move || t("dashboard.sex")}</span>
                <select
                    attr:data-testid="persona-sex"
                    style=move || format!("{select}{}", bad_ring(sex_bad)())
                    on:change=move |ev| {
                        match event_target_value(&ev).as_str() {
                            "male" => pick_sex(Sex::Male),
                            "female" => pick_sex(Sex::Female),
                            _ => {}
                        }
                        sex_bad.set(false);
                    }>
                    // Empty placeholder until a sex is chosen (keeps the profile incomplete).
                    <option value="" selected=sex0.is_none() disabled hidden></option>
                    <option value="male" selected=sex0 == Some(Sex::Male)>{move || t("dashboard.sex_male")}</option>
                    <option value="female" selected=sex0 == Some(Sex::Female)>{move || t("dashboard.sex_female")}</option>
                </select>
            </div>

            <div style=row>
                <span class="is-size-6" style=label>{move || t("dashboard.height")}</span>
                // Сохраняем по вводу, а не по уходу с поля: на телефоне из
                // числовой клавиатуры можно так и не «уйти», и заполненное поле
                // осталось бы несохранённым.
                <input type="number" inputmode="numeric" min="80" max="250"
                    attr:data-testid="persona-height"
                    style=move || format!("{field}{}", bad_ring(height_bad)())
                    prop:value=move || { bump.get(); profile::get_height_cm().map(|h| (h as i64).to_string()).unwrap_or_default() }
                    on:input=move |ev| {
                        let raw = event_target_value(&ev);
                        let raw = raw.trim();
                        let ok = raw.parse::<f64>().ok().filter(|v| (80.0..=250.0).contains(v));
                        if let Some(v) = ok {
                            profile::set_height_cm(v);
                            height_bad.set(false);
                            bump.update(|x| *x += 1);
                        }
                    }/>
            </div>

            <div style=row>
                <span class="is-size-6" style=label>{move || t("dashboard.birth_year")}</span>
                <input type="number" inputmode="numeric" min="1900" max="2026"
                    attr:data-testid="persona-year"
                    style=move || format!("{field}{}", bad_ring(year_bad)())
                    prop:value=move || { bump.get(); profile::get_birth_year().map(|y| y.to_string()).unwrap_or_default() }
                    on:input=move |ev| {
                        let raw = event_target_value(&ev);
                        let raw = raw.trim();
                        let ok = raw.parse::<i32>().ok().filter(|v| (1900..=2026).contains(v));
                        if let Some(v) = ok {
                            profile::set_birth_year(v);
                            year_bad.set(false);
                            bump.update(|x| *x += 1);
                        }
                    }/>
            </div>

            <div style=row>
                <span class="is-size-6" style=label>{move || t("dashboard.goal")}</span>
                <select style=select
                    on:change=move |ev| {
                        let g = match event_target_value(&ev).as_str() {
                            "gain" => CourseGoal::Gain,
                            "maintain" => CourseGoal::Maintain,
                            _ => CourseGoal::Lose,
                        };
                        pick_goal(g);
                    }>
                    <option value="lose" selected=goal0 == CourseGoal::Lose>{move || t("dashboard.goal_lose")}</option>
                    <option value="gain" selected=goal0 == CourseGoal::Gain>{move || t("dashboard.goal_gain")}</option>
                    <option value="maintain" selected=goal0 == CourseGoal::Maintain>{move || t("dashboard.goal_maintain")}</option>
                </select>
            </div>

            {move || {
                let p = problems.get();
                (!p.is_empty()).then(|| view! {
                    <div attr:data-testid="persona-problems"
                         class="has-text-danger is-size-7" style="margin-top: 10px;">
                        {p.into_iter().map(|line| view! {
                            <p style="margin: 0 0 2px;">{line}</p>
                        }).collect_view()}
                    </div>
                })
            }}

            {(first_run && on_done.is_some()).then(|| view! {
                <button
                    attr:data-testid="persona-btn-done"
                    class="button is-link is-medium is-fullwidth has-text-weight-semibold"
                    style="margin-top: 18px;"
                    on:click=submit
                >
                    {move || t("dashboard.close")}
                </button>
            })}
        </div>
    }
}

/// Full-panel list of background errors. Each row is tappable to copy its text to
/// the clipboard; a «clear» button empties the log.
#[component]
fn ErrorsPanel() -> impl IntoView {
    let errs = crate::services::errors::signal();
    let copied = create_rw_signal(None::<usize>);
    view! {
        <p class="is-size-7 has-text-grey" style="margin: 0 0 10px;">{move || t("errors.hint")}</p>
        <div style="display: flex; flex-direction: column; gap: 8px;">
            {move || {
                // Network problems first (live, from `net`), then the recorded
                // background errors — one combined list.
                let mut list = net_problem_entries();
                list.extend(errs.get());
                if list.is_empty() {
                    return view! { <p class="is-size-6 has-text-grey">{move || t("errors.none")}</p> }.into_view();
                }
                list.into_iter().enumerate().map(|(i, e)| {
                    let text = e.as_text();
                    // A real <button> — a <div on:click> is dead on iOS (Leptos
                    // delegates clicks and iOS only bubbles them from interactive
                    // elements). Tint the row green briefly to confirm the copy.
                    view! {
                        <button
                            style=move || format!(
                                "display: block; width: 100%; text-align: left; height: auto; \
                                 white-space: normal; border: none; border-radius: 12px; padding: 12px 14px; \
                                 cursor: pointer; font: inherit; color: inherit; transition: background 0.15s; \
                                 background: {};",
                                if copied.get() == Some(i) { "var(--bulma-success-soft)" } else { "var(--bulma-scheme-main)" }
                            )
                            on:click=move |_| {
                                // Call clipboard.writeText SYNCHRONOUSLY inside the gesture
                                // — iOS Safari drops it if deferred (spawn_local).
                                copy_to_clipboard(&text);
                                copied.set(Some(i));
                            }>
                            <p class="is-size-6 has-text-weight-semibold">{e.context.clone()}</p>
                            <p class="is-size-7 has-text-grey" style="white-space: pre-wrap; word-break: break-word; margin-top: 2px;">
                                {e.message.clone()}
                            </p>
                            {move || (copied.get() == Some(i)).then(|| view! {
                                <p class="is-size-7 has-text-weight-bold has-text-success" style="margin-top: 4px;">
                                    {move || t("errors.copied")}
                                </p>
                            })}
                        </button>
                    }
                }).collect_view()
            }}
        </div>
        <button class="button is-light is-fullwidth is-small" style="margin-top: 14px;"
            on:click=move |_| crate::services::errors::clear()>
            {move || t("errors.clear")}
        </button>
    }
}

/// The program-letters inbox. Lists every letter (newest first) and marks them all
/// read on open — which clears the mail tile's red dot.
#[component]
fn LettersPanel() -> impl IntoView {
    let ver = crate::services::letters::version_signal();
    // Mark read AFTER mount (an effect, so the version bump doesn't fire mid-render).
    // The effect tracks no reactive deps → runs exactly once.
    create_effect(|_| crate::services::letters::mark_all_read());
    view! {
        <div style="display: flex; flex-direction: column; gap: 10px;">
            {move || {
                ver.get();
                let list = crate::services::letters::all();
                if list.is_empty() {
                    return view! {
                        <p class="is-size-6 has-text-grey" style="padding: 1rem 0.25rem;">
                            {move || t("mail.empty")}
                        </p>
                    }.into_view();
                }
                list.into_iter().map(|l| {
                    let date = l.created_at.get(0..10).unwrap_or("").to_string();
                    view! {
                        <div style="border: 0.5px solid var(--bulma-border-weak); border-radius: 14px; \
                                padding: 14px 16px; background: var(--bulma-scheme-main);">
                            <p class="is-size-7 has-text-grey" style="margin-bottom: 8px;">{date}</p>
                            <p class="is-size-6" style="white-space: pre-wrap; line-height: 1.5;">{l.body}</p>
                        </div>
                    }
                }).collect_view()
            }}
        </div>
    }
}

/// Copy text to the clipboard SYNCHRONOUSLY from the click handler. Uses two
/// mechanisms for reliability on iOS PWAs: the async Clipboard API (fire-and-forget,
/// invoked inside the gesture) AND a legacy hidden-textarea + `execCommand('copy')`
/// fallback, which works in WKWebView/older Safari where the async API is flaky.
fn copy_to_clipboard(text: &str) {
    use wasm_bindgen::JsCast;
    let Some(window) = web_sys::window() else { return };

    // Modern async Clipboard API — the promise settles after the handler returns,
    // but the API is invoked within the gesture, which is what iOS checks.
    let _ = window.navigator().clipboard().write_text(text);

    // Legacy fallback: select a hidden textarea and execCommand('copy').
    let Some(document) = window.document() else { return };
    if let Ok(el) = document.create_element("textarea") {
        let ta: web_sys::HtmlTextAreaElement = el.unchecked_into();
        ta.set_value(text);
        let _ = ta.set_attribute("readonly", "");
        let _ = ta.style().set_property("position", "fixed");
        let _ = ta.style().set_property("top", "0");
        let _ = ta.style().set_property("opacity", "0");
        if let Some(body) = document.body() {
            let _ = body.append_child(&ta);
            ta.select();
            let _ = ta.set_selection_range(0, text.len() as u32);
            if let Ok(html_doc) = document.dyn_into::<web_sys::HtmlDocument>() {
                let _ = html_doc.exec_command("copy");
            }
            let _ = body.remove_child(&ta);
        }
    }
}

// ── Feather/Lucide line icons (24×24, currentColor, 2px round strokes) — the same
// style as the bottom-nav icons, so the widgets stop looking like OS emoji. ──

const IC: &str = "http://www.w3.org/2000/svg";

/// Feather `user`.
fn icon_user() -> impl IntoView {
    view! {
        <svg xmlns=IC width="30" height="30" viewBox="0 0 24 24" fill="none"
            stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">
            <path d="M20 21v-2a4 4 0 0 0-4-4H8a4 4 0 0 0-4 4v2"/>
            <circle cx="12" cy="7" r="4"/>
        </svg>
    }
}

/// Feather `alert-triangle`, drawn orange — the background-errors tile.
fn icon_alert() -> impl IntoView {
    view! {
        <svg xmlns=IC width="28" height="28" viewBox="0 0 24 24" fill="none"
            stroke="#e8850d" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">
            <path d="M10.29 3.86 1.82 18a2 2 0 0 0 1.71 3h16.94a2 2 0 0 0 1.71-3L13.71 3.86a2 2 0 0 0-3.42 0z"/>
            <line x1="12" y1="9" x2="12" y2="13"/>
            <line x1="12" y1="17" x2="12.01" y2="17"/>
        </svg>
    }
}

/// Feather `mail` — the program-letters inbox tile.
fn icon_mail() -> impl IntoView {
    view! {
        <svg xmlns=IC width="28" height="28" viewBox="0 0 24 24" fill="none"
            stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">
            <rect x="2" y="4" width="20" height="16" rx="2"/>
            <path d="m22 7-8.97 5.7a1.94 1.94 0 0 1-2.06 0L2 7"/>
        </svg>
    }
}

/// Feather `bell`.
fn icon_bell() -> impl IntoView {
    view! {
        <svg xmlns=IC width="28" height="28" viewBox="0 0 24 24" fill="none"
            stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">
            <path d="M18 8A6 6 0 0 0 6 8c0 7-3 9-3 9h18s-3-2-3-9"/>
            <path d="M13.73 21a2 2 0 0 1-3.46 0"/>
        </svg>
    }
}

/// Feather `bell-off` (the crossed-out bell for the disabled state).
fn icon_bell_off() -> impl IntoView {
    view! {
        <svg xmlns=IC width="28" height="28" viewBox="0 0 24 24" fill="none"
            stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">
            <path d="M13.73 21a2 2 0 0 1-3.46 0"/>
            <path d="M18.63 13A17.89 17.89 0 0 1 18 8"/>
            <path d="M6.26 6.26A5.86 5.86 0 0 0 6 8c0 7-3 9-3 9h14"/>
            <path d="M18 8a6 6 0 0 0-9.33-5"/>
            <line x1="1" y1="1" x2="23" y2="23"/>
        </svg>
    }
}
