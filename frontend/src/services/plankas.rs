//! Планки: одно место, одно правило.
//!
//! **Действующая планка — это запись в истории.** Есть запись — она; нет —
//! работает наше правило по умолчанию из общего крейта. Больше нигде планка не
//! живёт: ни `goals`, ни `ProfileRow.steps_planka` источником не являются (они
//! остались тем, чем всегда были на деле, — списком отслеживаемых нутриентов и
//! профилем), и приоритетов «чьё число главнее» здесь нет.
//!
//! Одно исключение, и оно про ПРОШЛОЕ, а не про правила: если истории по шагам
//! нет ВОВСЕ, кэш заводится от поля в профиле — старого места, куда планка
//! пишется до сих пор ради прежних сборок; см. [`hydrate`]. Так
//! выглядит человек, чья планка старше самой истории, и тот, у кого журнал смыло.
//! Пустая история у него значит «журнал пуст», а не «планки нет», и спутать эти
//! два состояния нельзя: во втором приложение ставит планку ВПЕРВЫЕ, заменяя его
//! число своим.
//!
//! Это заменило прежнюю затею с отдельным кураторским хранилищем. Та порождала
//! вопросы без хорошего ответа: что значит кураторское число без замка, от чего
//! отталкивается пересчёт, когда действующее число одно, а наше лежит рядом.
//! Одна запись на вид снимает их все.
//!
//! Читается СИНХРОННО: `veg_fruit_per_day_g`, `Fat::target`, `get_steps_planka` —
//! обычные функции без `await`. Отсюда кэш в памяти, как у [`super::profile`],
//! гидратируемый при смене активной базы и после синка.
//!
//! **Инвариант:** для девяти константных видов запись может появиться ТОЛЬКО от
//! куратора — приложение их никогда не пишет. Поэтому «стереть кураторское» —
//! это [`forget`], и различать авторство не нужно.

use std::cell::RefCell;
use std::collections::HashMap;

use leptos::{RwSignal, SignalUpdate};

pub use plankas::{Kind, Snapshot, ALL};

use crate::services::{local, profile};

thread_local! {
    /// Записанные значения по видам — то, что лежит в истории на сегодня.
    static RECORDED: RefCell<HashMap<&'static str, f64>> = RefCell::new(HashMap::new());
    /// Последний вес. Живёт здесь, а не в [`super::profile`], потому что нужен
    /// ровно для [`snapshot`]: от него зависят наши правила по белку и железу, а
    /// читаются они синхронно. В профиле веса нет — он в своём store и до сих пор
    /// доставался только через `await`.
    static LAST_WEIGHT: RefCell<Option<f64>> = const { RefCell::new(None) };
    static VERSION: RefCell<Option<RwSignal<u32>>> = const { RefCell::new(None) };
}

/// Создать корневой сигнал. Один раз, из `main()`, ДО первой гидратации.
pub fn init() {
    VERSION.with(|c| *c.borrow_mut() = Some(leptos::create_rw_signal(0u32)));
}

/// Сигнал, на который подписывается интерфейс: геттеры ниже сами по себе не
/// реактивны.
pub fn version_signal() -> RwSignal<u32> {
    VERSION.with(|c| c.borrow().expect("plankas::init() must run first"))
}

fn bump() {
    if let Some(sig) = VERSION.with(|c| *c.borrow()) {
        sig.update(|v| *v += 1);
    }
}

/// Перечитать историю в синхронный кэш. Зовётся после смены активной базы и после
/// синка: планка, поставленная куратором с другого устройства, обязана показаться
/// сразу, а не после перезапуска.
pub async fn hydrate() {
    let today = local::today_date().format("%Y-%m-%d").to_string();
    let mut map = HashMap::new();
    for kind in ALL {
        if let Some(v) = local::planka_on(kind.key(), &today).await {
            map.insert(kind.key(), v);
        }
    }
    // То же по шагам: их планка до истории жила в профиле, и поле там до сих пор
    // пишется — ради старых сборок на других устройствах человека. Пустая история
    // с заполненным полем значит ровно то же: журнал пуст, а планка есть.
    if !map.contains_key(Kind::Steps.key()) {
        if let Some(v) = profile::legacy_steps_planka() {
            map.insert(Kind::Steps.key(), v);
        }
    }
    let weight = local::list_weight_entries().await.last().map(|e| e.weight_kg);
    let changed = RECORDED.with(|c| {
        let mut cur = c.borrow_mut();
        if *cur == map {
            return false;
        }
        *cur = map;
        true
    }) | LAST_WEIGHT.with(|c| {
        let mut cur = c.borrow_mut();
        if *cur == weight {
            return false;
        }
        *cur = weight;
        true
    });
    if changed {
        bump();
    }
}

/// Новое взвешивание. Зовётся из [`local::save_weight`]: правила по белку и железу
/// идут за весом, и ждать следующей гидратации они не должны.
pub fn note_weight(weight_kg: f64) {
    let changed = LAST_WEIGHT.with(|c| {
        let mut cur = c.borrow_mut();
        if *cur == Some(weight_kg) {
            return false;
        }
        *cur = Some(weight_kg);
        true
    });
    if changed {
        bump();
    }
}

/// Последний записанный вес, синхронно.
pub fn last_weight_kg() -> Option<f64> {
    LAST_WEIGHT.with(|c| *c.borrow())
}

