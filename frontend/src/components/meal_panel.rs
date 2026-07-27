use leptos::*;
use leptos_router::use_navigate;

use crate::services::i18n::{self, t};

/// An explicit meal panel (Завтрак / Обед / Ужин) on the diary.
///
/// Behaviour (per the explicit-meals redesign):
///  - The HEADER is the add affordance: tapping it (or the «+» on its right) opens
///    the food-search flow pre-tagged to this meal (`/diary/add?meal=<key>`). Only
///    active when `can_add` (i.e. today) — past days show the header as a label.
///  - When `is_empty`, the panel is just that header + «+» — no body, no footer.
///  - Otherwise a small FOOTER always shows the meal's aggregate КБЖУ on the left
///    and a collapse/expand toggle on the right. Collapsing hides the product rows
///    (the КБЖУ summary stays); expanding shows them again. Collapse is per-panel
///    and user-driven (no accordion, no auto-collapse).
///
/// `accent` is a muted 6-digit `#rrggbb`; `accent + "22"` / `"55"` are alpha tints
/// for the header band / divider. The meal's rows come in as `children`.
#[component]
pub fn MealPanel(
    /// Localised meal name (Завтрак / Обед / Ужин).
    title: String,
    /// Muted per-meal accent colour, a 6-digit `#rrggbb` hex.
    accent: String,
    /// Meal key stored in `meal_label` and passed to the add flow.
    meal_key: String,
    /// Whether adding is allowed (only the current day). Off → header is a label.
    can_add: bool,
    /// No rows → render the header (+ «+») only.
    is_empty: bool,
    /// Aggregate calories for the meal (collapsed summary).
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
    // Per-panel collapse state; internal so toggling never re-runs the parent.
    let collapsed = create_rw_signal(false);
    let navigate = use_navigate();

    let macro_line = format!(
        "{} {:.0} · {} {:.0} · {} {:.0} · {} {:.0}",
        i18n::nutrient_badge("Calories"), kcal,
        i18n::nutrient_badge("Protein"), protein,
        i18n::nutrient_badge("Fat"), fat,
        i18n::nutrient_badge("Carbs"), carbs,
    );

    let tint = format!("{accent}22"); // ~13% — soft header band
    let divider = format!("{accent}55"); // ~33% — header/body separator
    let container_style = format!(
        "background: var(--bulma-scheme-main); border: 1px solid {accent}; border-radius: 12px; margin-bottom: 0.75rem;"
    );
    let title_style = format!("color: {accent};");

    // Header rounds all four corners when it IS the whole panel (empty), else only
    // the top; a divider under it only when there's an expanded body below.
    let show_divider = !is_empty;
    let header_style = {
        let tint = tint.clone();
        let divider = divider.clone();
        move || {
            let radius = if is_empty { "11px" } else { "11px 11px 0 0" };
            let show_body_divider = show_divider && !collapsed.get();
            format!(
                "width: 100%; height: auto; display: flex; align-items: center; justify-content: space-between; padding: 0.75rem 1rem; text-decoration: none; border: none; border-radius: {radius}; background: {tint}; {}",
                if show_body_divider { format!("border-bottom: 1px solid {divider};") } else { String::new() }
            )
        }
    };

    // Tapping the header opens the add flow tagged to this meal (same as «+»).
    let go_add = {
        let navigate = navigate.clone();
        let meal_key = meal_key.clone();
        move || {
            if can_add {
                navigate(&format!("/diary/add?meal={meal_key}"), Default::default());
            }
        }
    };

    view! {
        <div style=container_style>
            // Header — tap = add to this meal (iOS-safe <button>). Disabled = label.
            <button
                class="button is-ghost"
                style=header_style
                disabled=!can_add
                on:click={ let go_add = go_add.clone(); move |_| go_add() }
            >
                <span class="is-size-6 has-text-weight-bold has-text-left" style=title_style>{title}</span>
                {can_add.then(|| view! {
                    <span style=move || format!("flex-shrink: 0; margin-left: 0.75rem; color: {accent}; font-size: 1.4rem; line-height: 1;", )>"+"</span>
                })}
            </button>

            // Body + footer only when there are rows.
            {(!is_empty).then(|| {
                let divider = divider.clone();
                view! {
                    // Product rows — shown when expanded.
                    <div style=move || if collapsed.get() { "display: none;".to_string() } else { "padding: 0.25rem 1rem 0.5rem 1rem;".to_string() }>
                        {children()}
                    </div>
                    // Footer: ALWAYS shows the meal's aggregate КБЖУ on the left, with
                    // the collapse/expand toggle on the right — in both states.
                    <div style=format!(
                        "display: flex; align-items: center; justify-content: space-between; gap: 0.5rem; padding: 0.35rem 0.4rem 0.35rem 1rem; border-top: 1px solid {divider};"
                    )>
                        <span class="is-size-7 has-text-grey">{macro_line}</span>
                        <button
                            class="button is-ghost is-small has-text-grey"
                            style="text-decoration: none; height: auto; padding: 0.25rem 0.5rem; flex-shrink: 0;"
                            aria-label=move || if collapsed.get() { t("diary.expand") } else { t("diary.collapse") }
                            on:click=move |_| collapsed.update(|c| *c = !*c)
                        >
                            <svg xmlns="http://www.w3.org/2000/svg" width="18" height="18" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"
                                style=move || format!("transform: rotate({}deg);", if collapsed.get() { 180 } else { 0 })>
                                <polyline points="18 15 12 9 6 15"/>
                            </svg>
                        </button>
                    </div>
                }
            })}
        </div>
    }
}
