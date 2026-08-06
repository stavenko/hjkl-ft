//! Разбор отказов PassKey в точные сообщения.
//!
//! До этого модуля ЛЮБОЙ отказ `navigator.credentials.create/get` показывался как
//! «Создание PassKey было отменено» — включая те случаи, где человек ничего не
//! отменял: пропала сеть, не включён код-пароль, ключ для этого аккаунта на
//! устройстве уже есть. Сообщение, не отражающее причины, хуже отсутствия
//! сообщения: оно уводит и пользователя, и нас.
//!
//! Браузер отдаёт причину в `DOMException.name`. Имён немного, и каждое означает
//! своё — см. [`describe`].

use wasm_bindgen::JsValue;

use super::i18n::t;

/// Какую операцию просили у хранилища ключей.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Op {
    /// `credentials.create` — завести новый ключ.
    Create,
    /// `credentials.get` — предъявить существующий.
    Get,
}

impl Op {
    fn prefix(self) -> &'static str {
        match self {
            Op::Create => "pk.create",
            Op::Get => "pk.get",
        }
    }
}

/// Отказ вызова WebAuthn, каким его увидел браузер. Хранится целиком: имя нужно
/// для разбора, текст — для журнала.
pub struct DomError {
    pub name: String,
    pub message: String,
}

impl DomError {
    pub fn from_js(e: &JsValue) -> Self {
        let field = |k: &str| {
            js_sys::Reflect::get(e, &k.into())
                .ok()
                .and_then(|v| v.as_string())
                .unwrap_or_default()
        };
        let name = field("name");
        let message = field("message");
        if name.is_empty() && message.is_empty() {
            // Не DOMException: строка, число, что угодно. Терять это нельзя.
            return DomError { name: String::new(), message: format!("{e:?}") };
        }
        DomError { name, message }
    }
}

/// Умеет ли эта страница вообще создавать ключи. `Some(сообщение)` — не умеет.
///
/// Проверяется ДО обращения к серверу: незачем начинать регистрацию, которая
/// заведомо сорвётся на середине, оставив на сервере недоделанную запись.
pub fn precheck() -> Option<String> {
    let window = web_sys::window().expect("no window");

    // WebAuthn живёт только в защищённом контексте. Без этого `credentials`
    // существует, но create/get отказывают с невнятным SecurityError.
    if !window.is_secure_context() {
        return Some(t("pk.insecure").to_string());
    }

    let has_api = js_sys::Reflect::get(&window, &"PublicKeyCredential".into())
        .map(|v| !v.is_undefined() && !v.is_null())
        .unwrap_or(false);
    if !has_api {
        return Some(t("pk.unsupported").to_string());
    }

    None
}

/// Считает ли устройство себя подключённым к сети.
///
/// `false` — достоверный признак: браузер не видит ни одного интерфейса. `true`
/// ничего не гарантирует (Wi-Fi без интернета сюда же), поэтому используется
/// только чтобы УТОЧНИТЬ уже случившийся отказ, а не чтобы предсказать его.
pub fn offline() -> bool {
    !web_sys::window().expect("no window").navigator().on_line()
}

/// Порог, ниже которого отказ не мог быть решением человека.
///
/// Системный лист с Face ID нужно увидеть и осознанно закрыть; уложиться в
/// полсекунды нельзя. Такой отказ приходит от самой системы — до того, как
/// человека о чём-то спросили.
const HUMAN_REACTION_MS: f64 = 500.0;

/// Отказ WebAuthn человеческим языком.
///
/// * `op` — что просили;
/// * `e` — отказ, как его отдал браузер;
/// * `elapsed_ms` — сколько прошло с вызова: отличает отказ системы от решения
///   человека и от истёкшего времени;
/// * `timeout_ms` — таймаут, который мы передали в опциях (из ответа сервера).
///
/// `NotAllowedError` браузеры выдают на три разных события — отмену, таймаут и
/// отказ системы — намеренно, чтобы страница не могла отличить «ключа нет» от
/// «человек передумал». Различаем по времени: единственный доступный признак.
pub fn describe(op: Op, e: &DomError, elapsed_ms: f64, timeout_ms: Option<f64>) -> String {
    leptos::logging::error!(
        "PassKey {op:?} отказ: name={} message={} через {elapsed_ms:.0} мс",
        e.name,
        e.message
    );

    let key = key_for(op, &e.name, elapsed_ms, timeout_ms, offline());
    let msg = t(&key).to_string();
    if key.ends_with(".unknown") && !e.name.is_empty() {
        // Имя, которого мы не разбирали. Показываем как есть: пусть лучше человек
        // перешлёт нам непонятное слово, чем увидит выдумку.
        format!("{msg} ({})", e.name)
    } else {
        msg
    }
}

