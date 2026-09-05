//! Program "letters" — in-app notifications the user reads at leisure. The first
//! (and currently only) producer is the WEEKLY calorie-planka recompute: one week
//! after the planka was set (and every week thereafter), the planka is recomputed
//! from the last 7 days and a letter announces the new value.
//!
//! Storage: a JSON blob in `app_flags` (per-user, per-device — NOT synced, like the
//! stories seen-set). A root signal drives the dashboard's mail widget reactively.

use std::cell::RefCell;

use leptos::{create_rw_signal, RwSignal, SignalUpdate};
use serde::{Deserialize, Serialize};

use crate::services::app_flags;

/// JSON array of [`Letter`] in `app_flags`.
const LETTERS_KEY: &str = "letters_v1";
/// Date (YYYY-MM-DD) of the last weekly planka recompute. Seeded from the calorie
/// goal's creation date so the first letter arrives one week after the planka was set.
const PLANKA_ANCHOR_KEY: &str = "planka_weekly_anchor";

/// Что письмо ПРЕДЛАГАЕТ сделать. Письма до сих пор были только текстом, и
/// этого хватало: они сообщали о случившемся. Отвязка от куратора — первый
/// случай, когда письмо зовёт к действию («планки пересчитаются через неделю —
/// или пересчитать сейчас»), и звать надо оттуда же, где человек об этом узнал.
#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub enum LetterAction {
    /// Пересчитать планки калорий и шагов немедленно, не дожидаясь недели.
    RecomputePlankas,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct Letter {
    pub id: String,
    /// ISO datetime the letter was created.
    pub created_at: String,
    /// Pre-rendered body (Russian). Kept as text so display needs no recomputation.
    pub body: String,
    #[serde(default)]
    pub read: bool,
    /// Кнопка под текстом. `None` — обычное письмо-сообщение. Старые письма в
    /// блобе `letters_v1` разбираются как прежде.
    #[serde(default)]
    pub action: Option<LetterAction>,
    /// Действие уже выполнено — кнопка больше не показывается. Нажать дважды
    /// нечего: пересчёт уже случился, и второй раз он лишь запутает.
    #[serde(default)]
    pub action_done: bool,
}

thread_local! {
    /// Bumped whenever a letter is added or read, so the mail widget re-renders.
    static VERSION: RefCell<Option<RwSignal<u32>>> = const { RefCell::new(None) };
}

/// Create the root reactivity signal. Call once, at root scope, from `main()`.
pub fn init() {
    VERSION.with(|c| *c.borrow_mut() = Some(create_rw_signal(0u32)));
}

/// The root signal the mail widget subscribes to. Bumps whenever letters change.
pub fn version_signal() -> RwSignal<u32> {
    VERSION.with(|c| c.borrow().expect("letters::init() must run first"))
}

/// Re-render the inbox after SYNC replaced the letters blob: [`all`] reads the
/// (already refreshed) app_flags cache, but nothing tells the widget to re-read.
pub fn refresh() {
    bump();
}

/// Перезапустить недельные часы ШАГОВ — тем же движением, каким
/// [`mark_planka_recomputed`] перезапускает их у калорий.
pub fn mark_steps_recomputed() {
    let today = chrono::Local::now().date_naive();
    app_flags::set(STEPS_ANCHOR_KEY, &today.format("%Y-%m-%d").to_string());
}

/// Сдвинуть ОБА недельных якоря на сегодня.
///
/// Нужно при отвязке от куратора: пока стоял его запрет, пересчёт выходил рано и
/// якорь не двигался. Через полгода кураторства «прошло 180 дней ≥ 7» сработало
/// бы на ближайшем запуске — то есть немедленно, вместо обещанной недели.
pub fn reset_weekly_anchors() {
    mark_planka_recomputed();
    mark_steps_recomputed();
}

/// Record that the calorie planka was (re)computed today — restarts the weekly
/// clock. Called from [`crate::services::local::set_calorie_goal`], so EVERY planka
/// change resets the anchor: the manual «Пересчитать» button (e.g. after a course-
/// goal change) AND the automatic weekly path (which also goes through set_calorie_goal).
pub fn mark_planka_recomputed() {
    let today = chrono::Local::now().date_naive();
    app_flags::set(PLANKA_ANCHOR_KEY, &today.format("%Y-%m-%d").to_string());
}

fn bump() {
    VERSION.with(|c| {
        if let Some(s) = *c.borrow() {
            s.update(|v| *v += 1);
        }
    });
}

/// All letters, newest first.
pub fn all() -> Vec<Letter> {
    let mut v: Vec<Letter> = app_flags::get(LETTERS_KEY)
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default();
    v.sort_by(|a, b| b.created_at.cmp(&a.created_at));
    v
}

