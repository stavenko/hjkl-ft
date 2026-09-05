//! Кэш шагов разбора: у каждой задачи свой ключ, и под ключом лежит её исход.
//!
//! Разбор ленивой записи — это не один запрос, а цепочка: каждый снимок читается
//! отдельно, потом прочитанное сводится в список, потом каждая позиция ищется в
//! базе, а ненайденная заводится новой едой и размечается словами для поиска.
//! Запросов получается N+1+M, и стоят они денег.
//!
//! Раньше вся эта цепочка жила в памяти одного прохода. Упал последний шаг —
//! выброшены все предыдущие, и завтрашняя попытка платила за них заново. У записи
//! с тремя снимками это три обращения к модели в сутки впустую, пока сбой не
//! починят. Здесь это и лечится: каждый шаг адресуется ХЭШЕМ СВОЕГО ВХОДА, и под
//! этим хэшем лежит, что с ним случилось.
//!
//! # Ключ — это вход, и отсюда всё остальное
//!
//! Ключ считается от имени шага и его входных данных ([`key`]). Из этого само
//! собой выходит главное свойство: **изменил человек снимки или описание —
//! изменился ключ**, и шаг спрашивается заново, без всяких счётчиков и сбросов.
//! Не изменил — берётся готовое. Одна и та же фотография в двух записях читается
//! один раз: ключ у неё один (снимки и сами адресуются хэшем, см. `images`).
//!
//! # Неудача — тоже результат, и она говорит, когда возвращаться
//!
//! Хранится не только успех. У неудачи есть [`Retry`] — что с ней делать:
//!
//! * [`Retry::Now`] — сервер прилёг, оборвался поток, модель ответила мусором.
//!   Следующий проход очереди спросит снова.
//! * [`Retry::AfterDay`] — сбой на нашей стороне, который сам собой за минуту не
//!   исчезнет: протух ключ, кончилась квота. Спрашивать раньше суток бессмысленно,
//!   а бросать насовсем неправильно — это чиним мы, а не человек.
//! * [`Retry::Human`] — ждём человека. Автоматически не повторяется НИКОГДА, и это
//!   не жестокость: пока он не переснимет или не допишет, вход тот же, а значит и
//!   ответ будет тот же. А как только переснимет — сменится ключ, и шаг спросится
//!   сам собой.
//!
//! # Процесс завершается, только если завершились все его шаги
//!
//! Ни один частичный результат не превращается в запись. Свалился один снимок из
//! трёх — запись остаётся нераспознанной целиком, а два прочитанных кадра лежат в
//! кэше и завтра не будут оплачены второй раз. Ждать при этом приходится по
//! САМОМУ ТЯЖЁЛОМУ из блокирующих шагов ([`Retry::worst`]): если один шаг просит
//! повторить сейчас, а другой — через сутки, раньше суток запись всё равно не
//! соберётся, и трогать её раньше значит жечь запросы впустую.

use serde::{de::DeserializeOwned, Deserialize, Serialize};
use sha2::{Digest, Sha256};

use super::db;

/// Store кэша. НЕ синкается: это кэш, а не данные человека, — его дешевле
/// посчитать заново на другом устройстве, чем гонять по сети.
const STORE: &str = "ai_steps";

/// Разделитель частей ключа. Управляющий символ, а не запятая: в названии
/// продукта запятая бывает, и `["а,б"]` не должно совпасть с `["а","б"]`.
const SEP: char = '\u{1f}';

/// Что делать с шагом, который не удался.
///
/// Порядок вариантов — это порядок ТЯЖЕСТИ, по нему считается [`Retry::worst`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Retry {
    /// Повторить при следующем проходе очереди.
    Now,
    /// Повторить не раньше чем через сутки.
    AfterDay,
    /// Не повторять: нужно действие человека.
    Human,
}

impl Retry {
    /// Самое тяжёлое из требований. Пусто — блокирующих шагов не было.
    ///
    /// Берётся именно максимум, а не минимум: запись собирается, только когда
    /// готовы ВСЕ шаги, поэтому ждать приходится по самому долгому из них.
    pub fn worst(all: impl IntoIterator<Item = Retry>) -> Option<Retry> {
        all.into_iter().max()
    }

