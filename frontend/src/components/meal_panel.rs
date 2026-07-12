use leptos::*;

use crate::services::i18n;

/// A collapsible meal panel: header (meal name + macro summary) over a body of
/// diary rows. Tapping the header toggles collapse.
///
/// Each meal carries a muted `accent` colour (a 6-digit `#rrggbb`) used for the
/// panel border and a tinted header band, so the header reads as a distinct
/// heading instead of blending into the first food row. `accent + "22"` /
/// `accent + "55"` are 8-digit-hex alpha variants (soft tint / divider).
///
/// The macro totals (kcal / protein / fat / carbs) are the aggregate for the
/// meal, computed by the caller and passed in. The rows themselves come in as
/// `children` so the diary page keeps ownership of their signals/handlers.
#[component]
pub fn MealPanel(
    /// Localised meal name (Завтрак / Обед / Ужин / …).
    title: String,
    /// Muted per-meal accent colour, a 6-digit `#rrggbb` hex.
    accent: String,
    /// Aggregate calories for the meal.
    kcal: f64,
    /// Aggregate protein (g).
    protein: f64,
    /// Aggregate fat (g).
    fat: f64,
    /// Aggregate carbs (g).
    carbs: f64,
    /// The meal's diary rows.
    children: Children,
) -> impl IntoView {
    // Default expanded. Internal state → toggling never re-runs the parent's
    // entries block, so the panel stays put while collapsing.
    let collapsed = create_rw_signal(false);

    let macro_line = format!(
        "{} {:.0} · {} {:.0} · {} {:.0} · {} {:.0}",
        i18n::nutrient_badge("Calories"), kcal,
        i18n::nutrient_badge("Protein"), protein,
        i18n::nutrient_badge("Fat"), fat,
        i18n::nutrient_badge("Carbs"), carbs,
    );

    let tint = format!("{accent}22"); // ~13% — soft header band
    let divider = format!("{accent}55"); // ~33% — header/body separator
    // NOTE: no `overflow: hidden` here — it would clip the diary rows' action
    // menu (e.g. «повторить сегодня»), which drops BELOW its row. Instead the
    // header rounds its own top corners so the tinted band respects the border.
    let container_style = format!(
        "background: var(--bulma-scheme-main); border: 1px solid {accent}; border-radius: 12px; margin-bottom: 0.75rem;"
    );
    // Header band tint stays; the divider under it appears only when expanded,
    // so a collapsed panel doesn't show a dangling underline. Round the top
    // corners (all four when collapsed, since then the header IS the whole panel)
    // to match the container now that it no longer clips.
    let header_style = {
        let tint = tint.clone();
        let divider = divider.clone();
        move || {
            let collapsed = collapsed.get();
            let radius = if collapsed { "11px" } else { "11px 11px 0 0" };
            format!(
                "width: 100%; height: auto; display: flex; align-items: center; justify-content: space-between; padding: 0.75rem 1rem; text-decoration: none; border: none; border-radius: {radius}; background: {tint}; {}",
                if collapsed { String::new() } else { format!("border-bottom: 1px solid {divider};") }
            )
        }
    };
    let title_style = format!("color: {accent};");

    view! {
        <div style=container_style>
            // Header — tap toggles collapse. Uses a <button> so iOS fires the
            // delegated click reliably (bare <div on:click> is dead on iOS).
            <button
                class="button is-ghost"
                style=header_style
                on:click=move |_| collapsed.update(|c| *c = !*c)
            >
                <div style="display: flex; flex-direction: column; align-items: flex-start; gap: 0.15rem; min-width: 0;">
                    <span class="is-size-6 has-text-weight-bold has-text-left" style=title_style>{title}</span>
                    <span class="is-size-7 has-text-grey has-text-left">{macro_line}</span>
                </div>
                // Chevron: points down when expanded, right when collapsed.
                <span style=move || format!(
                    "flex-shrink: 0; margin-left: 0.75rem; color: {accent}; transition: transform 0.2s; transform: rotate({}deg);",
                    if collapsed.get() { -90 } else { 0 }
                )>
                    <svg xmlns="http://www.w3.org/2000/svg" width="18" height="18" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">
                        <polyline points="6 9 12 15 18 9"/>
                    </svg>
                </span>
            </button>
            <div style=move || if collapsed.get() { "display: none;" } else { "padding: 0.25rem 1rem 0.5rem 1rem;" }>
                {children()}
            </div>
        </div>
    }
}