/// Ключ сообщения по имени отказа. Вся логика различения — здесь, и она чистая:
/// снаружи остаётся только считать состояние браузера.
fn key_for(
    op: Op,
    name: &str,
    elapsed_ms: f64,
    timeout_ms: Option<f64>,
    offline: bool,
) -> String {
    let p = op.prefix();
    match name {
        "NotAllowedError" => {
            if offline {
                // Достоверно: сети нет. Хранилище ключей у Apple и Google
                // синхронизируется через их серверы, без сети оно отказывает.
                "pk.offline".to_string()
            } else if elapsed_ms < HUMAN_REACTION_MS {
                format!("{p}.blocked")
            } else if timeout_ms.is_some_and(|t| elapsed_ms >= t * 0.9) {
                format!("{p}.timeout")
            } else {
                format!("{p}.cancelled")
            }
        }
        // На create: ключ для этого аккаунта на устройстве уже есть
        // (сработал excludeCredentials). На get: предъявлять нечего.
        "InvalidStateError" => match op {
            Op::Create => "pk.create.exists".to_string(),
            Op::Get => "pk.get.no_key".to_string(),
        },
        "NotSupportedError" => format!("{p}.unsupported_algo"),
        // rpId не подходит к адресу страницы. Это наша настройка, не пользователя.
        "SecurityError" => format!("{p}.origin"),
        // Требуемую проверку личности выполнить нечем: нет ни биометрии, ни кода.
        "ConstraintError" => format!("{p}.no_screen_lock"),
        "AbortError" => format!("{p}.aborted"),
        "UnknownError" | "NotReadableError" | "OperationError" => format!("{p}.storage"),
        "TypeError" | "DataError" | "EncodingError" => format!("{p}.bad_options"),
        _ => format!("{p}.unknown"),
    }
}

/// Замер длительности вызова WebAuthn.
pub struct Clock(f64);

impl Clock {
    pub fn start() -> Self {
        Clock(js_sys::Date::now())
    }
    pub fn elapsed_ms(&self) -> f64 {
        js_sys::Date::now() - self.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::services::i18n::translated_everywhere;

    /// Имена, которые WebAuthn действительно выдаёт, плюс пустое (не-DOMException).
    const NAMES: &[&str] = &[
        "NotAllowedError",
        "InvalidStateError",
        "NotSupportedError",
        "SecurityError",
        "ConstraintError",
        "AbortError",
        "UnknownError",
        "NotReadableError",
        "OperationError",
        "TypeError",
        "DataError",
        "EncodingError",
        "",
        "СовершенноНовоеИмя",
    ];

    #[test]
    fn kazhdyj_klyuch_perevedyon_na_oba_yazyka() {
        for op in [Op::Create, Op::Get] {
            for name in NAMES {
                for (elapsed, timeout) in [(10.0, Some(60_000.0)), (5_000.0, Some(60_000.0)), (59_000.0, Some(60_000.0)), (5_000.0, None)] {
                    for offline in [false, true] {
                        let key = key_for(op, name, elapsed, timeout, offline);
                        assert!(
                            translated_everywhere(&key),
                            "нет перевода для {key} (op={op:?}, name={name})"
                        );
                    }
                }
            }
        }
    }

    #[test]
    fn otkaz_bez_seti_nazyvaetsya_setyu_a_ne_otmenoj() {
        assert_eq!(
            key_for(Op::Create, "NotAllowedError", 5_000.0, Some(60_000.0), true),
            "pk.offline"
        );
    }

    #[test]
    fn mgnovennyj_otkaz_ne_schitaetsya_otmenoj_polzovatelya() {
        // Системный лист нельзя увидеть и закрыть за 40 мс.
        assert_eq!(
            key_for(Op::Create, "NotAllowedError", 40.0, Some(60_000.0), false),
            "pk.create.blocked"
        );
    }

    #[test]
    fn otkaz_na_iskhode_tajmauta_schitaetsya_tajmautom() {
        assert_eq!(
            key_for(Op::Get, "NotAllowedError", 58_000.0, Some(60_000.0), false),
            "pk.get.timeout"
        );
    }

    #[test]
    fn otkaz_posle_razdumij_cheloveka_schitaetsya_otmenoj() {
        assert_eq!(
            key_for(Op::Create, "NotAllowedError", 4_000.0, Some(60_000.0), false),
            "pk.create.cancelled"
        );
    }

    #[test]
    fn povtornyj_klyuch_i_otsutstvie_klyucha_raznye_soobshcheniya() {
        assert_eq!(
            key_for(Op::Create, "InvalidStateError", 100.0, None, false),
            "pk.create.exists"
        );
        assert_eq!(
            key_for(Op::Get, "InvalidStateError", 100.0, None, false),
            "pk.get.no_key"
        );
    }
}
