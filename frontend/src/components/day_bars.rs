//! Interactive per-day bar chart with target-based colouring. Bars for days that
//! MISS the target are drawn red; the rest neutral. A dashed line marks the target.
//! Tap / drag moves a cursor showing that day's date + value — the same interaction
//! as the calorie chart. Used under each daily indicator in the expanded view.

use leptos::*;

/// Short "DD.MM" from a "YYYY-MM-DD" date (falls back to the raw string).
fn short_date(s: &str) -> String {
    let mut it = s.split('-');
    match (it.next(), it.next(), it.next()) {
        (Some(_y), Some(m), Some(d)) => format!("{d}.{m}"),
        _ => s.to_string(),
    }
}

// Plot geometry (scaled to container width). Compact — it sits inline between the
// indicator icon and the "?", so it's short: no static axis labels (the tap tooltip
// shows the date), just enough top room for that tooltip.
const VW: f64 = 340.0;
const VH: f64 = 60.0;
const PL: f64 = 4.0;
const PR: f64 = 336.0;
const PT: f64 = 16.0; // room for the tooltip
const PB: f64 = 58.0;

const BAR: &str = "#cfd8e3";
const BAR_MISS: &str = "#e0304f";
const BAR_ACTIVE: &str = "#3b6fd4";
const TARGET_LINE: &str = "#9aa0a6";

/// `over_is_bad`: false (default) means an "at least" goal — a day MISSES when its
/// value is BELOW the target (protein / veg-fruit). True means "at most" (calories).
#[component]
pub fn DayBars(
    series: Signal<Vec<(String, f64)>>,
    target: f64,
    #[prop(default = false)] over_is_bad: bool,
    unit: String,
) -> impl IntoView {
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
                    let max = data.iter().map(|(_, v)| *v).fold(0.0_f64, f64::max).max(target).max(1.0);
                    let mapy = move |v: f64| PB - (v / max) * (PB - PT);
                    let bw = (PR - PL) / n as f64;
                    let bar_w = (bw * 0.62).max(1.0);
                    let sel = active.get();
                    let miss = move |v: f64| {
                        target > 0.0 && if over_is_bad { v > target } else { v < target }
                    };

                    let bars = data.iter().enumerate().map(|(i, (_, v))| {
                        let cx = PL + (i as f64 + 0.5) * bw;
                        let y = mapy(*v);
                        let h = (PB - y).max(0.0);
                        let fill = if sel == Some(i) { BAR_ACTIVE } else if miss(*v) { BAR_MISS } else { BAR };
                        view! {
                            <rect x=cx - bar_w / 2.0 y=y width=bar_w height=h rx="1.5" fill=fill/>
                        }
                    }).collect_view();

                    let target_line = (target > 0.0).then(|| {
                        let ty = mapy(target);
                        view! {
                            <line x1=PL y1=ty x2=PR y2=ty stroke=TARGET_LINE stroke-width="1" stroke-dasharray="4 3"/>
                        }
                    });

                    let unit = unit.clone();
                    let cursor = sel.map(|i| {
                        let (date, v) = &data[i];
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

                    view! {
                        <g>
                            {bars}
                            {target_line}
                            {cursor}
                        </g>
                    }.into_view()
                }}
            </svg>
        </div>
    }
}