    fn slug(self) -> &'static str {
        match self {
            Retry::Now => "now",
            Retry::AfterDay => "day",
            Retry::Human => "human",
        }
    }

    fn of_slug(s: &str) -> Retry {
        match s {
            "human" => Retry::Human,
            "day" => Retry::AfterDay,
            _ => Retry::Now,
        }
    }
}

/// Неудавшийся шаг: сырая причина для нас и правило возврата.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Failed {
    pub cause: String,
    pub retry: Retry,
}

/// Строка кэша. Успех и неудача — одна и та же строка с разными полями: под
/// ключом всегда лежит ПОСЛЕДНЕЕ, что с этим шагом случилось.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StepRow {
    /// Хэш входа, он же ключ.
    pub key: String,
    /// Имя шага. В ключ оно уже вошло, но здесь лежит читаемым — по нему видно,
    /// что в кэше, когда разбираешься с чужой базой.
    pub step: String,
    /// Ответ, если шаг удался.
    #[serde(default)]
    pub ok: Option<serde_json::Value>,
    /// Сырая причина, если не удался.
    #[serde(default)]
    pub cause: Option<String>,
    /// Правило возврата к неудавшемуся шагу.
    #[serde(default)]
    pub retry: Option<String>,
    /// Когда это записано. По нему считаются сутки и чистка.
    pub at: String,
}

/// Ключ шага — хэш имени и входных данных.
///
/// Чистая функция, и это важно: от неё зависит, спросят модель заново или возьмут
/// готовое, а такое правило проверяется тестом, а не счётом за запросы.
pub fn key(step: &str, parts: &[&str]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(step.as_bytes());
    for part in parts {
        hasher.update([SEP as u8]);
        hasher.update(part.as_bytes());
    }
    let digest = hasher.finalize();
    let mut out = String::with_capacity(digest.len() * 2);
    for byte in digest {
        out.push_str(&format!("{byte:02x}"));
    }
    out
}

/// Сколько ждать, прежде чем возвращаться к шагу с [`Retry::AfterDay`].
pub const WAIT_MS: i64 = 24 * 60 * 60 * 1000;

/// Пора ли пробовать неудавшийся шаг снова.
///
/// Чистая функция по той же причине, что и [`key`].
pub fn ready_again(row: &StepRow, now: chrono::DateTime<chrono::Utc>) -> bool {
    match row.retry.as_deref().map(Retry::of_slug) {
        None => false,
        Some(Retry::Now) => true,
        Some(Retry::Human) => false,
        Some(Retry::AfterDay) => row
            .at
            .parse::<chrono::DateTime<chrono::FixedOffset>>()
            .is_ok_and(|at| (now - at.with_timezone(&chrono::Utc)).num_milliseconds() >= WAIT_MS),
    }
}

/// Когда возвращаться к шагу, сорвавшемуся с такой причиной.
///
/// Отвечает НЕ на тот вопрос, что `errors::worth_retrying`, и потому написано
/// отдельно. Там речь про повтор внутри одного запроса, через доли секунды; здесь
/// — про следующий проход очереди, до которого проходят минуты и часы. Расходятся
/// они ровно на 429: подряд долбиться в «слишком часто» нельзя, а вернуться к нему
/// на следующем проходе — самое то.
///
/// - **4xx, кроме 408 и 429** — сутки. Протух ключ, кончилась квота, запрос
///   отвергнут: чиним это мы, и за минуту такое не проходит.
/// - **всё остальное** — следующий проход. 5xx, оборванный поток, неразобранный
///   ответ, «слишком часто».
pub fn classify(cause: &str) -> Retry {
    match super::errors::http_status(cause) {
        Some(408) | Some(429) => Retry::Now,
        Some(s) if (400..500).contains(&s) => Retry::AfterDay,
        _ => Retry::Now,
    }
}

