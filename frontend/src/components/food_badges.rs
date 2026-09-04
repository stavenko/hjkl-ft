//! Строчка КБЖУ пилюлями — ОДНИМ куском на дневник и на правку разобранной записи.
//!
//! Позиции разобранной записи человек видит в двух местах: в дневнике внутри
//! строки и в форме правки. Это одна и та же еда с одними и теми же граммами, и
//! выглядеть она обязана одинаково — иначе правка читается как другая сущность.
//! Разметка жила только в `diary.rs`; здесь она вынута, чтобы второй экран не
//! получил свою копию, которая со временем разъедется.

use api_types::Food;
use leptos::*;

use crate::services::i18n;

/// Четыре пилюли по еде и доле съеденного.
///
/// `factor` — сколько сотен граммов съедено: `(граммы − несъеденное) / 100`.
/// Считает его зовущий, потому что только он знает про отходы: у позиции
/// разобранной записи их нет вовсе.
///
/// Калории берутся у `effective_kcal` — с ресторанной надбавкой, если еда так
/// помечена. Свои нутриенты (кальций и прочие) сюда НЕ идут намеренно: пятая
/// пилюля в строку не влезает и просто обрезается.
pub fn nutrient_badges(food: &Food, factor: f64) -> View {
    let badges = [
        // У калорий единицу не пишем: буква «К» уже значит калории, а «ккал»
        // широкое — с ним четыре пилюли не помещаются в строку.
        (i18n::nutrient_badge("Calories"), food.effective_kcal() * factor, ""),
        (i18n::nutrient_badge("Protein"), food.protein * factor, i18n::unit_label("g")),
        (i18n::nutrient_badge("Fat"), food.fat * factor, i18n::unit_label("g")),
        (i18n::nutrient_badge("Carbs"), food.carbs * factor, i18n::unit_label("g")),
    ];
    badges
        .into_iter()
        .map(|(label, value, unit)| {
            let unit = unit.to_string();
            view! {
                <span class="tag is-small"
                    style="white-space: nowrap; flex-shrink: 0; margin: 0; padding-left: 6px; padding-right: 6px;">
                    {format!("{} {:.0}", label, value)}
                    {(!unit.is_empty()).then(|| view! {
                        <span class="has-text-grey-light" style="margin-left: 2px;">{unit}</span>
                    })}
                </span>
            }
            .into_view()
        })
        .collect::<Vec<_>>()
        .into_view()
}

/// Обёртка строки пилюль: одна строка, без переноса, лишнее обрезается.
pub const BADGE_ROW: &str =
    "display: flex; flex-wrap: nowrap; gap: 4px; margin-top: 4px; min-width: 0; overflow: hidden;";
