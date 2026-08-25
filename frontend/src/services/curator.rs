//! Связь худеющего с куратором: приглашение, согласие, отвязка.
//!
//! Транспорт — тот же, что у чата (support-worker, тот же токен), поэтому и
//! используется его: заводить второй ради трёх запросов незачем.
//!
//! FAIL LOUDLY: каждый путь возвращает `Result<_, String>`; вызывающая сторона
//! показывает ошибку человеку, а не делает вид, что ничего не случилось.

use serde::Deserialize;

use crate::services::support_chat::{get_json, post_json};

/// Что показать на экране согласия.
#[derive(Debug, Clone, Deserialize)]
pub struct InvitePeek {
    /// Приглашение существует и ещё не погашено.
    #[serde(default)]
    pub found: bool,
    /// Приглашением уже воспользовались.
    #[serde(default)]
    pub used: bool,
    #[serde(default)]
    pub curator_name: String,
    /// Человек уже у другого куратора — согласие оборвёт прежнюю связь, и
    /// сказать об этом надо ДО, а не после.
    #[serde(default)]
    pub current_curator_id: Option<String>,
}

/// Кто курирует этого человека сейчас.
#[derive(Debug, Clone, Deserialize)]
pub struct Binding {
    #[serde(default)]
    pub bound: bool,
    #[serde(default)]
    pub curator_name: String,
}

pub async fn peek(code: &str) -> Result<InvitePeek, String> {
    get_json(&format!("/curator/invite/{code}")).await
}

pub async fn accept(code: &str) -> Result<serde_json::Value, String> {
    post_json(&format!("/curator/invite/{code}/accept"), "{}").await
}

pub async fn binding() -> Result<Binding, String> {
    get_json("/curator/binding").await
}

/// Отвязаться от куратора по своей воле.
pub async fn unbind() -> Result<serde_json::Value, String> {
    post_json("/curator/unbind", "{}").await
}

/// Забыть куратора на этом устройстве после успешной отвязки на сервере.
///
/// Сама уборка живёт в [`unbind_locally`] и запускается по СМЕНЕ адресата в
/// опросе — одним путём и для «куратор прекратил работу», и для «человек
/// отвязался сам». Здесь достаточно опросить сервер: он и сообщит, что адресат
/// сменился.
pub async fn forget_locally() {
    if let Err(e) = crate::services::support_chat::poll().await {
        leptos::logging::error!("отвязка: опрос не удался: {e}");
    }
}

/// Уборка после отвязки от куратора — на стороне приложения.
///
/// Три вещи, и порядок между ними существенный:
///
/// 1. Кураторские значения калорий и шагов ПЕРЕНОСЯТСЯ в наши места. Обещано,
///    что планки держатся до ближайшего пересчёта: просто стереть запись значило
///    бы уронить человека на число полугодовой давности.
/// 2. Всё остальное кураторское стирается — возвращаются наши правила.
/// 3. Оба недельных якоря сдвигаются на сегодня: неделя считается от отвязки, а
///    не от того дня, когда пересчёт последний раз проходил.
///
/// Перенос идёт ПОСЛЕ стирания, иначе норма белка пересчиталась бы от ещё
/// живого кураторского числа, а не от того, что осталось.
pub async fn unbind_locally() {
    use crate::services::{curator_plankas, letters, local, profile, sync};

    let calories = curator_plankas::get("calories");
    let steps = curator_plankas::get("steps");

    curator_plankas::clear_all().await;

    if let Some(kcal) = calories {
        local::set_calorie_goal(kcal).await;
    }
    if let Some(st) = steps {
        profile::set_steps_planka(st);
    }
    letters::reset_weekly_anchors();

    // Письмо — и с предложением не ждать неделю. Идентификатор по дню: две
    // отвязки в один день письма не удвоят.
    let today = chrono::Local::now().format("%Y-%m-%d").to_string();
    letters::add(letters::Letter {
        id: format!("curator-unbound-{today}"),
        created_at: chrono::Local::now().to_rfc3339(),
        body: crate::services::i18n::t("curator.letter.unbound").to_string(),
        read: false,
        action: Some(letters::LetterAction::RecomputePlankas),
        action_done: false,
    });
    sync::push_background();
}
