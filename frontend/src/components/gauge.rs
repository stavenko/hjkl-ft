//! Horizontal progress gauge: a grey track that fills with `color` from the left
//! as `value` approaches `target`. Used on the dashboard progress widget for
//! calories and the daily-nutrient goals. Empty (all grey) at value 0, so a
//! stack of gauges reads as "greyed out, filling in as the day's data arrives".

use leptos::*;

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
) -> impl IntoView {
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
    let hint_box = hint.map(|text| {
        view! {
            {move || open.get().then(|| view! {
                <div style="background: var(--bulma-scheme-main-ter, rgba(0,0,0,0.04)); \
                        border-radius: 8px; padding: 8px 10px;">
                    <span class="is-size-7 has-text-grey" style="line-height: 1.4;">{text.clone()}</span>
                </div>
            })}
        }
    });

    view! {
        <div style="display: flex; flex-direction: column; gap: 5px; width: 100%; min-width: 0;">
            <div style="display: flex; justify-content: space-between; align-items: baseline; gap: 8px;">
                <span style="display: inline-flex; align-items: center; gap: 6px;">
                    <span class="is-size-7 has-text-weight-medium" style="color: var(--bulma-text-weak);">{label}</span>
                    {hint_btn}
                </span>
                <span class="is-size-7" style="white-space: nowrap;">
                    <span class="has-text-weight-bold">{format!("{value:.0}")}</span>
                    <span class="has-text-grey">{format!(" / {target:.0} {unit}")}</span>
                </span>
            </div>
            <div style=format!("height: {height}px; border-radius: {radius}px; background: var(--bulma-border-weak); overflow: hidden;")>
                <div style=format!("height: 100%; width: {pct:.1}%; background: {color}; border-radius: {radius}px; transition: width 0.4s;")></div>
            </div>
            {hint_box}
        </div>
    }
}