fn save(list: &[Letter]) {
    let s = serde_json::to_string(list).expect("serialize letters");
    app_flags::set(LETTERS_KEY, &s);
    bump();
}

pub fn unread_count() -> usize {
    all().iter().filter(|l| !l.read).count()
}

pub fn has_unread() -> bool {
    unread_count() > 0
}

/// Append a letter (deduped by id) and persist.
pub fn add(letter: Letter) {
    let mut list = all();
    if list.iter().any(|l| l.id == letter.id) {
        return;
    }
    list.push(letter);
    save(&list);
}

/// Mark every letter read (called when the inbox is opened). Clears the red dot.
pub fn mark_all_read() {
    let mut list = all();
    let mut changed = false;
    for l in list.iter_mut() {
        if !l.read {
            l.read = true;
            changed = true;
        }
    }
    if changed {
        save(&list);
    }
}

/// Выполнить действие письма и отметить его сделанным.
///
/// Отметка обязательна: без неё кнопка осталась бы на месте, а второй пересчёт
/// в тот же день ничего не изменит и только собьёт человека с толку.
pub async fn run_action(letter_id: String) {
    let Some(action) = all().into_iter().find(|l| l.id == letter_id).and_then(|l| l.action) else {
        return;
    };
    match action {
        LetterAction::RecomputePlankas => {
            recompute_calorie_planka_now().await;
            recompute_steps_planka_now().await;
        }
    }
    let mut list = all();
    if let Some(l) = list.iter_mut().find(|l| l.id == letter_id) {
        l.action_done = true;
    }
    save(&list);
}

// ── Weekly calorie-planka recompute ──────────────────────────────────────────

/// One week after the planka was set (and weekly thereafter), recompute it and post
/// a letter. Safe to call on every launch/resume — it self-limits via the anchor.
pub async fn maybe_recompute_weekly_planka() {
    recompute_calorie_planka(false).await;
}

/// Пересчитать планку по калориям НЕМЕДЛЕННО, не дожидаясь недели.
///
/// Тот же путь, что у недельного пересчёта, — те же данные, то же письмо, тот же
/// сдвиг якоря. Отличие ровно одно: срок не проверяется.
pub async fn recompute_calorie_planka_now() {
    recompute_calorie_planka(true).await;
}

