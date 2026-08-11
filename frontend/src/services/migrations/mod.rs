//! Миграции локальной базы.
//!
//! Одна миграция — один файл в этом каталоге, с тремя публичными полями:
//!
//! ```ignore
//! pub const VERSION: u32 = 3;
//! pub const DESCRIPTION: &str = "что и зачем делает";
//! pub async fn script() -> Result<(), String> { … }
//! ```
//!
//! Файл добавляется в [`ALL`] — единственное место, где перечислены миграции.
//!
//! # Правила
//!
//! * Нумерация начинается с НУЛЯ и не имеет пропусков.
//! * Миграции идут строго по возрастанию версии: сначала 0, потом 1, потом 2.
//! * Скрипт прогоняется, только если версия базы МЕНЬШЕ его собственной. На базу
//!   версии 4 накатится скрипт 5; скрипт 5 на базе версии 6 будет пропущен.
//! * Каждый успешный прогон поднимает версию базы до версии скрипта.
//! * Версия проверяется перед КАЖДЫМ скриптом отдельно, а не один раз на весь
//!   проход.
//! * Ошибка внутри скрипта ОБРЫВАЕТ весь дальнейший процесс: следующие миграции
//!   рассчитывают на состояние, которого теперь нет. Ошибка уходит в журнал
//!   приложения — тот, что человек видит под треугольником.
//!
//! # Когда это запускается
//!
//! В работающем приложении, когда обновление не ждёт: миграция до обновления
//! накатила бы старый код на данные, которых новая сборка ждёт другими, и та
//! застала бы базу в промежуточном виде. Условие держит [`crate::app`].
//!
//! Онбординг НЕ исключается. Отличить его надёжно нечем: `AppState::Ready`
//! выдаётся и ему (чтобы страницу не накрыли оверлеи), а путь `/onboard` роутер
//! переписывает на `/` за миллисекунды. Вместо запрета — учёт: миграции, прошедшие
//! на гостевой базе, ничего не портят, потому что при входе `activate_for_user`
//! вызывается с `migrate_bootstrap = false` — гостевая база не копируется, версия
//! из неё не переезжает, и в базе пользователя миграции проходят заново, уже на
//! настоящих данных. Чтобы это «заново» действительно случилось, отметка о прогоне
//! привязана к ИМЕНИ БАЗЫ, а не к сессии — см. [`run_for_current_db`].

use std::cell::RefCell;
use std::future::Future;
use std::pin::Pin;

use super::{app_flags, db, errors};

mod m000_init;
mod m001_reset_calcium_iron;
mod m002_drop_bootstrap_db;
mod m003_seed_planka_history;
mod m004_reset_iron;
mod m005_recompute_recipes;
mod m006_reset_heme;
mod m007_drop_omega3;
mod m008_refix_fat_gate;
mod m009_protein_from_calories;
mod m010_drop_dead_flags;
mod m011_recheck_fish_calcium;

#[cfg(test)]
mod tests;

/// Где хранится версия базы. Ключ device-local: миграции прогоняет КАЖДОЕ
/// устройство у себя, поэтому версия не должна ездить по синку.
pub const VERSION_KEY: &str = "db_schema_version";

/// Версия базы «до нулевой миграции»: поля ещё нет вообще. Всё, что ниже нуля,
/// означает «не мигрировано ни разу».
const UNMIGRATED: i64 = -1;

type Script = fn() -> Pin<Box<dyn Future<Output = Result<(), String>>>>;

pub struct Migration {
    pub version: u32,
    pub description: &'static str,
    pub script: Script,
}

/// ВСЕ миграции, по возрастанию версии. Единственное место, где они перечислены.
const ALL: &[Migration] = &[
    Migration {
        version: m000_init::VERSION,
        description: m000_init::DESCRIPTION,
        script: || Box::pin(m000_init::script()),
    },
    Migration {
        version: m001_reset_calcium_iron::VERSION,
        description: m001_reset_calcium_iron::DESCRIPTION,
        script: || Box::pin(m001_reset_calcium_iron::script()),
    },
    Migration {
        version: m002_drop_bootstrap_db::VERSION,
        description: m002_drop_bootstrap_db::DESCRIPTION,
        script: || Box::pin(m002_drop_bootstrap_db::script()),
    },
    Migration {
        version: m003_seed_planka_history::VERSION,
        description: m003_seed_planka_history::DESCRIPTION,
        script: || Box::pin(m003_seed_planka_history::script()),
    },
    Migration {
        version: m004_reset_iron::VERSION,
        description: m004_reset_iron::DESCRIPTION,
        script: || Box::pin(m004_reset_iron::script()),
    },
    Migration {
        version: m005_recompute_recipes::VERSION,
        description: m005_recompute_recipes::DESCRIPTION,
        script: || Box::pin(m005_recompute_recipes::script()),
    },
    Migration {
        version: m006_reset_heme::VERSION,
        description: m006_reset_heme::DESCRIPTION,
        script: || Box::pin(m006_reset_heme::script()),
    },
    Migration {
        version: m007_drop_omega3::VERSION,
        description: m007_drop_omega3::DESCRIPTION,
        script: || Box::pin(m007_drop_omega3::script()),
    },
    Migration {
        version: m008_refix_fat_gate::VERSION,
        description: m008_refix_fat_gate::DESCRIPTION,
        script: || Box::pin(m008_refix_fat_gate::script()),
    },
    Migration {
        version: m009_protein_from_calories::VERSION,
        description: m009_protein_from_calories::DESCRIPTION,
        script: || Box::pin(m009_protein_from_calories::script()),
    },
    Migration {
        version: m010_drop_dead_flags::VERSION,
        description: m010_drop_dead_flags::DESCRIPTION,
        script: || Box::pin(m010_drop_dead_flags::script()),
    },
    Migration {
        version: m011_recheck_fish_calcium::VERSION,
        description: m011_recheck_fish_calcium::DESCRIPTION,
        script: || Box::pin(m011_recheck_fish_calcium::script()),
    },
];