/// Снимок человека, от которого зависят наши правила. Собирается из профиля и из
/// действующей калорийной планки — той же, что вернёт [`current`].
pub fn snapshot() -> Snapshot {
    Snapshot {
        goal: profile::planka_goal(),
        sex: profile::get_sex(),
        age_years: profile::get_age_years(),
        height_cm: profile::get_height_cm(),
        weight_kg: last_weight_kg(),
        kcal_planka: recorded(Kind::Calories),
    }
}

/// Записанное значение, если оно есть. Без правила по умолчанию.
pub fn recorded(kind: Kind) -> Option<f64> {
    RECORDED.with(|c| c.borrow().get(kind.key()).copied())
}

/// ДЕЙСТВУЮЩАЯ планка: запись, а если её нет — наше правило.
pub fn current(kind: Kind) -> Option<f64> {
    recorded(kind).or_else(|| plankas::default_for(kind, &snapshot()))
}

/// Действующая планка вида, у которого наше правило есть ВСЕГДА, — девяти
/// константных. Для калорий, шагов и белка правила может не быть (цикл их ещё не
/// вёл, профиль неполон), и им нужен [`current`] с его `Option`.
pub fn constant(kind: Kind) -> f64 {
    debug_assert!(!kind.is_dynamic(), "{}: правило есть не всегда", kind.key());
    current(kind).unwrap_or_default()
}

/// Планка, действовавшая в этот день. Прошлое судится по своей планке, а не по
/// сегодняшней, поэтому здесь читается ИСТОРИЯ, а не кэш.
pub async fn on(kind: Kind, date: &str) -> Option<f64> {
    match local::planka_on(kind.key(), date).await {
        Some(v) => Some(v),
        // Записи на тот день не было — работало правило. Считаем его по
        // СЕГОДНЯШНЕМУ снимку: восстановить профиль на ту дату всё равно нечем, а
        // нормы от него почти не двигаются.
        None => plankas::default_for(kind, &snapshot()),
    }
}

/// Установить планку с сегодняшнего дня.
///
/// Пределы проверяются ЗДЕСЬ, в единственной двери: число приходит и из чужого
/// приложения (директива куратора), и от недельного цикла.
pub async fn set(kind: Kind, amount: f64) {
    if !kind.accepts(amount) {
        leptos::logging::error!("планка {}: значение {amount} вне пределов", kind.key());
        return;
    }
    // Кэш обновит сама `record_planka` — она единственная дверь в историю, и
    // пишут в неё не только отсюда.
    local::record_planka(kind.key(), amount).await;
}

/// В историю записали планку — обновить синхронный кэш. Зовётся из
/// [`local::record_planka`]; напрямую не нужна.
pub(super) fn note_recorded(key: &str, amount: f64) {
    let Some(kind) = Kind::from_key(key) else {
        return;
    };
    let changed = RECORDED.with(|c| c.borrow_mut().insert(kind.key(), amount) != Some(amount));
    if changed {
        bump();
    }
}

/// Забыть записи этого вида — вернуть наше правило.
///
/// Стирается ИСТОРИЯ вида целиком. Для девяти константных это ровно кураторское
/// (приложение их не пишет), а прошлое по ним до этой задачи и не хранилось — они
/// судились по текущей константе, к чему мы и возвращаемся.
pub async fn forget(kind: Kind) {
    local::forget_planka_history(kind.key()).await;
    RECORDED.with(|c| {
        c.borrow_mut().remove(kind.key());
    });
    bump();
}

#[cfg(test)]
mod tests {
    use super::*;

    fn seed(kind: Kind, amount: f64) {
        RECORDED.with(|c| {
            c.borrow_mut().insert(kind.key(), amount);
        });
    }

    fn forget_all() {
        RECORDED.with(|c| c.borrow_mut().clear());
    }

    /// Главное правило: запись побеждает, без записи работает наше.
    #[test]
    fn zapis_pobezhdaet_pravilo() {
        forget_all();
        assert_eq!(current(Kind::Calcium), Some(plankas::defaults::CALCIUM_PER_DAY_MG));
        seed(Kind::Calcium, 1200.0);
        assert_eq!(current(Kind::Calcium), Some(1200.0));
    }

    /// Калорийная планка живёт в той же истории и питает снимок — значит правила,
    /// которые идут ЗА ней, двигаются вместе с ней. Ровно ради этого планки и
    /// собраны в одно место.
    #[test]
    fn kletchatka_idyot_za_zapisannymi_kaloriyami() {
        forget_all();
        seed(Kind::Calories, 1500.0);
        // 1500 ккал дали бы 21 г — держим минимум ВОЗ.
        assert_eq!(current(Kind::Fiber), Some(plankas::defaults::MIN_G_PER_DAY));
        seed(Kind::Calories, 3000.0);
        assert_eq!(current(Kind::Fiber), Some(42.0));
    }

    /// Калорий и шагов без записи НЕ СУЩЕСТВУЕТ: их ведёт недельный цикл, и
    /// выдумывать их из профиля было бы враньём.
    #[test]
    fn kalorii_i_shagi_bez_zapisi_ne_vydumyvayutsya() {
        forget_all();
        assert_eq!(current(Kind::Calories), None);
        assert_eq!(current(Kind::Steps), None);
        seed(Kind::Steps, 9000.0);
        assert_eq!(current(Kind::Steps), Some(9000.0));
    }

    /// Кэш обновляется только по известным ключам: чужой ключ из директивы не
    /// должен молча оседать планкой.
    #[test]
    fn chuzhoy_klyuch_ne_stanovitsya_plankoy() {
        forget_all();
        note_recorded("processed_meat", 300.0);
        assert!(RECORDED.with(|c| c.borrow().is_empty()));
    }
}
