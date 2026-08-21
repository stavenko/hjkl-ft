//! Interactive per-day bar chart. Each point is `(date, value, ratio)` where `ratio`
//! is the FROZEN `value / target` for that day (so met/unmet doesn't shift when the
//! target later changes): a met day (ratio ≥ 1.0) is GREEN, an unevaluable day
//! (`None`) neutral grey, and a MISSED day is drawn in `miss_color`. The day-of-week
//! sits under each bar; tap / drag moves a cursor showing that day's date + value.
//!
//! ЦВЕТ СТОЛБИКА НЕ ЗАВИСИТ ОТ ЦВЕТА ИНДИКАТОРА. У столбика вопрос двоичный —
//! закрыто или нет, — а индикатор считается по своему правилу поверх этих же
//! недель. Раньше сюда передавали цвет состояния, и стоило индикатору позеленеть,
//! как незакрытые недели зеленели вместе с ним: неделя с 1.2 порции гема при норме
//! 3 выглядела точно так же, как взятая.

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

// Plot geometry (scaled to container width). One indicator per row, so it's ~1.5×
// taller than the old two-up layout, with a row of readable weekday labels below.
const VW: f64 = 340.0;
const VH: f64 = 112.0;
const PL: f64 = 4.0;
const PR: f64 = 336.0;
const PT: f64 = 28.0; // top band reserved for the tap tooltip (text sits above the bars)
const PB: f64 = 92.0; // bar baseline; weekday labels sit below

const BAR_NEUTRAL: &str = "#cfd8e3"; // unevaluable day (no target)
const BAR_MET: &str = "#1fa463"; // green — target met (ratio ≥ 1.0)
const BAR_ACTIVE: &str = "#3b6fd4";
/// Цвет ПРОМАХА — один и тот же всегда: столбик отвечает на двоичный вопрос.
pub const BAR_MISS: &str = "#e0304f";

/// Bar colour: green when the day met its target (ratio ≥ 1.0), neutral grey when
/// unevaluable (no target), otherwise `miss_color` — the single per-chart colour the
/// caller derived from the indicator's overall state (orange/red).
fn bar_color<'a>(ratio: Option<f64>, miss_color: &'a str) -> &'a str {
    match ratio {
        None => BAR_NEUTRAL,
        Some(r) if r >= 1.0 => BAR_MET,
        Some(_) => miss_color,
    }
}

#[component]
pub fn DayBars(
    series: Signal<Vec<(String, f64, Option<f64>)>>,
    unit: String,
    /// Colour for MISSED days — one colour for the whole chart, from the indicator's
    /// overall state (orange for an occasional miss, red for a chronic one).
    miss_color: String,
    /// Per-day met/miss verdicts by the INDICATOR'S OWN rule (calories = the
    /// ±50 kcal band, AtLeast metrics = ratio ≥ 1.0): Some(true) met ·
    /// Some(false) missed · None unevaluable. When absent, falls back to the
    /// generic ratio ≥ 1.0 rule.
    #[prop(optional)] met: Option<Vec<Option<bool>>>,
    /// Подписи под столбиками. Пусто — берётся день недели из даты. Недельным
    /// индикаторам день недели не подходит: у них столбик — это целая неделя.
    #[prop(optional)] labels: Option<Vec<String>>,
    /// Столбики ОТ СЕРЕДИНЫ: вверх зелёным при значении больше нуля, вниз красным
    /// при меньше. Для показателей, у которых значение — ОТКЛОНЕНИЕ от нормы со
    /// знаком, а не количество (баланс жира). Обычная шкала от нуля для них не
    /// годится вдвойне: отрицательный столбик она рисует нулевой высоты, то есть
    /// молча прячет ровно те недели, о которых и надо говорить.
    #[prop(default = false)] signed: bool,
    /// Знаков после запятой в подписи курсора. У отклонения баланса это сотые:
    /// «−0.88» против бессмысленного «−0.9».
    #[prop(default = 1)] decimals: usize,
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
                    // У знакового ряда масштаб берётся по МОДУЛЮ, а базовая линия
                    // стоит посередине: иначе вверх и вниз меряются разными мерками.
                    let max = data
                        .iter()
                        .map(|(_, v, _)| if signed { v.abs() } else { *v })
                        .fold(0.0_f64, f64::max)
                        .max(if signed { 0.01 } else { 1.0 });
                    let base = if signed { (PT + PB) / 2.0 } else { PB };
                    let span = if signed { (PB - PT) / 2.0 } else { PB - PT };
                    let mapy = move |v: f64| base - (v / max) * span;
                    let bw = (PR - PL) / n as f64;
                    // Narrower bars (≈1.5× thinner than the 0.62 default).
                    let bar_w = (bw * 0.40).max(1.0);
                    let sel = active.get();

                    let bars = data.iter().enumerate().map(|(i, (date, v, ratio))| {
                        let cx = PL + (i as f64 + 0.5) * bw;
                        let y0 = mapy(*v);
                        // Знаковый столбик растёт от базовой линии в свою сторону:
                        // положительный вверх, отрицательный вниз.
                        let (y, h) = if signed {
                            (y0.min(base), (y0 - base).abs())
                        } else {
                            (y0, (PB - y0).max(0.0))
                        };
                        // Per-day verdict from the caller (the indicator's own rule)
                        // when provided; the generic ratio rule otherwise. У знакового
                        // ряда вердикт — это сам знак, и цвет берётся от него.
                        let day_fill = if signed {
                            match ratio {
                                None => BAR_NEUTRAL,
                                Some(_) if *v >= 0.0 => BAR_MET,
                                Some(_) => &miss_color,
                            }
                        } else {
                            match met.as_ref().and_then(|m| m.get(i).copied()) {
                                Some(Some(true)) => BAR_MET,
                                Some(Some(false)) => &miss_color,
                                Some(None) => BAR_NEUTRAL,
                                None => bar_color(*ratio, &miss_color),
                            }
                        };
                        let fill = if sel == Some(i) { BAR_ACTIVE } else { day_fill }.to_string();
                        view! {
                            <g>
                                <rect x=cx - bar_w / 2.0 y=y width=bar_w height=h rx="1.5" fill=fill/>
                                <text x=cx y=VH - 6.0 text-anchor="middle" font-size="14"
                                    fill="var(--bulma-text)">{
                                        labels.as_ref()
                                            .and_then(|l| l.get(i).cloned())
                                            .unwrap_or_else(|| weekday_ru(date).to_string())
                                    }</text>
                            </g>
                        }
                    }).collect_view();

                    let unit = unit.clone();
                    let cursor = sel.map(|i| {
                        let (date, v, _) = &data[i];
                        let cx = PL + (i as f64 + 0.5) * bw;
                        let tip_x = cx.clamp(PL + 52.0, PR - 52.0);
                        // У недельных столбиков в подсказке нужна их подпись, а не дата:
                        // дата начала недели ничего не говорит.
                        let head = labels
                            .as_ref()
                            .and_then(|l| l.get(i).cloned())
                            .unwrap_or_else(|| short_date(date));
                        // У знакового ряда знак — часть значения, и его надо назвать:
                        // «−0.88», а не «0.88». Минус типографский.
                        let num = if signed {
                            format!("{v:+.*}", decimals).replace('-', "\u{2212}")
                        } else {
                            format!("{v:.*}", decimals)
                        };
                        let label = format!("{head} · {num} {unit}");
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
