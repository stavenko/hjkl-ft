//! Кнопка «⋮» и выпадающее меню строки — ОДНИМ куском на все строки дневника.
//!
//! Раньше их было две: обычная строка рисовала кебаб из трёх кружков, а ленивая —
//! знак «⋯» другой кнопкой и другого размера. Человек видит один список, и меню в
//! нём обязано открываться одинаково; расхождение здесь читается как «эти строки
//! чем-то другие», хотя это просто еда, записанная иначе.

use leptos::*;

/// Значок «⋮». Три кружка, а не многоточие: многоточие в тексте значит
/// «продолжение», а здесь это кнопка.
pub fn kebab_icon() -> impl IntoView {
    view! {
        <svg xmlns="http://www.w3.org/2000/svg" width="20" height="20" viewBox="0 0 20 20" fill="currentColor">
            <circle cx="10" cy="4" r="1.6"/>
            <circle cx="10" cy="10" r="1.6"/>
            <circle cx="10" cy="16" r="1.6"/>
        </svg>
    }
}

/// Классы и размеры кнопки-кебаба. Строкой, а не компонентом: обработчик нажатия у
/// каждой строки свой (обычная держит открытое меню по идентификатору записи,
/// ленивая — своим сигналом), а одинаковыми должны быть вид и площадь под палец.
pub const KEBAB_CLASS: &str = "button is-ghost has-text-grey-light";
pub const KEBAB_STYLE: &str = "height: 2.5rem; width: 2.5rem; padding: 0; text-decoration: none;";

/// Обёртка выпадающего меню — та же тень, тот же угол, та же ширина.
pub const MENU_STYLE: &str = "position: absolute; right: 0; top: 100%; z-index: 10; \
     background: var(--bulma-scheme-main); border-radius: 6px; \
     box-shadow: 0 2px 12px rgba(0,0,0,0.15); min-width: 10rem; padding: 0.25rem 0;";

/// Пункт меню.
pub const ITEM_CLASS: &str = "button is-ghost is-small is-fullwidth";
pub const ITEM_STYLE: &str = "justify-content: flex-start; text-decoration: none;";
