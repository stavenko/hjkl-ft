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
//!
//! Здесь же живут ещё две вещи, выросшие из того же следа.
//!
//! КЭШ ОПОЗНАНИЯ. Опознание — самый дорогой вопрос прохода (его задаёт модель
//! покрупнее), и он повторяется для одного продукта столько раз, сколько у него
//! недостающих признаков: сегодня спросили про овощи, завтра открылась неделя
//! мяса — и продукт опознают заново. Между этими разами он не меняется, поэтому
//! УВЕРЕННОЕ опознание запоминается вместе с его весом и подставляется готовым.
//! Неуверенное не запоминается: перепроверить его через сутки полезнее.
//!
//! АРЕНДА. Две вкладки приложения делят одну IndexedDB и каждая крутит свою
//! очередь. Без общего замка обе берут один и тот же продукт и задают модели
//! одинаковые вопросы — платим дважды, а вердикты могут ещё и разойтись. Перед
//! проходом продукт «арендуется» одной транзакцией (см. [`claim`]); аренда
//! протухает сама, чтобы закрытая на середине вкладка не заперла продукт навсегда.

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
    /// ТОЛЬКО У ОПОЗНАНИЯ: что это за еда — тот же текст, что уходит в узлы
    /// признаков. Пусто, если опознание не запоминали.
    #[serde(default)]
    pub identity: String,
    /// Вес того опознания. По нему видно, стоит ли ему верить, и он же уходит
    /// дальше в конвейер вместо заново вычисленного.
    #[serde(default)]
    pub identity_weight: f64,
    /// До какого времени продукт занят другой вкладкой, мс эпохи. 0 — свободен.
    #[serde(default)]
    pub lease_until: f64,
}

/// Опознание с весом НИЖЕ этого не запоминается: продукт, разобранный впритык,
/// стоит переспросить, когда выйдет сборка с новым словарём. Реальная еда получает
/// 0.95 и выше, пограничные случаи — 0.66.
const CACHE_MIN_WEIGHT: f64 = 0.8;

/// На сколько продукт занимается на время прохода. Проход по одному продукту — это
/// до семи запросов к модели с повторами, поэтому минуты мало. Аренда протухает
/// сама: вкладку могли закрыть посреди работы, и запирать продукт навсегда нельзя.
const LEASE_MS: f64 = 10.0 * 60.0 * 1000.0;

/// Ключ аренды продукта — отдельный «аспект», чтобы не мешаться со следами вопросов.
const ASPECT_LEASE: &str = "lease";

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
        identity: String::new(),
        identity_weight: 0.0,
        lease_until: 0.0,
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

/// Запомнить УВЕРЕННОЕ опознание. Слабое (ниже [`CACHE_MIN_WEIGHT`]) записывается
/// как обычная удачная попытка, без кэша: в следующий раз спросим заново.
pub async fn record_identity(food_id: &str, identity: &str, weight: f64) {
    let cached = weight >= CACHE_MIN_WEIGHT && !identity.trim().is_empty();
    let probe = Probe {
        key: key(food_id, super::flags_pipeline::ASPECT_IDENTITY),
        food_id: food_id.to_string(),
        aspect: super::flags_pipeline::ASPECT_IDENTITY.to_string(),
        attempted_at: now_ms(),
        ok: true,
        note: String::new(),
        identity: if cached { identity.chars().take(400).collect() } else { String::new() },
        identity_weight: if cached { weight } else { 0.0 },
        lease_until: 0.0,
    };
    db::put("food_probe", &probe).await;
}

/// Запомненное опознание продукта: `(что это за еда, вес)`.
pub async fn cached_identity(food_id: &str) -> Option<(String, f64)> {
    match identity_plan(
        db::get::<Probe>("food_probe", &key(food_id, super::flags_pipeline::ASPECT_IDENTITY)).await,
        now_ms(),
    ) {
        IdentityPlan::UseCached(identity, weight) => Some((identity, weight)),
        _ => None,
    }
}

/// Что делать с опознанием продукта, у которого не хватает признака.
#[derive(Debug, Clone, PartialEq)]
pub enum IdentityPlan {
    /// Взять запомненное — модель не спрашиваем вовсе.
    UseCached(String, f64),
    /// Спросить модель.
    Ask,
    /// Ждать: с прошлого запроса не прошли сутки.
    Wait,
}