async fn recompute_calorie_planka(force: bool) {
    use crate::services::local;
    use crate::services::weight_trend;

    // Человека ведёт куратор — приложение планки не двигает. Одно условие на всё:
    // не «куратор запретил эту планку», а «за планки теперь отвечает он».
    //
    // Выходим ДО всего остального и НЕ двигаем якорь: двинуть его значило бы, что
    // после отвязки неделя пойдёт заново; человек и так ждал ровно столько,
    // сколько ждал. (Якоря сдвигает сама отвязка — см. `curator::unbind_locally`.)
    //
    // Отсюда следствие, которое стоит назвать вслух: недельные письма про новую
    // планку при кураторе НЕ ПРИХОДЯТ вовсе. Пересчёта нет — сообщать не о чем.
    // И планка, которую куратор не трогал, стоит на месте: за неё теперь он.
    if crate::services::support_chat::has_curator() {
        leptos::logging::log!("планка калорий: пересчёта нет — человека ведёт куратор");
        return;
    }
    // Адресата ещё не спрашивали — значит и про куратора мы ничего не знаем.
    // Считать в этот момент «куратора нет» нельзя: на свежем устройстве пересчёт
    // успевал отработать до первого опроса и двигал планку человеку, которого
    // ведёт куратор. Ждём ответа сервера: пересчёт недельный, одна отложенная
    // попытка ничего не стоит, а испорченная планка стоит недели.
    if !crate::services::support_chat::peer_known() {
        leptos::logging::log!("планка калорий: пересчёта нет — адресат ещё не известен");
        return;
    }

    // Планки ещё нет → пересчитывать нечего (гейт второй недели не сработал).
    //
    // Источник ОДИН — история. Раньше здесь спрашивались оба: и история, и запись
    // в `goals`, потому что у человека с давней планкой история могла быть пуста.
    // Теперь её заводит миграция (`m025_planka_from_goals`), и второй источник не
    // нужен: планка — это запись в истории, других мест у неё нет.
    let Some(previous) = local::calorie_goal_amount().await else {
        leptos::logging::log!("планка калорий: пересчёта нет — планка ещё не поставлена");
        return;
    };

    let today = chrono::Local::now().date_naive();
    // Якорь: последний пересчёт, иначе день, когда планка появилась впервые
    // (первый цикл = +7 дней). Первый день ищется в обоих источниках — по той же
    // причине, по какой в обоих ищется число.
    let first_planka_day = local::planka_history(local::PLANKA_CALORIES)
        .await
        .first()
        .and_then(|e| chrono::NaiveDate::parse_from_str(&e.date, "%Y-%m-%d").ok())
;
    let anchor = app_flags::get(PLANKA_ANCHOR_KEY)
        .and_then(|s| chrono::NaiveDate::parse_from_str(&s, "%Y-%m-%d").ok())
        .or(first_planka_day)
        .unwrap_or(today);

    let waited = (today - anchor).num_days();
    if !force && waited < 7 {
        leptos::logging::log!(
            "планка калорий: пересчёта нет — с {anchor} прошло {waited} дн., нужно 7"
        );
        return;
    }

    // ── На чём позволительно решать ──────────────────────────────────────────
    //
    // Обе стороны или ничего: и что происходит с весом, и исполнялась ли планка.
    // Не хватает любой — ОТКЛАДЫВАЕМ, и это принципиально не то же самое, что
    // «пропустить неделю»: якорь остаётся на месте, и пересчёт случится сам на
    // ближайшем запуске, как только данные появятся. Разбор каждого условия — в
    // доке к `plankas::check_evidence`.
    let entries = local::list_weight_entries().await;
    // Дни с записями — они же и есть среднее: одно чтение дневника на оба вопроса.
    let logged = local::daily_kcal_totals(7).await;
    if let Err(why) = local::check_evidence(today, &entries, Some(logged.len())) {
        // Срок вышел, а пересчёта не будет. Молчать об этом нельзя: снаружи это
        // выглядит как «планка просто не пересчиталась», и разобраться потом не по
        // чему. В журнал ошибок — чтобы человек увидел это в приложении.
        //
        // Текст БЕЗ меняющихся чисел: журнал дедуплицирует записи по совпадению
        // строки, и «прошло 9 дн.» внутри плодило бы по записи на каждый запуск.
        leptos::logging::log!("планка калорий: {why:?} (срок вышел {waited} дн. с {anchor})");
        super::errors::record_kind("planka.calories", "Планка по калориям", why.reason());
        return;
    }
    // Гейт выше уже потребовал `DIARY_MIN_DAYS` дней с записями, значит список не
    // пуст и деление безопасно.
    let avg = logged.iter().sum::<f64>() / logged.len() as f64;

    // Base the new planka on the PREVIOUS planka (`goal.amount`), nudged by AT MOST
    // ±5% from the weight trend — NOT on raw average intake. This keeps the planka
    // moving at most one small step per week and stops a low-intake week (e.g.
    // anxiety undereating) from ratcheting the target downward: eating under the
    // planka no longer drags the base down, only a confirmed weight trend moves it.
    // Average intake seeds only the FIRST planka (`calorie_planka_suggestion`).
    // Отталкиваемся от ДЕЙСТВУЮЩЕЙ планки, а не от записи в goals: планка живёт в
    // истории, и следующий шаг обязан идти от того числа, что действует сейчас, —
    // хоть от нашего прошлого, хоть от кураторского, оставшегося после отвязки.
    let weight_kg = entries
        .iter()
        .max_by(|a, b| a.date.cmp(&b.date))
        .map(|e| e.weight_kg)
        .unwrap_or(0.0);
    // Планка судится по 28 дням, а не по 14, которые показывает виджет веса: на
    // четырнадцати наклон гуляет вместе с водой, и правило разворачивает планку
    // каждую вторую неделю. Разбор с замерами — в доке к `DECISION_WINDOW_DAYS`.
    let trend = weight_trend::weight_trend(&entries, local::DECISION_WINDOW_DAYS);
    // Порог тот же, по которому день считается зелёным: держаться планки и значит
    // попадать в этот коридор.
    let adherence =
        local::adherence(avg, previous, crate::services::indicators::CALORIE_BAND_KCAL);
    // Куда звал бы вес, если бы исполнение не держало планку. Нужно письму: без
    // этого «планка не изменилась» не отличить от «мы придержали её намеренно».
    let goal = crate::services::profile::planka_goal();
    let wanted = local::calorie_planka(previous, &trend, weight_kg, goal);
    let new_planka = local::calorie_planka_weekly(previous, &trend, weight_kg, adherence, goal);
    leptos::logging::log!(
        "планка калорий: съедено в среднем {avg:.0} за {} дн. при планке {previous:.0} → \
         {adherence:?}; новая {new_planka:.0}",
        logged.len()
    );

    // Apply the new planka (syncs like any goal edit). `set_calorie_goal` also
    // advances the weekly anchor to today via `mark_planka_recomputed`, so the next
    // letter is a full week out — no separate anchor write needed here.
    local::set_calorie_goal(new_planka).await;
    crate::services::sync::push_background();

    // Post the letter announcing the new planka.
    add(Letter {
        id: format!("planka-{}", today.format("%Y-%m-%d")),
        created_at: chrono::Local::now().to_rfc3339(),
        body: planka_letter_body(&trend, weight_kg, previous, new_planka, wanted, adherence),
        read: false,
        action: None,
        action_done: false,
    });
}

