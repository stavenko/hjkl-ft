//! Планки, которые задал КУРАТОР, и его же запреты автопересчёта.
//!
//! Устройство простое и держится на одном правиле: **действующая планка
//! выбирается приоритетом**. Есть запись куратора — берётся она; нет — работает
//! прежнее правило приложения (недельный пересчёт, вывод из калорийной планки,
//! константа). Поэтому здесь хранятся ТОЛЬКО кураторские значения, и «вернуть
//! как было» после отвязки — это просто стереть записи, а не вспоминать, чьё
//! какое число.
//!
//! Читается СИНХРОННО: `veg_fruit_per_day_g`, `Fat::target`, `get_steps_planka` и
//! прочие точки, куда приоритет обязан попасть, — обычные функции без `await`.
//! Отсюда кэш в памяти, как у [`crate::services::profile`], который гидратируется
//! при каждой смене активной базы (запуск, вход, привязка устройства).
//!
//! Store синкается: планка, поставленная куратором, обязана быть на всех
//! устройствах человека, а не только на том, где приложение было открыто.

use std::cell::RefCell;
use std::collections::HashMap;

use api_types::CuratorPlankaRow;
use leptos::{RwSignal, SignalUpdate};

use crate::services::db;

pub const STORE: &str = "curator_plankas";

// Ключи индикаторов, у которых замок вообще осмыслен, — то есть у выводимых
// величин. У констант пересчитывать нечего, и переключатель там был бы враньём.
pub const RECOMPUTED: &[&str] = &["calories", "steps", "protein", "veg_fruit", "iron", "fiber"];

/// Осмыслен ли запрет автопересчёта у этого индикатора.
pub fn is_recomputed(key: &str) -> bool {
    RECOMPUTED.contains(&key)
}

thread_local! {
    static CACHE: RefCell<HashMap<String, CuratorPlankaRow>> = RefCell::new(HashMap::new());
    /// Бампается при любой правке, чтобы шкалы и индикаторы перерисовались.
    static VERSION: RefCell<Option<RwSignal<u32>>> = const { RefCell::new(None) };
}

/// Создать корневой сигнал. Один раз, из `main()`, ДО первой гидратации.
pub fn init() {
    VERSION.with(|c| *c.borrow_mut() = Some(leptos::create_rw_signal(0u32)));
}

/// Сигнал, на который подписывается интерфейс: синхронные геттеры ниже сами по
/// себе не реактивны.
pub fn version_signal() -> RwSignal<u32> {
    VERSION.with(|c| c.borrow().expect("curator_plankas::init() must run first"))
}

fn bump() {
    if let Some(sig) = VERSION.with(|c| *c.borrow()) {
        sig.update(|v| *v += 1);
    }
}

/// Загрузить записи активной базы в синхронный кэш. Зовётся после смены активной
/// базы — там же, где гидратируется профиль.
pub async fn hydrate() {
    let rows: Vec<CuratorPlankaRow> = db::list_all(STORE).await;
    let map: HashMap<String, CuratorPlankaRow> =
        rows.into_iter().map(|r| (r.key.clone(), r)).collect();
    let changed = CACHE.with(|c| {
        let mut cur = c.borrow_mut();
        if cur.len() == map.len() && cur.iter().all(|(k, v)| {
            map.get(k).map(|m| m.amount == v.amount && m.locked == v.locked).unwrap_or(false)
        }) {
            return false;
        }
        *cur = map;
        true
    });
    // Планка, приехавшая СИНКОМ с другого устройства, обязана показаться сразу:
    // шкалы читают кэш синхронно и без этого держали бы доснковое состояние до
    // следующего запуска.
    if changed {
        bump();
    }
}

/// Кураторское значение планки, если куратор его задал.
pub fn get(key: &str) -> Option<f64> {
    CACHE.with(|c| c.borrow().get(key).and_then(|r| r.amount))
}

/// Запретил ли куратор автопересчёт этой планки.
pub fn locked(key: &str) -> bool {
    CACHE.with(|c| c.borrow().get(key).map(|r| r.locked).unwrap_or(false))
}