/// ПОРЯДОК ПРИНЯТИЯ РЕШЕНИЯ, слово в слово по правилу:
///
/// 1. признака не хватает — иначе сюда не приходят вовсе (решает `classify`);
/// 2. есть запомненное уверенное опознание — берём его, [`UseCached`];
/// 3. кэша нет, и продукт вообще не спрашивали — [`Ask`];
/// 4. кэша нет, но запрос уже был — [`Ask`] только когда с него прошли сутки,
///    иначе [`Wait`].
///
/// Исход прошлого запроса на четвёртый пункт НЕ ВЛИЯЕТ. Удачный, но неуверенный
/// ответ (вес ниже [`CACHE_MIN_WEIGHT`]) не кэшируется — и если бы он открывал
/// дорогу сразу, продукт опознавали бы заново при каждом проходе, платя за самый
/// дорогой вопрос впустую.
///
/// [`UseCached`]: IdentityPlan::UseCached
/// [`Ask`]: IdentityPlan::Ask
/// [`Wait`]: IdentityPlan::Wait
pub fn identity_plan(probe: Option<Probe>, now: f64) -> IdentityPlan {
    let Some(p) = probe else {
        return IdentityPlan::Ask;
    };
    if p.ok && !p.identity.is_empty() && p.identity_weight >= CACHE_MIN_WEIGHT {
        return IdentityPlan::UseCached(p.identity, p.identity_weight);
    }
    if now - p.attempted_at >= RETRY_AFTER_MS {
        IdentityPlan::Ask
    } else {
        IdentityPlan::Wait
    }
}

/// Решение по опознанию этого продукта — та же [`identity_plan`], но с чтением следа.
pub async fn identity_plan_for(food_id: &str) -> IdentityPlan {
    identity_plan(
        db::get::<Probe>("food_probe", &key(food_id, super::flags_pipeline::ASPECT_IDENTITY)).await,
        now_ms(),
    )
}

/// Занять продукт на время прохода. `false` — им уже занят кто-то другой.
///
/// Читает и пишет ОДНОЙ транзакцией: между обычными `get` и `put` вторая вкладка
/// успевает прочитать ту же строку и решить, что продукт свободен.
pub async fn claim(food_id: &str) -> bool {
    let now = now_ms();
    let k = key(food_id, ASPECT_LEASE);
    let id = food_id.to_string();
    let row_key = k.clone();
    db::update_atomic::<Probe, _>("food_probe", &k, move |current| {
        if let Some(p) = &current {
            if p.lease_until > now {
                return None; // занято, и аренда ещё жива
            }
        }
        Some(Probe {
            key: row_key,
            food_id: id,
            aspect: ASPECT_LEASE.to_string(),
            attempted_at: now,
            ok: true,
            note: String::new(),
            identity: String::new(),
            identity_weight: 0.0,
            lease_until: now + LEASE_MS,
        })
    })
    .await
}

/// Освободить продукт. Вызывать на ЛЮБОМ выходе из прохода, иначе следующий заход
/// будет ждать, пока аренда протухнет сама.
pub async fn release(food_id: &str) {
    db::delete("food_probe", &key(food_id, ASPECT_LEASE)).await;
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

#[cfg(test)]
mod tests {
    use super::*;

    const DAY: f64 = 24.0 * 60.0 * 60.0 * 1000.0;

    fn probe(ok: bool, identity: &str, weight: f64, attempted_at: f64) -> Probe {
        Probe {
            key: "f1:identity".into(),
            food_id: "f1".into(),
            aspect: "identity".into(),
            attempted_at,
            ok,
            note: String::new(),
            identity: identity.into(),
            identity_weight: weight,
            lease_until: 0.0,
        }
    }

    #[test]
    fn nesprosennyj_produkt_sprashivaetsya_srazu() {
        assert_eq!(identity_plan(None, DAY), IdentityPlan::Ask);
    }

    #[test]
    fn uverennoe_opoznanie_beryotsya_iz_kesha() {
        let p = probe(true, "a type of squash", 0.95, DAY);
        assert_eq!(
            identity_plan(Some(p), DAY + 60_000.0),
            IdentityPlan::UseCached("a type of squash".into(), 0.95)
        );
    }

    #[test]
    fn udachnyj_no_neuverennyj_otvet_ne_keshiruetsya_i_zhdyot_sutki() {
        // Вес 0.66 — ниже порога кэша, значит `record_identity` кэш не положил.
        let p = probe(true, "", 0.0, DAY);
        assert_eq!(identity_plan(Some(p.clone()), DAY + 60_000.0), IdentityPlan::Wait);
        assert_eq!(identity_plan(Some(p), DAY + DAY), IdentityPlan::Ask);
    }

    #[test]
    fn neudachnoe_opoznanie_zhdyot_sutki() {
        let p = probe(false, "", 0.0, DAY);
        assert_eq!(identity_plan(Some(p.clone()), DAY + 3600_000.0), IdentityPlan::Wait);
        assert_eq!(identity_plan(Some(p), DAY + DAY + 1.0), IdentityPlan::Ask);
    }

    #[test]
    fn staraya_zapis_bez_novyh_polej_chitaetsya_i_znachit_sprosit_zanovo() {
        // Строка, записанная прежней версией приложения: полей кэша в ней нет.
        let old = serde_json::json!({
            "key": "f1:identity", "food_id": "f1", "aspect": "identity",
            "attempted_at": DAY, "ok": true
        });
        let p: Probe = serde_json::from_value(old).expect("старая строка читается");
        assert_eq!(p.identity, "");
        assert_eq!(p.identity_weight, 0.0);
        assert_eq!(p.lease_until, 0.0);
        assert_eq!(identity_plan(Some(p), DAY + DAY), IdentityPlan::Ask);
    }
}