// ── Weekly steps-planka step-up ──────────────────────────────────────────────

/// Date (YYYY-MM-DD) of the last weekly STEPS-planka recompute. Seeded from the
/// date the activity week opened, so the FIRST step-up lands one week after the
/// steps planka was set.
const STEPS_ANCHOR_KEY: &str = "steps_planka_weekly_anchor";

/// One week after the steps planka was set (and weekly thereafter), raise it by
/// the step indicator's own colour and post a letter. Safe to call on every
/// launch/resume — it self-limits via the anchor.
pub async fn maybe_recompute_weekly_steps_planka() {
    recompute_steps_planka(false).await;
}

/// Пересчитать планку по шагам НЕМЕДЛЕННО — см. [`recompute_calorie_planka_now`].
pub async fn recompute_steps_planka_now() {
    recompute_steps_planka(true).await;
}

async fn recompute_steps_planka(force: bool) {
    use crate::services::indicators::{self, IndicatorState};
    use crate::services::{local, profile};

    // Куратор ведёт человека — приложение не двигает и эту планку; якорь стоит.
    // См. подробности в пересчёте калорий выше.
    if crate::services::support_chat::has_curator() {
        leptos::logging::log!("планка шагов: пересчёта нет — человека ведёт куратор");
        return;
    }
    // См. пересчёт калорий: пока адресат неизвестен, о кураторе судить не по чему.
    if !crate::services::support_chat::peer_known() {
        leptos::logging::log!("планка шагов: пересчёта нет — адресат ещё не известен");
        return;
    }

    // No planka yet → the activity week hasn't opened; nothing to raise.
    // get_steps_planka возвращает ДЕЙСТВУЮЩУЮ планку из истории — ступенька идёт
    // от неё.
    let Some(current) = profile::get_steps_planka() else {
        leptos::logging::log!("планка шагов: пересчёта нет — планка ещё не поставлена");
        return;
    };
    let current = current.round() as u32;

    let today = chrono::Local::now().date_naive();
    let anchor = app_flags::get(STEPS_ANCHOR_KEY)
        .or_else(|| app_flags::get(indicators::STEPS_GATE_OPEN_KEY))
        .and_then(|s| chrono::NaiveDate::parse_from_str(&s, "%Y-%m-%d").ok())
        .unwrap_or(today);
    let waited = (today - anchor).num_days();
    if !force && waited < 7 {
        leptos::logging::log!(
            "планка шагов: пересчёта нет — с {anchor} прошло {waited} дн., нужно 7"
        );
        return;
    }

    // The signal is the step indicator over the last 7 COMPLETED days — exactly
    // what the widget shows the user.
    let state = indicators::indicator_state("steps").await;
    if state == IndicatorState::Unknown {
        // Not judgeable (no step data in the window) — defer WITHOUT advancing the
        // anchor, so the next launch tries again instead of silently skipping a week.
        // Видимо в журнале ошибок: срок вышел, а планка не двинулась.
        super::errors::record_kind(
            "planka.steps",
            "Планка по шагам",
            &format!(
                "пересчёт отложен: срок вышел ({waited} дн. с {anchor}), но за последние 7 \
                 завершённых дней нет данных о шагах"
            ),
        );
        return;
    }

    let next = local::next_steps_planka(current, state);

    // The week was assessed — restart the clock even when the planka HOLDS (red
    // week / already at the ceiling), otherwise every launch would re-assess and
    // a mid-week colour change would raise the planka off-schedule.
    app_flags::set(STEPS_ANCHOR_KEY, &today.format("%Y-%m-%d").to_string());
    if next == current {
        return;
    }

    profile::set_steps_planka(next as f64);
    crate::services::sync::push_background();

    // Цифры для письма: сколько пройдено всего и как изменилась активность
    // против ПЕРВЫХ двух недель наблюдения.
    let total = local::total_steps().await;
    let baseline = local::avg_steps_first_days(14).await;
    let recent = local::avg_steps_last_days(7).await;

    add(Letter {
        id: format!("steps-planka-{}", today.format("%Y-%m-%d")),
        created_at: chrono::Local::now().to_rfc3339(),
        body: steps_letter_body(next, total, baseline, recent),
        read: false,
        action: None,
        action_done: false,
    });
}

