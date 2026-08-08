//! Журнал ошибок фоновой работы — тот, что человек видит под треугольником.
//!
//! Ошибки складываются в КОРНЕВОЙ сигнал (создаётся один раз в `main`), поэтому
//! любой виджет может смотреть, есть ли они, и перечислять их. Только в памяти:
//! перезагрузка очищает, фоновый проход повторится и сообщит снова, если беда
//! никуда не делась.
//!
//! Что сюда попадает, а что нет: здесь место тому, о чём человеку осмысленно
//! знать — «про этот продукт данные могут быть неполными». Наши внутренние сбои
//! (не смогли открыть базу, не сошлась схема) сюда НЕ идут: сделать он с ними
//! ничего не может, а испугается. Такое уходит в консоль и в телеметрию.

use std::cell::RefCell;

use leptos::*;

/// Что именно не удалось выяснить про продукт.
///
/// Отдельным типом, а не строкой: из него собирается и фраза человеку, и код
/// события для телеметрии, и они обязаны не разъезжаться.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum FoodAspect {
    /// Тип продукта — перекус, жидкие калории, овощ-фрукт.
    Kind,
    /// Кальций, омега-3, клетчатка — общий проход по нутриентам.
    Nutrients,
    /// Железо — свой проход, со своей долей усвоения.
    Iron,
    /// Жирные кислоты — свой проход, профиль жира одним запросом.
    Fats,
}

impl FoodAspect {
    /// Как назвать это человеку в винительном падеже: «не получилось определить …».
    fn accusative(self) -> &'static str {
        match self {
            FoodAspect::Kind => "тип продукта",
            FoodAspect::Nutrients => "кальций и омегу",
            FoodAspect::Iron => "железо",
            FoodAspect::Fats => "жирные кислоты",
        }
    }

    /// Короткое имя для телеметрии — стабильное, не зависит от языка интерфейса.
    pub fn slug(self) -> &'static str {
        match self {
            FoodAspect::Kind => "kind",
            FoodAspect::Nutrients => "nutrients",
            FoodAspect::Iron => "iron",
            FoodAspect::Fats => "fats",
        }
    }
}

/// Одна записанная ошибка.
#[derive(Clone, PartialEq, Eq)]
pub struct AppError {
    /// Заголовок строки в списке.
    pub context: String,
    /// Подробность под заголовком.
    pub message: String,
    /// Техническая причина — для телеметрии, человеку не показывается.
    /// `None` у записей, собранных не из сбоя (например, «нет сети»).
    pub cause: Option<String>,
    /// Вид события для телеметрии: `food.iron`, `planka.calories`, … Пустая
    /// строка — событие не телеметрируется.
    pub kind: String,
}

impl AppError {
    /// Однострочное представление для копирования.
    pub fn as_text(&self) -> String {
        match &self.cause {
            Some(c) => format!("{}\n{}\n{c}", self.context, self.message),
            None => format!("{}\n{}", self.context, self.message),
        }
    }
}

thread_local! {
    static ERRS: RefCell<Option<RwSignal<Vec<AppError>>>> = const { RefCell::new(None) };
}

/// Create the error-log signal at the ROOT scope. Call once from `main()`.
pub fn init() {
    ERRS.with(|c| {
        if c.borrow().is_none() {
            *c.borrow_mut() = Some(create_rw_signal(Vec::new()));
        }
    });
}

/// The reactive error list (observers re-render when it changes).
pub fn signal() -> RwSignal<Vec<AppError>> {
    ERRS.with(|c| c.borrow().expect("errors::init() must run before errors::signal()"))
}

/// Короткий устойчивый код сообщения (6 hex, FNV-1a): один и тот же сбой всегда
/// даёт один и тот же код, поэтому по нему можно считать, скольких он задел.
pub fn code_of(s: &str) -> String {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in s.bytes() {
        h ^= b as u64;
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
    }
    format!("{:06X}", h & 0xFF_FFFF)
}

/// Не удалось доразобрать продукт.
///
/// Человеку — что это за продукт и чего про него не хватит; чинить ему нечего,
/// поэтому и просить не о чем. `cause` — техническая причина, она уходит в
/// телеметрию и в копируемый текст, но не в саму фразу.
pub fn record_food(aspect: FoodAspect, food_name: &str, cause: &str) {
    push(AppError {
        context: food_name.to_string(),
        message: food_message(aspect, food_name),
        cause: Some(cause.to_string()),
        kind: format!("food.{}", aspect.slug()),
    });
}

/// Фраза, которую человек видит про недоразобранный продукт.
fn food_message(aspect: FoodAspect, food_name: &str) -> String {
    format!(
        "Возникли некоторые ошибки при работе с вашей едой. Это касается «{food_name}». \
         Возможно, информация об этом продукте будет неполной в ближайшее время: не \
         получилось определить {}. Наши инженеры уже знают об ошибке и работают над ней.",
        aspect.accusative()
    )
}

/// Record an error (deduplicated by context+message; keeps the most recent 50).
pub fn record(context: &str, message: &str) {
    push(AppError {
        context: context.to_string(),
        message: message.to_string(),
        cause: None,
        kind: String::new(),
    });
}

/// То же, но с видом события для телеметрии.
pub fn record_kind(kind: &str, context: &str, message: &str) {
    push(AppError {
        context: context.to_string(),
        message: message.to_string(),
        cause: None,
        kind: kind.to_string(),
    });
}

fn push(e: AppError) {
    // Guard: if init() hasn't run yet, drop silently rather than panic.
    let has = ERRS.with(|c| c.borrow().is_some());
    if !has {
        return;
    }
    let fresh = signal().with_untracked(|v| !v.iter().any(|x| *x == e));
    if !fresh {
        return;
    }
    if !e.kind.is_empty() {
        super::telemetry::report(&e);
    }
    signal().update(|v| {
        v.push(e);
        if v.len() > 50 {
            v.remove(0);
        }
    });
}

/// Clear all recorded errors.
pub fn clear() {
    if ERRS.with(|c| c.borrow().is_some()) {
        signal().update(|v| v.clear());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fraza_nazyvaet_produkt_i_chego_ne_hvataet() {
        let m = food_message(FoodAspect::Iron, "Голец");
        assert!(m.contains("«Голец»"), "{m}");
        assert!(m.contains("не получилось определить железо"), "{m}");
        assert!(m.contains("Наши инженеры уже знают"), "{m}");
        // Технической причины в тексте человеку быть не должно.
        assert!(!m.contains("error"), "{m}");
    }

    #[test]
    fn u_kazhdogo_vida_svoyo_nazvanie() {
        assert!(food_message(FoodAspect::Kind, "Х").contains("тип продукта"));
        assert!(food_message(FoodAspect::Nutrients, "Х").contains("кальций и омегу"));
        assert!(food_message(FoodAspect::Iron, "Х").contains("железо"));
    }

    #[test]
    fn vid_sobytiya_ne_zavisit_ot_yazyka() {
        assert_eq!(FoodAspect::Kind.slug(), "kind");
        assert_eq!(FoodAspect::Nutrients.slug(), "nutrients");
        assert_eq!(FoodAspect::Iron.slug(), "iron");
    }

    #[test]
    fn kod_ne_zavisit_ot_produkta_no_zavisit_ot_prichiny() {
        // Одна поломка на сотне продуктов обязана сложиться в один код.
        assert_eq!(code_of("food.iron|timeout"), code_of("food.iron|timeout"));
        assert_ne!(code_of("food.iron|timeout"), code_of("food.iron|bad json"));
    }
}