/// Пойдёт ли миграция версии `version` на базу версии `base`.
///
/// Единственное правило отбора: версия скрипта строго БОЛЬШЕ версии базы. На базу
/// версии 4 накатится скрипт 5; скрипт 5 на базе версии 6 будет пропущен; при
/// отсутствии версии ([`UNMIGRATED`]) пойдёт нулевой.
fn should_run(version: u32, base: i64) -> bool {
    (version as i64) > base
}

/// Версия базы, или [`UNMIGRATED`], если поля ещё нет.
fn current_version() -> i64 {
    app_flags::get(VERSION_KEY)
        .and_then(|s| s.trim().parse::<i64>().ok())
        .unwrap_or(UNMIGRATED)
}

fn set_version(v: u32) {
    app_flags::set(VERSION_KEY, &v.to_string());
}

/// Версия базы для отображения (например, в настройках).
pub fn version() -> i64 {
    current_version()
}

thread_local! {
    /// База, на которой миграции уже гоняли в этой сессии.
    static DONE_FOR: RefCell<Option<String>> = const { RefCell::new(None) };
}

/// Прогнать миграции для АКТИВНОЙ базы, если для неё их ещё не гоняли.
///
/// Отметка привязана к имени базы, а не к сессии, и это существенно: приложение
/// стартует на гостевой базе, а при входе переключается на базу пользователя. Общая
/// защёлка «один раз за сессию» оставила бы базу пользователя немигрированной —
/// именно её и надо мигрировать, гостевую же миграции проходят вхолостую.
///
/// Повторный вызов для той же базы ничего не делает. Возвращает число выполненных.
pub async fn run_for_current_db() -> usize {
    let Some(name) = db::current_name() else {
        // Базы ещё нет — мигрировать нечего и некуда.
        return 0;
    };
    if DONE_FOR.with(|c| c.borrow().as_deref() == Some(name.as_str())) {
        return 0;
    }
    DONE_FOR.with(|c| *c.borrow_mut() = Some(name.clone()));
    run_all().await
}

/// Прогнать все миграции, которые старше текущей версии базы.
///
/// Вызывать МОЖНО сколько угодно: миграция, чья версия не выше версии базы,
/// пропускается без работы. Возвращает число выполненных.
pub async fn run_all() -> usize {
    debug_assert!(
        ALL.windows(2).all(|w| w[1].version == w[0].version + 1) && ALL[0].version == 0,
        "версии миграций обязаны идти с нуля подряд, без пропусков и повторов"
    );

    let mut done = 0usize;
    for m in ALL {
        // Версия перечитывается ПЕРЕД КАЖДЫМ скриптом: предыдущий её только что
        // поднял, а на устройстве мог остаться и более поздний прогон.
        let base = current_version();
        if !should_run(m.version, base) {
            continue;
        }
        leptos::logging::log!(
            "миграция {}: {} (версия базы {base})",
            m.version,
            m.description
        );
        match (m.script)().await {
            Ok(()) => {
                set_version(m.version);
                done += 1;
            }
            Err(e) => {
                // Дальше идти нельзя: следующие миграции рассчитывают на состояние,
                // которого теперь нет. Версию НЕ поднимаем — при следующем запуске
                // эта же миграция попробует снова.
                errors::record(
                    &format!("Миграция базы {} — {}", m.version, m.description),
                    &format!("прервана: {e}. Следующие миграции не выполнялись."),
                );
                leptos::logging::error!("миграция {} прервана: {e}", m.version);
                crate::services::telemetry::report_internal(
                    "db.migration_failed",
                    &format!("{} {}", m.version, m.description),
                    &format!("прервана: {e}. Следующие миграции не выполнялись."),
                );
                return done;
            }
        }
    }
    done
}
