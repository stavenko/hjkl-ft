//! Radial progress gauge: a grey track that fills with `color` as `value`
//! approaches `target`. Used on the dashboard progress widget for calories and
//! the daily-nutrient indicators. Empty (all grey) at value 0, so a set of
//! gauges reads as "greyed out, filling in as the day's data arrives".

use leptos::*;

/// `size` px square. `value`/`target` in the same unit; the arc fills to
/// `min(value/target, 1)`. `color` is the fill (the metric's colour); the track
/// is grey. The centre shows `value` over `/ target unit`, with `label` beneath.
#[component]
pub fn Gauge(
    value: f64,
    target: f64,
    label: String,
    unit: String,
    color: String,
    #[prop(default = 92.0)] size: f64,
) -> impl IntoView {
    // Normalize negative zero (an empty nutrient sum can be -0.0) so the centre
    // reads "0", not "-0".
    let value = value + 0.0;
    let frac = if target > 0.0 { (value / target).clamp(0.0, 1.0) } else { 0.0 };
    const R: f64 = 42.0;
    let circ = 2.0 * std::f64::consts::PI * R; // ≈ 263.9
    let dash = frac * circ;
    let sw = 9.0;

    view! {
        <div style="display: flex; flex-direction: column; align-items: center; gap: 5px; min-width: 0;">
            <div style=format!("position: relative; width: {size}px; height: {size}px;")>
                <svg viewBox="0 0 100 100" width="100%" height="100%" style="transform: rotate(-90deg);">
                    <circle cx="50" cy="50" r=R fill="none" stroke="var(--bulma-border-weak)" stroke-width=sw/>
                    <circle cx="50" cy="50" r=R fill="none" stroke=color stroke-width=sw
                        stroke-linecap="round"
                        stroke-dasharray=format!("{dash} {circ}")
                        style="transition: stroke-dasharray 0.4s;"/>
                </svg>
                <div style="position: absolute; inset: 0; display: flex; flex-direction: column; align-items: center; justify-content: center; gap: 0;">
                    <span class="has-text-weight-bold" style="font-size: 1.05rem; line-height: 1.1;">{format!("{value:.0}")}</span>
                    <span class="is-size-7 has-text-grey" style="line-height: 1.1;">{format!("/ {target:.0}")}</span>
                    <span class="has-text-grey-light" style="font-size: 0.6rem;">{unit}</span>
                </div>
            </div>
            <span class="is-size-7 has-text-grey has-text-weight-medium">{label}</span>
        </div>
    }
}
