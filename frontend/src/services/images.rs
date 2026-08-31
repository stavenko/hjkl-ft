//! Провайдер изображений — единственное место, где в приложении лежит картинка.
//!
//! Всё, что должно сохраниться, кладётся сюда и адресуется ХЭШЕМ СОДЕРЖИМОГО:
//! потребитель держит короткую строку, а не мегабайт base64, и одна и та же
//! фотография, попавшая в две записи, хранится один раз. Формат хранения —
//! base64 без префикса data-URL: всё происходит в браузере и локально, а
//! `<img src>` собирается на месте отрисовки.
//!
//! Store `images` НЕ синкается (см. `db::builder`): картинка принадлежит
//! устройству, на котором снята.

use std::collections::BTreeSet;

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use super::db;

/// Строка store `images`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StoredImage {
    /// Хэш содержимого, он же ключ.
    pub hash: String,
    /// Сама картинка: base64 БЕЗ префикса `data:…;base64,`.
    pub data: String,
    pub created_at: String,
}

/// Хэш содержимого — по нему картинка и адресуется.
///
/// Считается от base64-строки, а не от разжатых байт: до байт нам дела нет, а
/// строка — ровно то, что мы храним, и одинаковый вход даёт одинаковый ключ.
pub fn hash_of(base64: &str) -> String {
    let digest = Sha256::digest(base64.as_bytes());
    let mut out = String::with_capacity(digest.len() * 2);
    for byte in digest {
        out.push_str(&format!("{byte:02x}"));
    }
    out
}

/// Положить картинку и получить её хэш.
///
/// Повторный вызов с тем же содержимым не пишет ничего: строка уже на месте, и
/// перезапись только сдвинула бы `created_at`, по которому мы судим о возрасте.
pub async fn put(base64: &str) -> String {
    let hash = hash_of(base64);
    if db::get::<StoredImage>("images", &hash).await.is_some() {
        return hash;
    }
    let image = StoredImage {
        hash: hash.clone(),
        data: base64.to_string(),
        created_at: super::local::now(),
    };
    db::put("images", &image).await;
    hash
}

/// Картинка по хэшу — base64 без префикса. `None`, если её уже нет: срок
/// хранения вышел, или это чужое устройство, куда картинка не приезжала.
pub async fn get(hash: &str) -> Option<String> {
    db::get::<StoredImage>("images", hash)
        .await
        .map(|image| image.data)
}

/// Готовый `src` для `<img>`. Отдельная функция, чтобы префикс data-URL был
/// записан в одном месте, а не собирался заново у каждого потребителя.
pub async fn data_url(hash: &str) -> Option<String> {
    get(hash)
        .await
        .map(|data| format!("data:image/jpeg;base64,{data}"))
}

/// Удалить картинку по хэшу.
pub async fn remove(hash: &str) {
    db::delete("images", hash).await;
}

/// Выбросить всё, на что никто больше не ссылается, и вернуть число удалённых.
///
/// `live` — хэши, которые ДОЛЖНЫ остаться; собирает их вызывающий, потому что
/// только он знает, чьи картинки ещё нужны (недельный срок у распознанных
/// записей — его правило, не наше).
pub async fn prune(live: &BTreeSet<String>) -> usize {
    let mut dropped = 0;
    for image in db::list_all::<StoredImage>("images").await {
        if !live.contains(&image.hash) {
            remove(&image.hash).await;
            dropped += 1;
        }
    }
    dropped
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hash_is_stable_and_content_addressed() {
        // Одинаковое содержимое — одинаковый ключ, разное — разный. На этом
        // держится и дедупликация, и то, что `put` можно звать повторно.
        assert_eq!(hash_of("QUJD"), hash_of("QUJD"));
        assert_ne!(hash_of("QUJD"), hash_of("QUJE"));
        assert_eq!(hash_of("QUJD").len(), 64);
        assert!(hash_of("QUJD").chars().all(|c| c.is_ascii_hexdigit()));
    }
}