/// Готовый исход шага, если он есть и его можно взять.
///
/// `Some(Ok(_))` — шаг уже удался, спрашивать нечего.
/// `Some(Err(_))` — шаг не удался и время возвращаться ещё не пришло.
/// `None` — шага не было либо пора спрашивать снова.
async fn ready<T: DeserializeOwned>(
    key: &str,
    now: chrono::DateTime<chrono::Utc>,
) -> Option<Result<T, Failed>> {
    let row: StepRow = db::get(STORE, key).await?;
    if let Some(value) = row.ok.clone() {
        // Разобрать не вышло — значит формат ответа с тех пор изменился (вышла
        // новая сборка). Кэш тут не помощник: спрашиваем заново.
        return serde_json::from_value(value).ok().map(Ok);
    }
    if ready_again(&row, now) {
        return None;
    }
    Some(Err(Failed {
        cause: row.cause.clone().unwrap_or_default(),
        retry: row.retry.as_deref().map(Retry::of_slug).unwrap_or(Retry::Now),
    }))
}

async fn remember(key: &str, step: &str, ok: Option<serde_json::Value>, failed: Option<&Failed>) {
    let row = StepRow {
        key: key.to_string(),
        step: step.to_string(),
        ok,
        cause: failed.map(|f| f.cause.clone()),
        retry: failed.map(|f| f.retry.slug().to_string()),
        at: super::local::now(),
    };
    db::put(STORE, &row).await;
}

/// Выполнить шаг — или взять готовое.
///
/// `step` и `parts` дают ключ; `parts` обязаны содержать ВСЁ, от чего зависит
/// ответ, иначе кэш вернёт чужой результат на изменившийся вход.
pub async fn run<T, F, Fut>(step: &str, parts: &[&str], call: F) -> Result<T, Failed>
where
    T: Serialize + DeserializeOwned,
    F: FnOnce() -> Fut,
    Fut: std::future::Future<Output = Result<T, String>>,
{
    let key = key(step, parts);
    if let Some(done) = ready::<T>(&key, chrono::Utc::now()).await {
        return done;
    }
    match call().await {
        Ok(value) => {
            remember(&key, step, serde_json::to_value(&value).ok(), None).await;
            Ok(value)
        }
        Err(cause) => {
            let failed = Failed { retry: classify(&cause), cause };
            remember(&key, step, None, Some(&failed)).await;
            Err(failed)
        }
    }
}

/// Записать исход шага, который решается НЕ запросом к модели.
///
/// Нужен там, где ответ модели пришёл, но пользоваться им нельзя: еды на кадре не
/// нашлось, описание пустое. Такой шаг тоже неудача, только ждёт она человека, а
/// не сервера, — и лежать это должно там же, где всё остальное.
pub async fn remember_failure(step: &str, parts: &[&str], failed: &Failed) {
    remember(&key(step, parts), step, None, Some(failed)).await;
}

/// Выбросить всё, что старше срока.
///
/// Срок тот же, что у снимков (`local::IMAGE_KEEP_DAYS`), и это не совпадение:
/// после него разбирать всё равно нечего — сами кадры уже стёрты, и держать их
/// прочтение незачем. Возвращает, сколько выброшено.
pub async fn sweep(now: chrono::DateTime<chrono::Utc>, keep_days: i64) -> usize {
    let rows: Vec<StepRow> = db::list_all(STORE).await;
    let stale: Vec<String> = rows.iter().filter(|r| aged_out(r, now, keep_days)).map(|r| r.key.clone()).collect();
    let n = stale.len();
    for key in stale {
        db::delete(STORE, &key).await;
    }
    n
}

