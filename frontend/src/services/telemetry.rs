//! Отправка ошибок приложения нам — чтобы мы видели их, не дожидаясь жалобы.
//!
//! До этого модуля всё, что происходило у человека, оставалось у человека: журнал
//! под треугольником живёт в памяти и умирает с перезагрузкой. Узнать, что у
//! десяти человек не определяется один и тот же продукт, было неоткуда.
//!
//! Приёмник — `POST /event` на bug-report-worker, оттуда точка уходит в
//! Analytics Engine. Отправка «выстрелил и забыл»: провал НИКОГДА не превращается
//! в запись журнала, иначе неудачная отправка порождала бы ошибку, которая
//! порождала бы отправку.
//!
//! Что уходит: вид события, короткий устойчивый код, техническая причина,
//! название продукта, версия сборки и платформа. Название продукта — не новая
//! утечка: оно и так уходит в ai-worker, иначе его нечем было бы разбирать.
//! Ничего сверх этого — ни дневника, ни веса, ни переписки.

use serde::Serialize;
use wasm_bindgen::JsValue;

use super::errors::{code_of, AppError};
use super::{auth, config};

/// Сколько символов технической причины отправляем. Длинные ответы модели режем:
/// диагностическая ценность в начале, а точка данных не резиновая.
const CAUSE_LIMIT: usize = 400;

#[derive(Serialize)]
struct Event<'a> {
    /// Устойчивый код: один и тот же сбой — один и тот же код.
    code: String,
    /// `food.iron`, `planka.calories` — по чему группировать.
    kind: &'a str,
    /// К чему относится: название продукта и т. п.
    subject: &'a str,
    /// Техническая причина, обрезанная.
    cause: String,
    /// Версия сборки — чтобы отличить «уже починили» от «до сих пор».
    build: String,
    /// Платформа по нашему же определителю: ios_safari, android_chrome, …
    platform: &'static str,
}

/// Отправить ошибку. Ничего не возвращает и ни на что не влияет.
pub fn report(e: &AppError) {
    let base = config::get().bug_report_base_url.clone();
    // Без адреса приёмника или без сессии отправлять некуда и нечем: событие
    // теряется молча. Шуметь об этом бессмысленно — человеку это не сообщение.
    if base.is_empty() {
        return;
    }
    let Some(token) = auth::get_token() else { return };

    let cause = e.cause.clone().unwrap_or_else(|| e.message.clone());
    let cause: String = cause.chars().take(CAUSE_LIMIT).collect();
    // Код считается по ВИДУ и ПРИЧИНЕ, без названия продукта: иначе одна и та же
    // поломка на сотне продуктов дала бы сотню разных кодов и не сложилась бы.
    let code = code_of(&format!("{}|{cause}", e.kind));

    let event = Event {
        code,
        kind: &e.kind,
        subject: &e.context,
        cause,
        build: build_version(),
        platform: crate::pages::pwa_prompt::detect_platform(),
    };
    let Ok(body) = serde_json::to_string(&event) else { return };

    leptos::spawn_local(async move {
        if let Err(e) = post(&base, &token, &body).await {
            // Только в консоль. В журнал нельзя: запись оттуда снова позвала бы
            // сюда, и неудачная отправка закольцевалась бы сама на себя.
            leptos::logging::warn!("телеметрия не ушла: {e}");
        }
    });
}

/// Версия работающей сборки — её же показывает экран «Версия» в настройках.
fn build_version() -> String {
    js_sys::Reflect::get(&js_sys::global(), &JsValue::from_str("__APP_VERSION__"))
        .ok()
        .and_then(|v| v.as_string())
        .unwrap_or_default()
}

async fn post(base: &str, token: &str, body: &str) -> Result<(), String> {
    use wasm_bindgen::JsCast;
    use wasm_bindgen_futures::JsFuture;

    let opts = web_sys::RequestInit::new();
    opts.set_method("POST");
    opts.set_body(&JsValue::from_str(body));

    let headers = web_sys::Headers::new().map_err(|e| format!("{e:?}"))?;
    headers.set("Content-Type", "application/json").map_err(|e| format!("{e:?}"))?;
    headers.set("Authorization", &format!("Bearer {token}")).map_err(|e| format!("{e:?}"))?;
    opts.set_headers(&headers);

    let request = web_sys::Request::new_with_str_and_init(&format!("{base}/event"), &opts)
        .map_err(|e| format!("{e:?}"))?;
    let window = web_sys::window().expect("no window");
    let resp_val = JsFuture::from(window.fetch_with_request(&request))
        .await
        .map_err(|e| format!("{e:?}"))?;
    let resp: web_sys::Response = resp_val.dyn_into().map_err(|_| "not a Response".to_string())?;
    if !resp.ok() {
        return Err(format!("HTTP {}", resp.status()));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::super::errors::code_of;

    #[test]
    fn kod_ustojchiv_i_razlichaet() {
        assert_eq!(code_of("food.iron|timeout"), code_of("food.iron|timeout"));
        assert_ne!(code_of("food.iron|timeout"), code_of("food.kind|timeout"));
        assert_eq!(code_of("food.iron|timeout").len(), 6);
    }
}