/// Все записи — для отчёта куратору и для отвязки.
pub fn all() -> Vec<CuratorPlankaRow> {
    let mut v: Vec<CuratorPlankaRow> = CACHE.with(|c| c.borrow().values().cloned().collect());
    v.sort_by(|a, b| a.key.cmp(&b.key));
    v
}

/// Приоритет одной строкой: кураторское значение поверх нашего.
///
/// Ровно это выражение и есть всё правило выбора планки; каждая точка чтения
/// оборачивает им своё прежнее вычисление.
pub fn or_ours(key: &str, ours: f64) -> f64 {
    get(key).unwrap_or(ours)
}

/// То же для величин, которых может не быть вовсе (планка шагов до открытия темы).
pub fn or_ours_opt(key: &str, ours: Option<f64>) -> Option<f64> {
    get(key).or(ours)
}

/// Записать кураторскую планку. `amount = None` — куратор тронул только замок,
/// число остаётся нашим.
pub async fn set(key: &str, amount: Option<f64>, locked_flag: bool) {
    let row = CuratorPlankaRow {
        key: key.to_string(),
        amount,
        locked: locked_flag,
        updated_at: chrono::Utc::now().to_rfc3339(),
    };
    CACHE.with(|c| {
        c.borrow_mut().insert(key.to_string(), row.clone());
    });
    db::put(STORE, &row).await;
    bump();
}

/// Стереть одну кураторскую планку — вернуть индикатор нашему правилу.
pub async fn clear(key: &str) {
    CACHE.with(|c| {
        c.borrow_mut().remove(key);
    });
    db::delete(STORE, key).await;
    bump();
}

/// Стереть ВСЕ кураторские планки. Это отвязка: наши правила возвращаются
/// целиком. Перенос значений, которые обязаны дожить до ближайшего пересчёта
/// (калории и шаги), делает вызывающая сторона ДО этого — здесь их уже не будет.
pub async fn clear_all() {
    let keys: Vec<String> = CACHE.with(|c| c.borrow().keys().cloned().collect());
    CACHE.with(|c| c.borrow_mut().clear());
    for k in keys {
        db::delete(STORE, &k).await;
    }
    bump();
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Положить запись прямо в кэш, минуя базу: правило выбора проверяется
    /// само по себе, без IndexedDB.
    fn seed(key: &str, amount: Option<f64>, locked_flag: bool) {
        CACHE.with(|c| {
            c.borrow_mut().insert(
                key.to_string(),
                CuratorPlankaRow {
                    key: key.to_string(),
                    amount,
                    locked: locked_flag,
                    updated_at: String::new(),
                },
            );
        });
    }

    #[test]
    fn kuratorskoe_znachenie_pobezhdaet_nashe() {
        seed("calories", Some(1800.0), false);
        assert_eq!(or_ours("calories", 2100.0), 1800.0);
        // Индикатор, которого куратор не трогал, остаётся на нашем правиле.
        assert_eq!(or_ours("calcium", 1000.0), 1000.0);
    }

    #[test]
    fn zamok_bez_znacheniya_ne_menyaet_chislo() {
        // Куратор запретил пересчёт, но число оставил наше: планка не меняется,
        // а вот недельный цикл её больше не двигает.
        seed("steps", None, true);
        assert_eq!(or_ours("steps", 10_000.0), 10_000.0);
        assert!(locked("steps"));
        assert!(!locked("calories"));
    }

    #[test]
    fn planki_kotoroj_u_nas_net_vovse_hvataet_kuratorskoj() {
        // Планка шагов до открытия темы — None. Кураторская её задаёт.
        seed("steps", Some(9000.0), false);
        assert_eq!(or_ours_opt("steps", None), Some(9000.0));
    }

    #[test]
    fn zamok_osmyslen_tolko_u_vyvodimyh() {
        for k in ["calories", "steps", "protein", "veg_fruit", "iron", "fiber"] {
            assert!(is_recomputed(k), "{k} выводится и замок ему нужен");
        }
        // Константы: пересчитывать нечего, переключатель был бы враньём.
        for k in ["calcium", "epa_dha", "fat_ratio", "red_meat", "heme", "egg"] {
            assert!(!is_recomputed(k), "{k} — константа, замок ей ни к чему");
        }
    }
}