/// Текст недельного письма о планке по шагам.
///
/// Строка о динамике появляется ТОЛЬКО когда есть обе величины — базовая линия
/// (первые 14 записанных дней) и среднее за последнюю неделю. Нет одной из них —
/// сравнивать не с чем, и выдумывать сравнение нельзя.
fn steps_letter_body(
    planka: u32,
    total: u64,
    baseline: Option<u32>,
    recent: Option<u32>,
) -> String {
    let mut out = String::from("Недельное обновление планки по шагам.\n\n");
    out.push_str(&format!(
        "Новая планка: {} {}.\n",
        fmt_thousands(planka),
        plural_steps(planka)
    ));
    out.push_str(&format!(
        "За время использования вы прошли {} {}.\n",
        fmt_thousands_u64(total),
        plural_steps_u64(total)
    ));
    if let (Some(was), Some(now)) = (baseline, recent) {
        let dir = if now >= was { "выше" } else { "ниже" };
        out.push_str(&format!(
            "Ваша активность стала {dir}: была {}, а стала {}.\n",
            fmt_thousands(was),
            fmt_thousands(now)
        ));
    }
    out.push_str("\nВскоре это отразится на вашем весе и на вашей планке по калориям.");
    out
}

/// Текст недельного письма о планке по калориям.
///
/// Сначала — что происходит с весом: направление (стоит / снижается / растёт),
/// насколько мы в этом уверены и, для уверенного снижения, темп относительно
/// комфортной полосы. Потом — куда двинулась планка и почему. И, в зависимости
/// от направления сдвига, короткая подсказка, чем этот сдвиг закрывать.
fn planka_letter_body(
    trend: &crate::services::weight_trend::WeightTrend,
    weight_kg: f64,
    old_planka: f64,
    planka: f64,
    // Куда звал расчёт по весу ДО того, как исполнение придержало планку.
    wanted: f64,
    adherence: crate::services::local::Adherence,
) -> String {
    let mut out = String::from("Недельное обновление планки\n\n");
    out.push_str(&weight_verdict(trend, weight_kg));
    out.push_str("\n\n");

    let planka_i = planka as i64;
    if planka > old_planka + 0.5 {
        out.push_str(&format!(
            "А поэтому вам необходимо начать питаться чуть более калорийно. \
             На ближайшую неделю ваша планка {planka_i} калорий.\n\n\
             Если не знаете, чем заполнить внезапное увеличение, попробуйте небольшие \
             конфетки, орешки."
        ));
    } else if planka < old_planka - 0.5 {
        out.push_str(&format!(
            "А поэтому вам необходимо начать питаться чуть менее калорийно. \
             На ближайшую неделю ваша планка {planka_i} калорий.\n\n\
             Напоминаем, что необходимо использовать еду с низкой калорийной плотностью. \
             Больше белка, больше растительности, меньше ультрапереработанных продуктов."
        ));
    } else {
        // Планка не сдвинулась. Причин ДВЕ, и человеку они видятся по-разному: либо
        // всё идёт как надо, либо расчёт звал её сдвинуть, а мы придержали, потому
        // что прошлую неделю человек планку не выполнял. Второе надо назвать прямо —
        // иначе письмо читается как «ничего не произошло».
        use crate::services::local::Adherence;
        let held_up = wanted > old_planka + 0.5;
        let held_down = wanted < old_planka - 0.5;
        match adherence {
            Adherence::Under if held_up => out.push_str(&format!(
                "По всем расчётам выходит, что вам необходимо поднять планку. Однако из-за \
                 того, что вы недоедали, мы этого сделать не можем. Необходимо чётко \
                 следовать вашей планке.\n\n\
                 На ближайшую неделю она остаётся прежней — {planka_i} калорий."
            )),
            Adherence::Over if held_down => out.push_str(&format!(
                "По всем расчётам выходит, что вам необходимо опустить планку. Однако из-за \
                 того, что вы переедали, мы этого сделать не можем. Необходимо чётко \
                 следовать вашей планке.\n\n\
                 На ближайшую неделю она остаётся прежней — {planka_i} калорий."
            )),
            _ => out.push_str(&format!(
                "А поэтому менять питание не нужно. На ближайшую неделю ваша планка \
                 остаётся прежней — {planka_i} калорий."
            )),
        }
    }
    out
}

