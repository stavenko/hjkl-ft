//! След попыток разобрать продукт: что спрашивали, когда и чем кончилось.
//!
//! Зачем. Очередь проходит по всей базе продуктов при каждом запуске приложения, и
//! до сих пор она переспрашивала всё, чего не хватает, — каждый раз. Для продукта,
//! который модель разобрать не может, это бесконечный круг: те же запросы, тот же
//! отказ, и так при каждом открытии приложения. Здесь хранится время последней
//! попытки, и повтор допускается не раньше чем через сутки.
//!
//! Записи локальные и НЕ синкаются: это след разговора с моделью на этом
//! устройстве, а не данные человека. На другом устройстве свои попытки — и пусть:
//! продукт там могли и не спрашивать.
//!
//! Ключ — «id продукта : что спрашивали». Что спрашивали — это либо `identity`
//! (опознание, оно решает судьбу всего прохода), либо имя признака.

use serde::{Deserialize, Serialize};

use super::db;

/// Сколько ждать после неудачной попытки, прежде чем спрашивать снова.
///
/// Сутки — потому что за сутки меняется то, что может изменить исход: выходит новая
/// сборка со справочником или словарём, правится промпт. Чаще — бессмысленно: модель
/// та же и ответит то же.
const RETRY_AFTER_MS: f64 = 24.0 * 60.0 * 60.0 * 1000.0;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Probe {
    /// «<id продукта>:<что спрашивали>».
    pub key: String,
    pub food_id: String,
    /// `identity` или имя признака.
    pub aspect: String,
    /// Когда спрашивали в последний раз, мс эпохи.
    pub attempted_at: f64,
    /// Чем кончилось: false — не получили ответа, которому верим.
    pub ok: bool,
    /// Коротко: что именно не вышло. Для журнала и разбора.
    #[serde(default)]
    pub note: String,
}

fn key(food_id: &str, aspect: &str) -> String {
    format!("{food_id}:{aspect}")
}

fn now_ms() -> f64 {
    js_sys::Date::now()
}

/// Записать попытку. Удачную тоже: по ней видно, что продукт разбирался, и когда.
pub async fn record(food_id: &str, aspect: &str, ok: bool, note: &str) {
    let probe = Probe {
        key: key(food_id, aspect),
        food_id: food_id.to_string(),
        aspect: aspect.to_string(),
        attempted_at: now_ms(),
        ok,
        note: note.chars().take(200).collect(),
    };
    db::put("food_probe", &probe).await;
}

/// Прошло ли достаточно времени, чтобы спрашивать это снова.
///
/// Нет записи — спрашивать можно: продукт новый. Последняя попытка удалась — тоже
/// можно: раз спрашиваем опять, значит поля не хватает, и дело не в отказе модели.
pub async fn may_ask(food_id: &str, aspect: &str) -> bool {
    let Some(p) = db::get::<Probe>("food_probe", &key(food_id, aspect)).await else {
        return true;
    };
    if p.ok {
        return true;
    }
    now_ms() - p.attempted_at >= RETRY_AFTER_MS
}

/// Продукты, которые не удалось ОПОЗНАТЬ, — то есть подвисшие целиком.
/// Нужны, чтобы показать человеку список и понять, чего просит словарь.
pub async fn stuck() -> Vec<Probe> {
    db::list_all::<Probe>("food_probe")
        .await
        .into_iter()
        .filter(|p| p.aspect == super::flags_pipeline::ASPECT_IDENTITY && !p.ok)
        .collect()
}

/// Забыть попытки по продукту — например, когда его переименовали: имя другое,
/// значит и разговор с моделью начинается заново.
pub async fn forget(food_id: &str) {
    for p in db::list_all::<Probe>("food_probe").await {
        if p.food_id == food_id {
            db::delete("food_probe", &p.key).await;
        }
    }
}
