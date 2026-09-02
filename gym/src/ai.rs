//! Единственное, зачем приложению тренировок пока нужна модель: придумать фразу
//! восстановления.
//!
//! Тот же ai-worker и тот же токен, что у приложения питания, — ручка
//! подписочная, а подписка здесь уже проверена. Своей логики у модуля нет:
//! спросили пять слов, вычистили ответ, отдали.

use wasm_bindgen::JsCast;
use wasm_bindgen::JsValue;
use wasm_bindgen_futures::JsFuture;

use crate::i18n::{self, Lang};
use crate::{auth, config};

/// Придумать свежую фразу из пяти слов на языке человека.
///
/// Ответ модели ЧИСТИТСЯ до ровно пяти простых слов (строчные, только буквы, от
/// двух знаков): модель охотно добавляет нумерацию, кавычки и пояснения, а фразу
/// человеку потом набирать руками. Меньше пяти слов — ошибка, а не «сколько
/// получилось»: короткую фразу сервер и сам не примет.
pub async fn generate_backup_phrase() -> Result<String, String> {
    let prompt = match i18n::get() {
        Lang::Ru => "Придумай 5 простых, не связанных между собой нарицательных существительных \
                     в единственном числе на русском языке. Ответь ТОЛЬКО пятью словами через \
                     пробел: без нумерации, без запятых, без кавычек, строчными буквами.",
        Lang::En => "Invent 5 simple, unrelated common nouns in English. Reply with ONLY the five \
                     words separated by spaces: no numbering, no commas, no quotes, lowercase.",
    };
    let raw = summarize(prompt).await?;
    let words = sanitize_phrase(&raw);
    if words.len() < 5 {
        return Err("модель вернула слишком мало слов".to_string());
    }
    Ok(words.join(" "))
}

/// Вычистить ответ модели до слов, годных для фразы.
fn sanitize_phrase(raw: &str) -> Vec<String> {
    raw.split_whitespace()
        .map(|w| w.trim_matches(|c: char| !c.is_alphabetic()).to_lowercase())
        .filter(|w| w.chars().count() >= 2 && w.chars().all(|c| c.is_alphabetic()))
        .take(5)
        .collect()
}

/// Один вопрос модели без потока — ответ целиком.
async fn summarize(prompt: &str) -> Result<String, String> {
    let base = &config::get().ai_base_url;
    if base.is_empty() {
        return Err("ai_base_url не сконфигурирован".to_string());
    }
    let token = auth::get_token().ok_or_else(|| "нет сессии".to_string())?;
    let url = format!("{base}/chat/completions");
    let body = serde_json::json!({
        "model": "@cf/qwen/qwen3-30b-a3b-fp8",
        "stream": false,
        "chat_template_kwargs": { "enable_thinking": false },
        "messages": [{ "role": "user", "content": prompt }],
    });

    let opts = web_sys::RequestInit::new();
    opts.set_method("POST");
    opts.set_body(&JsValue::from_str(
        &serde_json::to_string(&body).map_err(|e| e.to_string())?,
    ));
    let headers = web_sys::Headers::new().map_err(|e| format!("{e:?}"))?;
    headers.set("Content-Type", "application/json").map_err(|e| format!("{e:?}"))?;
    headers
        .set("Authorization", &format!("Bearer {token}"))
        .map_err(|e| format!("{e:?}"))?;
    opts.set_headers(&headers);

    let request = web_sys::Request::new_with_str_and_init(&url, &opts).map_err(|e| format!("{e:?}"))?;
    let window = web_sys::window().expect("no window");
    let resp_val = JsFuture::from(window.fetch_with_request(&request))
        .await
        .map_err(|e| format!("{e:?}"))?;
    let resp: web_sys::Response = resp_val.dyn_into().map_err(|_| "not a Response".to_string())?;
    let text = JsFuture::from(resp.text().map_err(|e| format!("{e:?}"))?)
        .await
        .map_err(|e| format!("{e:?}"))?;
    let text = text.as_string().ok_or("response not string")?;
    if !resp.ok() {
        return Err(format!("HTTP {}: {}", resp.status(), text));
    }

    let v: serde_json::Value = serde_json::from_str(&text).map_err(|e| e.to_string())?;
    v.pointer("/choices/0/message/content")
        .and_then(|c| c.as_str())
        .map(|s| s.trim().to_string())
        .ok_or_else(|| format!("непонятный ответ модели: {text}"))
}

#[cfg(test)]
mod tests {
    use super::sanitize_phrase;

    #[test]
    fn chistit_numeraciyu_i_kavychki() {
        let raw = "1. «стол», 2. окно, 3. \"перо\", 4. мост, 5. лампа";
        assert_eq!(sanitize_phrase(raw), vec!["стол", "окно", "перо", "мост", "лампа"]);
    }

    #[test]
    fn beryot_rovno_pyat_slov() {
        assert_eq!(sanitize_phrase("один два три четыре пять шесть семь").len(), 5);
    }

    // Односимвольные обрывки и голая пунктуация словами не считаются — иначе
    // «фраза» из пяти запятых прошла бы как годная.
    #[test]
    fn otbrasyvaet_ogryzki() {
        assert_eq!(sanitize_phrase("a — стол , окно"), vec!["стол", "окно"]);
    }
}
