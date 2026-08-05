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
// The planka line colour (green). Bars stay neutral — the target is shown by the
// line only (green planka bars on a green line blended together).
const PLANKA_LINE: &str = "#1fa463";

/// Optional target line the chart draws INSTEAD of the average (green planka vs pink
/// average). `None` → the average line. Bars are always neutral either way.
///
/// `planka_series` — ИСТОРИЯ планки по тем же дням, что и `series`: значение на
/// каждый день. Планка движется, и одна горизонтальная линия врала бы про прошлое —
/// день, выполненный по старой планке, оказывался бы ниже сегодняшней. Ступенчатая
/// линия показывает ровно то, по чему день судился. Пусто → рисуется среднее.
#[component]
pub fn BarChart(
    series: Signal<Vec<(String, f64)>>,
    unit: String,
    #[prop(optional, into)] planka_series: MaybeSignal<Vec<(String, f64)>>,
) -> impl IntoView {
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

                    // Average over the shown days EXCLUDING today (the last point is
                    // today — a still-partial day that would drag the mean down).
                    // Unlogged (zero) days don't count.
                    let logged_past: Vec<f64> =
                        data[..n - 1].iter().map(|(_, k)| *k).filter(|k| *k > 0.0).collect();
                    let avg = (!logged_past.is_empty())
                        .then(|| logged_past.iter().sum::<f64>() / logged_past.len() as f64);

                    // Планка по дням, выровненная с барами: для каждого дня серии
                    // берётся значение из истории. Пусто → показываем среднее.
                    let ph = planka_series.get();
                    let planka_by_day: Vec<Option<f64>> = data
                        .iter()
                        .map(|(d, _)| ph.iter().find(|(pd, _)| pd == d).map(|(_, v)| *v))
                        .collect();
                    let has_planka = planka_by_day.iter().any(|p| p.is_some());
                    let line_val = if has_planka { None } else { avg };
                    let max = data
                        .iter()
                        .map(|(_, k)| *k)
                        .fold(0.0_f64, f64::max)
                        .max(line_val.unwrap_or(0.0))
                        .max(planka_by_day.iter().flatten().copied().fold(0.0_f64, f64::max))
                        .max(1.0);
                    let mapy = move |k: f64| PB - (k / max) * (PB - PT);
                    let bw = (PR - PL) / n as f64;
                    let bar_w = (bw * 0.62).max(1.0);
                    let sel = active.get();

                    let bars = data.iter().enumerate().map(|(i, (_, k))| {
                        let cx = PL + (i as f64 + 0.5) * bw;
                        let y = mapy(*k);
                        let h = (PB - y).max(0.0);
                        // Bars stay neutral; the target is shown by the line only.
                        // Selected day → blue highlight.
                        let fill = if sel == Some(i) { BAR_ACTIVE } else { BAR };
                        view! {
                            <rect x=cx - bar_w / 2.0 y=y width=bar_w height=h rx="1.5" fill=fill/>
                        }
                    }).collect_view();

                    // Среднее — только когда планки нет вовсе.
                    let line_unit = unit.clone();
                    let avg_line = line_val.map(|lv| {
                        let ly = mapy(lv);
                        view! {
                            <g>
                                <line x1=PL y1=ly x2=PR y2=ly
                                    stroke=AVG stroke-width="1.2" stroke-dasharray="4 3"/>
                                <text x=PR y=ly - 3.0 text-anchor="end"
                                    fill=AVG font-size="10.5" font-weight="600">
                                    {format!("{} {:.0} {}", t("chart.average"), lv, line_unit)}
                                </text>
                            </g>
                        }
                    });

                    // СТУПЕНЧАТАЯ линия планки: горизонтальный отрезок на ширину дня,
                    // вертикальный — в день смены. Так видно и величину, и когда она
                    // менялась; прямая через весь график врала бы про прошлое.
                    let planka_unit = unit.clone();
                    let planka_line = has_planka.then(|| {
                        let mut d = String::new();
                        let mut prev: Option<f64> = None;
                        for (i, p) in planka_by_day.iter().enumerate() {
                            let Some(v) = p else { prev = None; continue };
                            let (x0, x1) = (PL + i as f64 * bw, PL + (i as f64 + 1.0) * bw);
                            let y = mapy(*v);
                            match prev {
                                // Планка сменилась — соединяем вертикалью.
                                Some(pv) if (pv - *v).abs() > f64::EPSILON => {
                                    d.push_str(&format!(" L {x0:.1} {y:.1} L {x1:.1} {y:.1}"));
                                }
                                Some(_) => d.push_str(&format!(" L {x1:.1} {y:.1}")),
                                None => d.push_str(&format!(" M {x0:.1} {y:.1} L {x1:.1} {y:.1}")),
                            }
                            prev = Some(*v);
                        }
                        // Подпись — у последнего известного значения.
                        let last = planka_by_day.iter().flatten().last().copied();
                        view! {
                            <g>
                                <path d=d.trim().to_string() fill="none"
                                    stroke=PLANKA_LINE stroke-width="1.4" stroke-dasharray="4 3"/>
                                {last.map(|lv| view! {
                                    <text x=PR y=mapy(lv) - 3.0 text-anchor="end"
                                        fill=PLANKA_LINE font-size="10.5" font-weight="600">
                                        {format!("{} {:.0} {}", t("chart.planka"), lv, planka_unit)}
                                    </text>
                                })}
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
                            {planka_line}
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
