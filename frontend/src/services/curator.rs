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
/// Три вещи, и различие между первыми двумя — суть всей механики:
///
/// 1. Девять ПОСТОЯННЫХ планок забываются: кальций, овощи-фрукты, клетчатка,
///    железо, гем, омега-3, баланс жиров, красное мясо, яйца. Стирается запись, а
///    не пишется наше число, — и это принципиально. Клетчатка обязана следовать
///    за калорийной планкой, железо — за профилем; записать число значило бы
///    заморозить их на сегодняшнем значении навсегда.
///
///    Различать авторство при этом не нужно: для этих девяти запись в истории
///    может появиться ТОЛЬКО от куратора — приложение их никогда не пишет.
///
/// 2. Калории, шаги и белок остаются кураторскими. Обещано, что планки держатся
///    до ближайшего пересчёта: их ведёт недельный цикл, он и заберёт их обратно —
///    первым же своим проходом, теперь снова разрешённым.
///
/// 3. Оба недельных якоря сдвигаются на сегодня: неделя считается от отвязки, а
///    не от того дня, когда пересчёт последний раз проходил.
///
/// Переход СРАЗУ к другому куратору — тоже отвязка, и постоянные планки прежнего
/// в ней тоже забываются: новый куратор назовёт свои. Но письма «приложение снова
/// ведёт планки само» в этом случае не будет — оно было бы неправдой, — и якоря
/// двигать незачем: пересчёт по-прежнему выключен.
pub async fn unbind_locally() {
    use crate::services::{directives, i18n, letters, plankas, support_chat, sync};

    for kind in plankas::ALL.iter().filter(|k| !k.is_dynamic()) {
        plankas::forget(*kind).await;
    }
    if support_chat::has_curator() {
        sync::push_background();
        return;
    }
    letters::reset_weekly_anchors();

    // Письмо — и с предложением не ждать неделю. Идентификатор по дню: две
    // отвязки в один день письма не удвоят.
    //
    // Перечень планок собирается ПОСЛЕ забывания: человек должен увидеть то, что
    // соблюдать теперь, а не то, что действовало минуту назад.
    let today = chrono::Local::now().format("%Y-%m-%d").to_string();
    let body = format!(
        "{}\n\n{}\n{}",
        i18n::t("curator.letter.unbound"),
        i18n::t("curator.letter.unbound_list"),
        directives::planka_list()
    );
    letters::add(letters::Letter {
        id: format!("curator-unbound-{today}"),
        created_at: chrono::Local::now().to_rfc3339(),
        body,
        read: false,
        action: Some(letters::LetterAction::RecomputePlankas),
        action_done: false,
    });
    sync::push_background();
}
