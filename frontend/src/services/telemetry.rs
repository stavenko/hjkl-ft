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
use wasm_bindgen::{JsCast, JsValue};

use super::errors::{code_of, AppError};
use super::{auth, config};

/// Сколько символов причины отправляем. Причина — это и есть то, ради чего всё
/// затевалось: по ней чинят. Режем только чтобы не упереться в предел точки
/// данных (16 КБ на все блобы разом), а не «на всякий случай».
const CAUSE_LIMIT: usize = 8000;

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

#[derive(Serialize)]
struct Detection<'a> {
    /// `fat.row`, `flag.heme`, `dish.composite` — по чему группировать.
    kind: &'a str,
    /// Продукт: название, как его ввёл человек.
    subject: &'a str,
    /// Что решила модель: ключ строки, «true»/«false», значение.
    verdict: &'a str,
    /// Обоснование модели, её словами.
    reason: String,
    build: String,
    platform: &'static str,
    /// Числа определения; у признаков нули.
    n1: f64,
    n2: f64,
    n3: f64,
    n4: f64,
}

/// Отправить то, что человеку не показывают.
///
/// Наши внутренние сбои — не открылась база, не разобралась строка, упала
/// паника. Человеку про них сообщать нечего (чинить ему нечем), а нам знать
/// обязательно: до этого они жили только в консоли его телефона.
pub fn report_internal(kind: &str, subject: &str, cause: &str) {
    send(kind, subject, cause);
}

/// Отправить ошибку, показанную человеку.
pub fn report(e: &AppError) {
    let cause = e.cause.clone().unwrap_or_else(|| e.message.clone());
    send(&e.kind, &e.context, &cause);
}

/// Что модель РАЗОБРАЛА — вердикт, а не сбой.
///
/// Ошибки показывают только то, обо что споткнулись; неверный ответ, прошедший
/// проверку, выглядит как обычная работа и не виден вообще. Поэтому определения
/// уходят отдельным потоком: по нему видно, что у людей реально определяется, и
/// какие продукты стоит прогнать собственным замером.
///
/// `numbers` — до четырёх величин определения (у жира: НЖК, МНЖК, ПНЖК, EPA+DHA);
/// у признаков чисел нет, там пустой срез.
pub fn report_detection(kind: &str, subject: &str, verdict: &str, reason: &str, numbers: &[f64]) {
    let base = config::get().bug_report_base_url.clone();
    if base.is_empty() {
        return;
    }
    let Some(token) = auth::get_token() else { return };
    let n = |i: usize| numbers.get(i).copied().unwrap_or(0.0);
    let detection = Detection {
        kind,
        subject: &subject.chars().take(CAUSE_LIMIT).collect::<String>(),
        verdict: &verdict.chars().take(CAUSE_LIMIT).collect::<String>(),
        reason: reason.chars().take(CAUSE_LIMIT).collect(),
        build: build_version(),
        platform: crate::pages::pwa_prompt::detect_platform(),
        n1: n(0),
        n2: n(1),
        n3: n(2),
        n4: n(3),
    };
    let Ok(body) = serde_json::to_string(&detection) else { return };
    let Some(promise) = dispatch_to(&base, "/detection", &token, &body) else { return };
    // Исход интересует только консоль: определение — не работа человека, повторять
    // его некому и незачем.
    leptos::spawn_local(async move {
        if let Err(e) = wasm_bindgen_futures::JsFuture::from(promise).await {
            leptos::logging::warn!("телеметрия определений не ушла: {e:?}");
        }
    });
}

fn send(kind: &str, subject: &str, cause: &str) {
    // Без адреса приёмника или без сессии отправлять некуда и нечем: событие
    // теряется молча. Шуметь об этом бессмысленно — человеку это не сообщение.
    let base = config::get().bug_report_base_url.clone();
    if base.is_empty() {
        return;
    }
    let Some(token) = auth::get_token() else { return };
    let cause: String = cause.chars().take(CAUSE_LIMIT).collect();
    // Код считается по ВИДУ и ПРИЧИНЕ, без названия продукта: иначе одна и та же
    // поломка на сотне продуктов дала бы сотню разных кодов и не сложилась бы.
    let code = code_of(&format!("{kind}|{cause}"));

    let event = Event {
        code,
        kind,
        subject,
        cause,
        build: build_version(),
        platform: crate::pages::pwa_prompt::detect_platform(),
    };
    let Ok(body) = serde_json::to_string(&event) else { return };

    // Запрос отправляется ЗДЕСЬ, синхронно, а не внутри задачи. Разница
    // принципиальна для паники: после неё исполнение wasm обрывается, и
    // отложенная задача уже не запустится — сообщение о падении потерялось бы
    // ровно там, где нужнее всего. `fetch` же уходит в сеть в момент вызова.
    let Some(promise) = dispatch(&base, &token, &body) else { return };

    // Ответ дожидаемся только ради жалобы в консоль; сам запрос уже в пути.
    leptos::spawn_local(async move {
        match wasm_bindgen_futures::JsFuture::from(promise).await {
            Ok(v) => {
                if let Some(resp) = v.dyn_ref::<web_sys::Response>() {
                    if !resp.ok() {
                        leptos::logging::warn!("телеметрия: HTTP {}", resp.status());
                    }
                }
            }
            // Только в консоль. В журнал нельзя: запись оттуда снова позвала бы
            // сюда, и неудачная отправка закольцевалась бы сама на себя.
            Err(e) => leptos::logging::warn!("телеметрия не ушла: {e:?}"),
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

/// Отправить ошибку на `/event`.
fn dispatch(base: &str, token: &str, body: &str) -> Option<js_sys::Promise> {
    dispatch_to(base, "/event", token, body)
}

/// Отправить запрос НЕМЕДЛЕННО и вернуть его обещание.
///
/// Ни одного `expect`: сюда приходят и из обработчика паники, а паника внутри
/// обработчика паники — это уже ничем не диагностируемый обрыв.
fn dispatch_to(base: &str, path: &str, token: &str, body: &str) -> Option<js_sys::Promise> {
    let opts = web_sys::RequestInit::new();
    opts.set_method("POST");
    opts.set_body(&JsValue::from_str(body));
    // `keepalive` — чтобы браузер не отменил запрос, если страница в этот момент
    // рушится или закрывается. В web-sys этого сеттера нет, а поле обычное.
    let _ = js_sys::Reflect::set(&opts, &JsValue::from_str("keepalive"), &JsValue::TRUE);

    let headers = web_sys::Headers::new().ok()?;
    headers.set("Content-Type", "application/json").ok()?;
    headers.set("Authorization", &format!("Bearer {token}")).ok()?;
    opts.set_headers(&headers);

    let request = web_sys::Request::new_with_str_and_init(&format!("{base}{path}"), &opts).ok()?;
    let window = web_sys::window()?;
    Some(window.fetch_with_request(&request))
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