/// Пережила ли строка свой срок. Отдельной функцией — чтобы правило проверялось
/// тестом, а не спустя неделю на живой базе.
pub fn aged_out(row: &StepRow, now: chrono::DateTime<chrono::Utc>, keep_days: i64) -> bool {
    row.at
        .parse::<chrono::DateTime<chrono::FixedOffset>>()
        .is_ok_and(|at| (now - at.with_timezone(&chrono::Utc)).num_days() >= keep_days)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(iso: &str) -> chrono::DateTime<chrono::Utc> {
        iso.parse::<chrono::DateTime<chrono::FixedOffset>>().unwrap().with_timezone(&chrono::Utc)
    }

    fn row(retry: Option<Retry>, at_iso: &str) -> StepRow {
        StepRow {
            key: "k".into(),
            step: "s".into(),
            ok: None,
            cause: Some("причина".into()),
            retry: retry.map(|r| r.slug().to_string()),
            at: at_iso.into(),
        }
    }

    #[test]
    fn kljuch_menyaetsya_vmeste_so_vhodom() {
        // Всё правило кэша держится на этом: тот же вход — тот же ключ, другой
        // вход — другой, и никакого сброса счётчиков не нужно.
        assert_eq!(key("read", &["a"]), key("read", &["a"]));
        assert_ne!(key("read", &["a"]), key("read", &["b"]));
        // Имя шага — часть ключа: одинаковый вход у разных шагов не должен
        // столкнуться.
        assert_ne!(key("read", &["a"]), key("merge", &["a"]));
    }

    #[test]
    fn granicy_chastej_ne_razmyvayutsya() {
        // «а,б» одной частью и «а»,«б» двумя — это РАЗНЫЕ входы. Без разделителя
        // они склеились бы в один ключ, и описание еды подменяло бы соседнее поле.
        assert_ne!(key("merge", &["а,б"]), key("merge", &["а", "б"]));
        assert_ne!(key("merge", &["аб"]), key("merge", &["а", "б"]));
    }

    #[test]
    fn zhdyom_po_samomu_tyazhyolomu() {
        // Один шаг просит повторить сейчас, другой — через сутки: раньше суток
        // запись всё равно не соберётся.
        assert_eq!(Retry::worst([Retry::Now, Retry::AfterDay]), Some(Retry::AfterDay));
        assert_eq!(Retry::worst([Retry::Now, Retry::Human, Retry::AfterDay]), Some(Retry::Human));
        assert_eq!(Retry::worst([Retry::Now, Retry::Now]), Some(Retry::Now));
        assert_eq!(Retry::worst([]), None);
    }

    #[test]
    fn srazu_znachit_pri_sleduyushchem_prohode() {
        let now = at("2026-09-05T12:00:00Z");
        assert!(ready_again(&row(Some(Retry::Now), "2026-09-05T11:59:00Z"), now));
    }

    #[test]
    fn sutki_znachit_ne_ranshe_sutok() {
        let now = at("2026-09-05T12:00:00Z");
        assert!(!ready_again(&row(Some(Retry::AfterDay), "2026-09-04T13:00:00Z"), now), "23 часа — рано");
        assert!(ready_again(&row(Some(Retry::AfterDay), "2026-09-04T11:00:00Z"), now), "25 часов — пора");
    }

    #[test]
    fn ozhidanie_cheloveka_vremenem_ne_lechitsya() {
        // Через год ответ будет тот же: вход не изменился. А изменится — сменится
        // ключ, и шаг спросится сам, мимо этой строки.
        let now = at("2027-09-05T12:00:00Z");
        assert!(!ready_again(&row(Some(Retry::Human), "2026-09-05T12:00:00Z"), now));
    }

    #[test]
    fn sboj_svyazi_razbiraetsya_tem_zhe_pravilom() {
        assert_eq!(classify("HTTP 500: oops"), Retry::Now);
        assert_eq!(classify("parse error: чушь"), Retry::Now);
        assert_eq!(classify("HTTP 401: Unauthorized"), Retry::AfterDay);
        assert_eq!(classify("HTTP 403: Forbidden"), Retry::AfterDay);
        // «Слишком часто» — про сейчас, а не про вообще.
        assert_eq!(classify("HTTP 429: slow down"), Retry::Now);
    }

    #[test]
    fn starye_stroki_vybrasyvayutsya_vmeste_so_snimkami() {
        let now = at("2026-09-12T12:00:00Z");
        assert!(aged_out(&row(Some(Retry::Now), "2026-09-05T11:00:00Z"), now, 7), "неделя прошла");
        assert!(!aged_out(&row(Some(Retry::Now), "2026-09-08T11:00:00Z"), now, 7), "четыре дня — рано");
    }
}
