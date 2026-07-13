//! Interactive per-day bar chart. Each point is `(date, value, ratio)` where `ratio`
//! is the FROZEN `value / target` for that day (so colours don't shift when the
//! target later changes): met (≥ 1.0) is GREEN, a shallow miss (≥ 0.5) ORANGE, a
//! deep miss RED, and an unevaluable day (`None`) neutral grey. The day-of-week sits
//! under each bar; tap / drag moves a cursor showing that day's date + value.

use leptos::*;

/// Short "DD.MM" from a "YYYY-MM-DD" date (falls back to the raw string).
fn short_date(s: &str) -> String {
    let mut it = s.split('-');
    match (it.next(), it.next(), it.next()) {
        (Some(_y), Some(m), Some(d)) => format!("{d}.{m}"),
        _ => s.to_string(),
    }
}

/// Russian two-letter weekday for a "YYYY-MM-DD" date.
fn weekday_ru(s: &str) -> &'static str {
    use chrono::Datelike;
    chrono::NaiveDate::parse_from_str(s, "%Y-%m-%d")
        .ok()
        .map(|d| ["Пн", "Вт", "Ср", "Чт", "Пт", "Сб", "Вс"][d.weekday().num_days_from_monday() as usize])
        .unwrap_or("")
}

// Plot geometry (scaled to container width). Compact — it sits inline between the
// indicator icon and the "?", with a row of weekday labels under the bars.
const VW: f64 = 340.0;
const VH: f64 = 76.0;
const PL: f64 = 4.0;
const PR: f64 = 336.0;
const PT: f64 = 16.0; // room for the tap tooltip
const PB: f64 = 58.0; // bar baseline; weekday labels sit below

const BAR_NEUTRAL: &str = "#cfd8e3"; // unevaluable day (no target)
const BAR_MET: &str = "#1fa463"; // green — target met (ratio ≥ 1.0)
const BAR_MILD: &str = "#e8850d"; // orange — shallow miss (ratio ≥ 0.5)
const BAR_DEEP: &str = "#e0304f"; // red — deep miss (ratio < 0.5)
const BAR_ACTIVE: &str = "#3b6fd4";

/// Bar colour from the frozen ratio: green (met) / orange (shallow) / red (deep) /
/// neutral (no target). Deeper shortfall → redder.
fn bar_color(ratio: Option<f64>) -> &'static str {
    match ratio {
        None => BAR_NEUTRAL,
        Some(r) if r >= 1.0 => BAR_MET,
        Some(r) if r >= 0.5 => BAR_MILD,
        Some(_) => BAR_DEEP,
    }
}

#[component]
pub fn DayBars(series: Signal<Vec<(String, f64, Option<f64>)>>, unit: String) -> impl IntoView {
    let active = create_rw_signal(None::<usize>);
    let svg_ref = create_node_ref::<leptos::svg::Svg>();

    let update = move |client_x: f64| {
        let Some(el) = svg_ref.get() else { return };
        let n = series.get_untracked().len();
        if n == 0 {
            return;
        }
        let element: &web_sys::Element = &el;
        let rect = element.get_bounding_client_rect();
        if rect.width() <= 0.0 {
            return;
        }
        let rel = (client_x - rect.left()) / rect.width();
        let plot_rel = (rel * VW - PL) / (PR - PL);
        let idx = (plot_rel * n as f64).floor() as i64;
        active.set(Some(idx.clamp(0, n as i64 - 1) as usize));
    };

    view! {
        <div style="display: flex; flex-direction: column; gap: 4px; -webkit-user-select: none; user-select: none; -webkit-touch-callout: none;">
            <svg
                node_ref=svg_ref
                viewBox=format!("0 0 {VW} {VH}")
                width="100%"
                style="display: block; touch-action: none; -webkit-user-select: none; user-select: none; -webkit-touch-callout: none;"
                on:pointerdown=move |ev: web_sys::PointerEvent| {
                    ev.prevent_default();
                    if let Some(el) = svg_ref.get() {
                        let element: &web_sys::Element = &el;
                        let _ = element.set_pointer_capture(ev.pointer_id());
                    }
                    update(ev.client_x() as f64);
                }
                on:pointermove=move |ev: web_sys::PointerEvent| {
                    if active.get_untracked().is_some() {
                        update(ev.client_x() as f64);
                    }
                }
                on:pointerup=move |_| active.set(None)
                on:pointercancel=move |_| active.set(None)
            >
                {move || {
                    let data = series.get();
                    let n = data.len();
                    if n == 0 {
                        return ().into_view();
                    }
                    let max = data.iter().map(|(_, v, _)| *v).fold(0.0_f64, f64::max).max(1.0);
                    let mapy = move |v: f64| PB - (v / max) * (PB - PT);
                    let bw = (PR - PL) / n as f64;
                    // Narrower bars (≈1.5× thinner than the 0.62 default).
                    let bar_w = (bw * 0.40).max(1.0);
                    let sel = active.get();

                    let bars = data.iter().enumerate().map(|(i, (date, v, ratio))| {
                        let cx = PL + (i as f64 + 0.5) * bw;
                        let y = mapy(*v);
                        let h = (PB - y).max(0.0);
                        let fill = if sel == Some(i) { BAR_ACTIVE } else { bar_color(*ratio) };
                        view! {
                            <g>
                                <rect x=cx - bar_w / 2.0 y=y width=bar_w height=h rx="1.5" fill=fill/>
                                <text x=cx y=VH - 4.0 text-anchor="middle" font-size="9"
                                    fill="var(--bulma-text-weak)">{weekday_ru(date)}</text>
                            </g>
                        }
                    }).collect_view();

                    let unit = unit.clone();
                    let cursor = sel.map(|i| {
                        let (date, v, _) = &data[i];
                        let cx = PL + (i as f64 + 0.5) * bw;
                        let tip_x = cx.clamp(PL + 42.0, PR - 42.0);
                        let label = format!("{} · {:.0} {}", short_date(date), v, unit);
                        view! {
                            <g>
                                <line x1=cx y1=PT - 4.0 x2=cx y2=PB stroke=BAR_ACTIVE stroke-width="1"/>
                                <circle cx=cx cy=mapy(*v) r="3" fill=BAR_ACTIVE/>
                                <text x=tip_x y=PT - 12.0 text-anchor="middle"
                                    fill="var(--bulma-text)" font-size="12" font-weight="700">
                                    {label}
                                </text>
                            </g>
                        }
                    });

                    view! { <g>{bars}{cursor}</g> }.into_view()
                }}
            </svg>
        </div>
    }
}
