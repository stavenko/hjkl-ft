//! Horizontal progress gauge: a grey track that fills with `color` from the left
//! as `value` approaches `target`. Used on the dashboard progress widget for
//! calories and the daily-nutrient goals. Empty (all grey) at value 0, so a
//! stack of gauges reads as "greyed out, filling in as the day's data arrives".

use leptos::*;

/// (bar colour, value colour) for an "at least" daily gauge (protein / veg-fruit):
/// a NEUTRAL grey fill while the day's amount is below the target, turning GREEN
/// (bar + value number) once the target is met. `value colour` is `None` when the
/// value keeps its default colour.
pub fn at_least_colors(value: f64, target: f64) -> (&'static str, Option<&'static str>) {
    if target > 0.0 && value >= target {
        ("#1fa463", Some("#1fa463"))
    } else {
        ("#9aa0a6", None)
    }
}

/// A full-width bar. `value`/`target` in the same unit; the fill spans
/// `min(value/target, 1)` of the track. `color` is the fill (the metric's
/// colour); the track is grey. The header line shows `label` on the left and
/// `value / target unit` on the right.
///
/// `hint`: when set, a "?" button appears next to the label; tapping it toggles a
/// tooltip (below the bar) that explains where this target came from.
#[component]
pub fn Gauge(
    value: f64,
    target: f64,
    label: String,
    unit: String,
    color: String,
    /// Bar thickness in px (calories get a thicker bar than the daily goals).
    #[prop(default = 8.0)] height: f64,
    /// Optional "why this target" explanation, shown via a "?" tooltip.
    #[prop(optional, into)] hint: Option<String>,
    /// Optional colour for the VALUE number (the eaten amount) — e.g. red when the
    /// calorie planka is exceeded, green when an "at least" goal is met. The target
    /// (`/ NNN`) always keeps its usual grey.
    #[prop(default = None)] value_color: Option<String>,
) -> impl IntoView {
    let value_style = value_color
        .map(|c| format!("color: {c};"))
        .unwrap_or_default();
    // Normalize negative zero (an empty nutrient sum can be -0.0) so the label
    // reads "0", not "-0".
    let value = value + 0.0;
    let frac = if target > 0.0 { (value / target).clamp(0.0, 1.0) } else { 0.0 };
    let pct = frac * 100.0;
    let radius = height / 2.0;

    let open = create_rw_signal(false);
    let hint_btn = hint.clone().map(|_| {
        view! {
            <button
                attr:aria-label="?"
                on:click=move |_| open.update(|o| *o = !*o)
                style="width: 16px; height: 16px; min-width: 16px; border-radius: 50%; \
                    border: 1px solid var(--bulma-border); background: transparent; \
                    color: var(--bulma-text-weak); font-size: 0.62rem; font-weight: 700; \
                    line-height: 1; cursor: pointer; padding: 0; display: inline-flex; \
                    align-items: center; justify-content: center;">
                "?"
            </button>
        }
    });
    // A floating popup (not inline): a full-screen backdrop to dismiss + a card
    // anchored under the gauge header, overlaying the content rather than pushing it.
    let hint_popup = hint.map(|text| {
        view! {
            {move || open.get().then(|| {
                let t = text.clone();
                // Dismiss on a tap ANYWHERE — the full-screen backdrop AND the card
                // itself. `pointerup` (not `click`) so it fires on iOS, where a tap on
                // a bare <div> never raises a delegated click.
                view! {
                    <div on:pointerup=move |_| open.set(false)
                        style="position: fixed; inset: 0; z-index: 40; cursor: pointer;"></div>
                    <div on:pointerup=move |_| open.set(false)
                        style="position: absolute; z-index: 41; top: 24px; left: 0; right: 0; cursor: pointer; \
                            background: var(--bulma-scheme-main); border: 0.5px solid var(--bulma-border-weak); \
                            box-shadow: 0 10px 30px rgba(0,0,0,0.22); border-radius: 12px; padding: 12px 14px;">
                        <span class="is-size-7 has-text-grey" style="line-height: 1.45; white-space: pre-line;">{t}</span>
                    </div>
                }
            })}
        }
    });

    view! {
        <div style="position: relative; display: flex; flex-direction: column; gap: 5px; width: 100%; min-width: 0;">
            <div style="display: flex; justify-content: space-between; align-items: baseline; gap: 8px;">
                <span style="display: inline-flex; align-items: center; gap: 6px;">
                    <span class="is-size-7 has-text-weight-medium" style="color: var(--bulma-text-weak);">{label}</span>
                    {hint_btn}
                </span>
                <span class="is-size-7" style="white-space: nowrap;">
                    <span class="has-text-weight-bold" style=value_style>{format!("{value:.0}")}</span>
                    <span class="has-text-grey">{format!(" / {target:.0} {unit}")}</span>
                </span>
            </div>
            <div style=format!("height: {height}px; border-radius: {radius}px; background: var(--bulma-border-weak); overflow: hidden;")>
                <div style=format!("height: 100%; width: {pct:.1}%; background: {color}; border-radius: {radius}px; transition: width 0.4s;")></div>
            </div>
            {hint_popup}
        </div>
    }
}
