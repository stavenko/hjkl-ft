//! Interactive daily bar chart for the expanded dashboard widgets.
//!
//! Bars = the per-day value (oldest → newest, today rightmost). A dashed line
//! marks the average over the logged days (value > 0) EXCLUDING today (a
//! still-partial day), and is labelled. Touch (or drag) anywhere on the chart
//! moves a cursor that snaps to the nearest day and shows that day's date +
//! value. `unit` labels the tooltip and the average line (e.g. "ккал", "шагов").

use leptos::*;

use crate::services::i18n::t;

/// Short "DD.MM" from a "YYYY-MM-DD" date (falls back to the raw string).
fn short_date(s: &str) -> String {
    let mut it = s.split('-');
    match (it.next(), it.next(), it.next()) {
        (Some(_y), Some(m), Some(d)) => format!("{d}.{m}"),
        _ => s.to_string(),
    }
}

// Plot geometry in the SVG's own coordinate space (scaled to the container width).
const VW: f64 = 340.0;
const VH: f64 = 200.0;
const PL: f64 = 12.0; // plot left
const PR: f64 = 328.0; // plot right
const PT: f64 = 30.0; // plot top (room for the tooltip)
const PB: f64 = 168.0; // plot bottom (room for x-axis labels)

const BAR: &str = "#cfd8e3";
const BAR_ACTIVE: &str = "#3b6fd4";
const AVG: &str = "#e0699b";

#[component]
pub fn BarChart(series: Signal<Vec<(String, f64)>>, unit: String) -> impl IntoView {
    let active = create_rw_signal(None::<usize>);
    let svg_ref = create_node_ref::<leptos::svg::Svg>();

    // Map an absolute clientX to the nearest day index using the SVG's on-screen
    // rect (robust regardless of which child the pointer is over).
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
        // The plot area is inset by PL/PR within the VW-wide viewBox; map the
        // pointer into plot space so the snap lines up with the bars.
        let rel = (client_x - rect.left()) / rect.width(); // 0..1 across the svg
        let plot_rel = (rel * VW - PL) / (PR - PL); // 0..1 across the plot
        let idx = (plot_rel * n as f64).floor() as i64;
        active.set(Some(idx.clamp(0, n as i64 - 1) as usize));
    };

    view! {
        // Kill the iOS Safari text-selection loupe on touch-drag: disable webkit
        // selection AND the touch callout (the plain `user-select` isn't enough on
        // Safari — it needs the -webkit- prefixes).
        <div style="display: flex; flex-direction: column; gap: 6px; -webkit-user-select: none; user-select: none; -webkit-touch-callout: none;">
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
                    // Track only while a press is active (touch/drag), not on hover.
                    if active.get_untracked().is_some() {
                        update(ev.client_x() as f64);
                    }
                }
                on:pointerup=move |_| active.set(None)
                on:pointercancel=move |_| active.set(None)
            >
                {
                    let unit = unit.clone();
                    move || {
                    let data = series.get();
                    let n = data.len();
                    let logged: Vec<f64> = data.iter().map(|(_, k)| *k).filter(|k| *k > 0.0).collect();
                    if n == 0 || logged.is_empty() {
                        return view! {
                            <text x=VW / 2.0 y=VH / 2.0 text-anchor="middle"
                                fill="var(--bulma-text-weak)" font-size="13">
                                {move || t("chart.no_data")}
                            </text>
                        }.into_view();
                    }

                    let max = data.iter().map(|(_, k)| *k).fold(0.0_f64, f64::max).max(1.0);
                    // Average over the shown days EXCLUDING today (the last point is
                    // today — a still-partial day that would drag the mean down).
                    // Unlogged (zero) days don't count.
                    let logged_past: Vec<f64> =
                        data[..n - 1].iter().map(|(_, k)| *k).filter(|k| *k > 0.0).collect();
                    let avg = (!logged_past.is_empty())
                        .then(|| logged_past.iter().sum::<f64>() / logged_past.len() as f64);
                    let mapy = move |k: f64| PB - (k / max) * (PB - PT);
                    let bw = (PR - PL) / n as f64;
                    let bar_w = (bw * 0.62).max(1.0);
                    let sel = active.get();

                    let bars = data.iter().enumerate().map(|(i, (_, k))| {
                        let cx = PL + (i as f64 + 0.5) * bw;
                        let y = mapy(*k);
                        let h = (PB - y).max(0.0);
                        let fill = if sel == Some(i) { BAR_ACTIVE } else { BAR };
                        view! {
                            <rect x=cx - bar_w / 2.0 y=y width=bar_w height=h rx="1.5" fill=fill/>
                        }
                    }).collect_view();

                    let avg_unit = unit.clone();
                    let avg_line = avg.map(|avg| {
                        let avg_y = mapy(avg);
                        view! {
                            <g>
                                <line x1=PL y1=avg_y x2=PR y2=avg_y
                                    stroke=AVG stroke-width="1.2" stroke-dasharray="4 3"/>
                                <text x=PR y=avg_y - 3.0 text-anchor="end"
                                    fill=AVG font-size="10.5" font-weight="600">
                                    {format!("{} {:.0} {}", t("chart.average"), avg, avg_unit)}
                                </text>
                            </g>
                        }
                    });

                    // X-axis: first + last date only, to keep it uncluttered.
                    let axis = view! {
                        <g fill="var(--bulma-text-weak)" font-size="10">
                            <text x=PL y=VH - 6.0 text-anchor="start">{short_date(&data[0].0)}</text>
                            <text x=PR y=VH - 6.0 text-anchor="end">{short_date(&data[n - 1].0)}</text>
                        </g>
                    };

                    // Cursor + tooltip for the selected day.
                    let tip_unit = unit.clone();
                    let cursor = sel.map(|i| {
                        let (date, k) = &data[i];
                        let cx = PL + (i as f64 + 0.5) * bw;
                        let tip_x = cx.clamp(PL + 42.0, PR - 42.0);
                        let label = format!("{} · {:.0} {}", short_date(date), k, tip_unit);
                        view! {
                            <g>
                                <line x1=cx y1=PT - 4.0 x2=cx y2=PB stroke=BAR_ACTIVE stroke-width="1"/>
                                <circle cx=cx cy=mapy(*k) r="3" fill=BAR_ACTIVE/>
                                <text x=tip_x y=PT - 12.0 text-anchor="middle"
                                    fill="var(--bulma-text)" font-size="12" font-weight="700">
                                    {label}
                                </text>
                            </g>
                        }
                    });

                    view! {
                        <g>
                            {bars}
                            {avg_line}
                            {axis}
                            {cursor}
                        </g>
                    }.into_view()
                }}
            </svg>
            <p class="is-size-7 has-text-grey" style="text-align: center; margin: 0;">
                {move || t("chart.hint")}
            </p>
        </div>
    }
}
