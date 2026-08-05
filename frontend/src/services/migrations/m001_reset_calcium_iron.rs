//! Стереть кальций и железо у всех продуктов.
//!
//! Значения, набранные до 2026-08-05, недостоверны. Железо приходило равным доле
//! усвоения — печень 0.25 мг вместо ~9, чечевица 0.05 вместо ~7.5: доли усвоения
//! были единственными числами в промпте, и модель списывала их в поле миллиграммов.
//! Кальций часть времени вообще не записывался (модель отвечала документом схемы
//! вместо значения), а когда записывался — половина ответов ложилась в коридор
//! 120–135 мг независимо от продукта.
//!
//! Отличить испорченное от верного задним числом не по чему, поэтому стирается всё
//! разом. После стирания `enrich::needs_enrichment` и `iron::needs_iron` снова дают
//! `true`, и ближайший фоновый проход запросит значения заново — уже по таблицам
//! категорий. КБЖУ, теги, дневник, вес и цели не трогаются.

use api_types::Food;

use crate::services::{db, indicators, local, sync};

pub const VERSION: u32 = 1;
pub const DESCRIPTION: &str = "стереть кальций и железо, набранные испорченными промптами";

pub async fn script() -> Result<(), String> {
    let mut cleared = 0usize;
    for mut food in local::list_foods().await {
        let had_ca = food.nutrients.remove(indicators::N_CALCIUM).is_some();
        // Ключ «Железо» в карте нутриентов писали ранние сборки. В интерфейсе он
        // скрыт, железо живёт в собственных полях — в данных он только мешает.
        let had_legacy = food.nutrients.remove(indicators::N_IRON).is_some();
        let had_fe = food.iron_mg.is_some() || food.iron_absorption.is_some();
        if !had_ca && !had_legacy && !had_fe {
            continue;
        }
        food.iron_mg = None;
        food.iron_absorption = None;
        food.updated_at = local::now();
        db::put::<Food>("foods", &food).await;
        // Дни, в которые этот продукт ели, обязаны пересчитаться — иначе виджет
        // продолжит показывать кальций по стёртым значениям.
        indicators::invalidate_food(&food.id).await;
        cleared += 1;
    }
    leptos::logging::log!("миграция 1: очищено продуктов — {cleared}");
    if cleared > 0 {
        sync::push_background();
    }
    Ok(())
}