/// Что происходит с весом — с НАЗВАННЫМ сроком, направлением и положением
/// относительно целевой полосы.
///
/// Срок называется вслух и берётся из константы. Панель веса показывает тренд за
/// 14 дней, планка судится по 28 — на одних и тех же данных это разные числа
/// (у живого пользователя −0.99 и −0.58 кг/нед в один день), и рядом они читаются
/// как противоречие, пока не сказано, что они про разные сроки.
///
/// Положение относительно полосы берётся у `local::pace` — у ТОЙ ЖЕ функции, что
/// двигает планку. Своя копия правила здесь однажды уже была, и после перехода на
/// значимость она сделала бы письмо самопротиворечивым: «снижается слишком
/// быстро» рядом с «менять питание не нужно».
fn weight_verdict(t: &crate::services::weight_trend::WeightTrend, weight_kg: f64) -> String {
    use crate::services::local::Pace;
    use crate::services::profile::planka_goal;
    use crate::services::weight_trend::{Direction, WeightTrend, CONFIDENT, WEAK};
    use plankas::Goal;

    let goal = planka_goal();
    let window = window_phrase();
    // Где вес относительно ЦЕЛЕВОЙ полосы — и что это значит при этой цели.
    // «Ниже полосы» у похудения и у набора — совсем разные новости, поэтому текст
    // выбирается парой, а не одним только положением.
    let band_clause = || -> Option<&'static str> {
        match (goal, crate::services::local::pace(t, weight_kg, goal)?) {
            (Goal::Lose, Pace::BelowBand) => Some("и делает это быстрее комфортного"),
            (Goal::Lose, Pace::AboveBand) => Some("но медленнее, чем нужно"),
            (Goal::Lose, Pace::InBand) => Some(in_band_clause(t, weight_kg, "и делает это в комфортном темпе")),
            (Goal::Maintain, Pace::BelowBand) => Some("и уходит вниз заметнее, чем нужно для удержания"),
            (Goal::Maintain, Pace::AboveBand) => Some("и уходит вверх заметнее, чем нужно для удержания"),
            (Goal::Maintain, Pace::InBand) => Some(in_band_clause(t, weight_kg, "и держится там, где надо")),
            (Goal::Gain, Pace::BelowBand) => Some("а набора пока не видно"),
            // Верхней границы у набора нет: слишком быстрый набор — не повод для
            // правки, и говорить о нём как о проблеме нельзя.
            (Goal::Gain, _) => Some(in_band_clause(t, weight_kg, "и набор идёт")),
        }
    };
    match *t {
        WeightTrend::Insufficient { .. } => format!(
            "{window} взвешиваний слишком мало, чтобы понять, что происходит с вашим весом."
        ),
        WeightTrend::Tentative { direction, .. } => {
            let d = match direction {
                Direction::Down => "снижается",
                Direction::Up => "растёт",
            };
            format!("{window} ваш вес, кажется, {d}, но данных пока мало, чтобы утверждать это уверенно.")
        }
        WeightTrend::Estimated { direction, confidence, .. } => {
            if confidence >= CONFIDENT {
                let d = match direction {
                    Direction::Down => "уверенно снижается",
                    Direction::Up => "уверенно растёт",
                };
                match band_clause() {
                    Some(c) => format!("{window} ваш вес {d} — {c}."),
                    None => format!("{window} ваш вес {d}."),
                }
            } else if confidence >= WEAK {
                let d = match direction {
                    Direction::Down => "снижается",
                    Direction::Up => "растёт",
                };
                format!("{window} ваш вес, кажется, {d}, но пока неуверенно.")
            } else {
                format!("{window} ваш вес стоит на месте.")
            }
        }
    }
}

/// «Не отличается от полосы» — это ДВА разных сообщения, и путать их нельзя:
/// темп внутри полосы мы утверждаем, темп снаружи, но неотличимый от неё, —
/// называем тем, чем он является, погрешностью.
fn in_band_clause(
    t: &crate::services::weight_trend::WeightTrend,
    weight_kg: f64,
    inside: &'static str,
) -> &'static str {
    use crate::services::weight_trend::WeightTrend;
    let WeightTrend::Estimated { slope_kg_per_week, .. } = *t else { return inside };
    let goal = crate::services::profile::planka_goal();
    let Some(band) = crate::services::local::target_band(goal, weight_kg) else { return inside };
    let below = band.lo.is_some_and(|lo| slope_kg_per_week < lo);
    let above = band.hi.is_some_and(|hi| slope_kg_per_week > hi);
    if below || above {
        "и пока это в пределах погрешности взвешиваний"
    } else {
        inside
    }
}

/// «За последние 28 дней» — срок, по которому судится планка, словами и из
/// константы, чтобы текст не разошёлся с расчётом.
fn window_phrase() -> String {
    let n = crate::services::local::DECISION_WINDOW_DAYS;
    format!("За последние {n} {}", plural_days(n))
}

/// Русское склонение «день/дня/дней».
fn plural_days(n: i64) -> &'static str {
    let n100 = n % 100;
    let n10 = n % 10;
    if (11..=14).contains(&n100) {
        "дней"
    } else if n10 == 1 {
        "день"
    } else if (2..=4).contains(&n10) {
        "дня"
    } else {
        "дней"
    }
}

/// Group thousands with a thin space: 8200 → "8 200".
/// Как [`fmt_thousands`], но для больших сумм (все шаги за всё время).
fn fmt_thousands_u64(n: u64) -> String {
    let s = n.to_string();
    let bytes = s.as_bytes();
    let mut out = String::new();
    for (i, b) in bytes.iter().enumerate() {
        if i > 0 && (bytes.len() - i) % 3 == 0 {
            out.push('\u{202f}'); // narrow no-break space
        }
        out.push(*b as char);
    }
    out
}

/// Русское склонение «шаг/шага/шагов» для большого числа: правило смотрит только
/// на две последние цифры.
fn plural_steps_u64(n: u64) -> &'static str {
    plural_steps((n % 100) as u32)
}

fn fmt_thousands(n: u32) -> String {
    let s = n.to_string();
    let bytes = s.as_bytes();
    let mut out = String::new();
    for (i, b) in bytes.iter().enumerate() {
        if i > 0 && (bytes.len() - i) % 3 == 0 {
            out.push('\u{202f}'); // narrow no-break space
        }
        out.push(*b as char);
    }
    out
}

/// Russian plural for «шаг»: 1 шаг / 2 шага / 5 шагов.
fn plural_steps(n: u32) -> &'static str {
    let n100 = n % 100;
    let n10 = n % 10;
    if (11..=14).contains(&n100) {
        "шагов"
    } else if n10 == 1 {
        "шаг"
    } else if (2..=4).contains(&n10) {
        "шага"
    } else {
        "шагов"
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn thousands_grouping() {
        assert_eq!(fmt_thousands(0), "0");
        assert_eq!(fmt_thousands(200), "200");
        assert_eq!(fmt_thousands(8200), "8\u{202f}200");
        assert_eq!(fmt_thousands(12345), "12\u{202f}345");
    }

    use crate::services::local::Adherence;
    use crate::services::weight_trend::{Direction, WeightTrend};

    /// Оценка тренда с ЯВНОЙ погрешностью: от неё зависит и вердикт письма, и
    /// шаг планки. По умолчанию берём узкую — тесты про тексты, а не про шум.
    fn est(dir: Direction, slope_wk: f64, conf: f64) -> WeightTrend {
        est_se(dir, slope_wk, conf, 0.03)
    }

    fn est_se(dir: Direction, slope_wk: f64, conf: f64, se_wk: f64) -> WeightTrend {
        WeightTrend::Estimated {
            direction: dir,
            slope_kg_per_week: slope_wk,
            confidence: conf,
            slope_se_kg_per_week: se_wk,
            days: 14,
        }
    }


    /// Вердикт письма — при цели по умолчанию (похудение; профиль в тестах пуст).
    /// 90 кг: полоса 0.27…0.63 кг/нед.
    #[test]
    fn calorie_letter_states_the_weight_verdict() {
        let fast = planka_letter_body(&est(Direction::Down, -1.2, 0.99), 90.0, 2500.0, 2650.0, 2650.0, Adherence::OnTarget);
        assert!(fast.starts_with("Недельное обновление планки"), "{fast}");
        assert!(fast.contains("уверенно снижается — и делает это быстрее комфортного"), "{fast}");

        let slow = planka_letter_body(&est(Direction::Down, -0.1, 0.99), 90.0, 2500.0, 2400.0, 2400.0, Adherence::OnTarget);
        assert!(slow.contains("уверенно снижается — но медленнее, чем нужно"), "{slow}");

        let comfy = planka_letter_body(&est(Direction::Down, -0.5, 0.99), 90.0, 2500.0, 2500.0, 2500.0, Adherence::OnTarget);
        assert!(comfy.contains("в комфортном темпе"), "{comfy}");

        let flat = planka_letter_body(&est(Direction::Down, -0.05, 0.5), 90.0, 2500.0, 2400.0, 2400.0, Adherence::OnTarget);
        assert!(flat.contains("ваш вес стоит на месте."), "{flat}");

        let unsure = planka_letter_body(&est(Direction::Down, -0.3, 0.7), 90.0, 2500.0, 2500.0, 2500.0, Adherence::OnTarget);
        assert!(unsure.contains("ваш вес, кажется, снижается, но пока неуверенно."), "{unsure}");

        let few = planka_letter_body(&WeightTrend::Insufficient { days: 1 }, 90.0, 2500.0, 2500.0, 2500.0, Adherence::OnTarget);
        assert!(few.contains("взвешиваний слишком мало"), "{few}");
    }

    /// СРОК НАЗЫВАЕТСЯ. Панель веса судит по 14 дням, планка — по 28, и на одних
    /// и тех же данных это разные числа. Пока срок не назван, они читаются как
    /// противоречие. Число берётся из константы — разойтись с расчётом не может.
    #[test]
    fn calorie_letter_names_its_window() {
        let n = crate::services::local::DECISION_WINDOW_DAYS;
        for t in [
            est(Direction::Down, -1.2, 0.99),
            est(Direction::Down, -0.05, 0.5),
            est(Direction::Down, -0.3, 0.7),
            WeightTrend::Insufficient { days: 1 },
            WeightTrend::Tentative { direction: Direction::Down, slope_kg_per_week: -0.5, days: 2 },
        ] {
            let body = planka_letter_body(&t, 90.0, 2500.0, 2500.0, 2500.0, Adherence::OnTarget);
            assert!(body.contains(&format!("За последние {n} дней")), "{body}");
        }
    }

    /// Склонение срока — на случай, если окно однажды станет не 28.
    #[test]
    fn sklonenie_sroka() {
        assert_eq!(plural_days(1), "день");
        assert_eq!(plural_days(3), "дня");
        assert_eq!(plural_days(5), "дней");
        assert_eq!(plural_days(11), "дней");
        assert_eq!(plural_days(14), "дней");
        assert_eq!(plural_days(21), "день");
        assert_eq!(plural_days(28), "дней");
    }

    #[test]
    fn calorie_letter_advice_follows_the_direction() {
        // Планка выросла — зовём есть БОЛЬШЕ и подсказываем, чем добрать.
        let up = planka_letter_body(&est(Direction::Down, -1.2, 0.99), 90.0, 2500.0, 2650.0, 2650.0, Adherence::OnTarget);
        assert!(up.contains("чуть более калорийно"), "{up}");
        assert!(up.contains("ваша планка 2650 калорий"), "{up}");
        assert!(up.contains("конфетки, орешки"), "{up}");
        assert!(!up.contains("калорийной плотностью"), "{up}");

        // Планка упала — зовём есть МЕНЬШЕ и напоминаем про плотность.
        let down = planka_letter_body(&est(Direction::Up, 0.4, 0.99), 90.0, 2500.0, 2400.0, 2400.0, Adherence::OnTarget);
        assert!(down.contains("чуть менее калорийно"), "{down}");
        assert!(down.contains("ваша планка 2400 калорий"), "{down}");
        assert!(down.contains("низкой калорийной плотностью"), "{down}");
        assert!(!down.contains("конфетки"), "{down}");

        // Планка не изменилась — не зовём ни туда, ни сюда.
        let hold = planka_letter_body(&est(Direction::Down, -0.5, 0.99), 90.0, 2500.0, 2500.0, 2500.0, Adherence::OnTarget);
        assert!(hold.contains("менять питание не нужно"), "{hold}");
        assert!(hold.contains("остаётся прежней — 2500 калорий"), "{hold}");
        assert!(!hold.contains("калорийно."), "{hold}");
    }

    /// Письмо не имеет права противоречить самому себе. Темп берётся у той же
    /// функции, что двигает планку, — значит «слишком быстро» и «менять питание
    /// не нужно» в одном письме встретиться не могут.
    #[test]
    fn calorie_letter_does_not_contradict_itself() {
        // −0.75 кг/нед при 85 кг выше комфортной полосы ТОЧЕЧНО, но погрешность
        // 0.19 не даёт этого утверждать. Планка стоит — и письмо это признаёт.
        let noisy = est_se(Direction::Down, -0.75, 0.99, 0.19);
        let body = planka_letter_body(&noisy, 85.0, 2800.0, 2800.0, 2800.0, Adherence::OnTarget);
        assert!(body.contains("в пределах погрешности взвешиваний"), "{body}");
        assert!(!body.contains("быстрее комфортного"), "{body}");
        assert!(body.contains("остаётся прежней — 2800 калорий"), "{body}");
    }

    #[test]
    fn steps_letter_omits_the_comparison_without_a_baseline() {
        // Есть обе величины — строка про динамику появляется.
        let both = steps_letter_body(11_000, 254_300, Some(7_200), Some(9_800));
        assert!(both.starts_with("Недельное обновление планки по шагам."), "{both}");
        assert!(both.contains("Новая планка: 11\u{202f}000 шагов."), "{both}");
        assert!(both.contains("вы прошли 254\u{202f}300 шагов."), "{both}");
        assert!(both.contains("стала выше: была 7\u{202f}200, а стала 9\u{202f}800."), "{both}");
        assert!(both.ends_with("Вскоре это отразится на вашем весе и на вашей планке по калориям."), "{both}");

        // Активность упала — «ниже».
        let worse = steps_letter_body(11_000, 254_300, Some(9_800), Some(7_200));
        assert!(worse.contains("стала ниже: была 9\u{202f}800, а стала 7\u{202f}200."), "{worse}");

        // Базовой линии нет — сравнивать не с чем, строки быть не должно.
        let none = steps_letter_body(11_000, 254_300, None, Some(9_800));
        assert!(!none.contains("Ваша активность"), "{none}");
        assert!(none.contains("вы прошли 254\u{202f}300 шагов."), "{none}");
    }

    #[test]
    fn steps_plural() {
        assert_eq!(plural_steps(1), "шаг");
        assert_eq!(plural_steps(2), "шага");
        assert_eq!(plural_steps(4), "шага");
        assert_eq!(plural_steps(5), "шагов");
        assert_eq!(plural_steps(11), "шагов");
        assert_eq!(plural_steps(21), "шаг");
        assert_eq!(plural_steps(8200), "шагов");
    }
}
