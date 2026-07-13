//! Shared mini line-chart used by the weight and steps widgets/modals.
//! The line/area/dots are SVG (scaled uniformly — no distortion), while the
//! date labels are plain HTML so they stay readable at any width.

const LABEL_ROW: &str = "display: flex; justify-content: space-between; font-size: 10px; color: var(--bulma-text-weak); margin-top: 2px; min-height: 12px;";

/// Y axis, X axis and two faint gridlines — drawn in every chart (with or
/// without data).
const AXES: &str = r#"<line x1="0" y1="20" x2="300" y2="20" stroke="var(--bulma-border-weak)" stroke-width="1" stroke-dasharray="3 4" vector-effect="non-scaling-stroke" opacity="0.7"/>
  <line x1="0" y1="50" x2="300" y2="50" stroke="var(--bulma-border-weak)" stroke-width="1" stroke-dasharray="3 4" vector-effect="non-scaling-stroke" opacity="0.7"/>
  <line x1="0" y1="0" x2="0" y2="80" stroke="var(--bulma-border)" stroke-width="1.5" vector-effect="non-scaling-stroke"/>
  <line x1="0" y1="80" x2="300" y2="80" stroke="var(--bulma-border)" stroke-width="1.5" vector-effect="non-scaling-stroke"/>"#;

pub fn short_date(date_str: &str) -> String {
    if let Ok(d) = chrono::NaiveDate::parse_from_str(date_str, "%Y-%m-%d") {
        format!("{}.{:02}", d.format("%d"), d.format("%m"))
    } else {
        date_str.to_string()
    }
}

/// A self-contained chart block (SVG line + HTML date labels). Shows the empty
/// axes placeholder only when there's no data at all; one point already draws.
pub fn chart_block(dates: &[&str], values: &[f64]) -> String {
    if values.is_empty() {
        return format!(
            r#"<div><svg viewBox="-4 -4 308 88" style="width: 100%; height: auto; display: block;">{AXES}</svg><div style="{LABEL_ROW}"><span></span><span></span></div></div>"#
        );
    }
    let svg = line_chart_svg(values);
    let first = short_date(dates.first().copied().unwrap_or(""));
    let last = if dates.len() > 1 {
        short_date(dates.last().copied().unwrap_or(""))
    } else {
        String::new()
    };
    format!(
        r#"<div>{svg}<div style="{LABEL_ROW}"><span>{first}</span><span>{last}</span></div></div>"#
    )
}

/// A self-contained BAR chart block (compact histogram + HTML date labels), for
/// tiles where a count-per-day reads better as bars than a line (e.g. steps).
/// Bars grow from a zero baseline. Empty data draws the same axes placeholder.
pub fn bar_block(dates: &[&str], values: &[f64]) -> String {
    if values.is_empty() {
        return format!(
            r#"<div><svg viewBox="-4 -4 308 88" style="width: 100%; height: auto; display: block;">{AXES}</svg><div style="{LABEL_ROW}"><span></span><span></span></div></div>"#
        );
    }
    let svg = bar_chart_svg(values);
    let first = short_date(dates.first().copied().unwrap_or(""));
    let last = if dates.len() > 1 {
        short_date(dates.last().copied().unwrap_or(""))
    } else {
        String::new()
    };
    format!(
        r#"<div>{svg}<div style="{LABEL_ROW}"><span>{first}</span><span>{last}</span></div></div>"#
    )
}

fn bar_chart_svg(values: &[f64]) -> String {
    let w = 300.0_f64;
    let h = 80.0_f64;
    let n = values.len();
    // Bars grow from zero, scaled to the tallest day.
    let max_val = values.iter().copied().fold(0.0_f64, f64::max).max(1.0);

    let slot = w / n as f64;
    let bar_w = (slot * 0.6).max(1.0);

    let bars: String = values
        .iter()
        .enumerate()
        .map(|(i, &v)| {
            let cx = (i as f64 + 0.5) * slot;
            let bh = (v / max_val) * h;
            let y = h - bh;
            format!(
                r#"<rect x="{:.1}" y="{:.1}" width="{:.1}" height="{:.1}" rx="1" fill="var(--bulma-link)"/>"#,
                cx - bar_w / 2.0,
                y,
                bar_w,
                bh.max(0.0),
            )
        })
        .collect::<Vec<_>>()
        .join("\n");

    format!(
        r#"<svg viewBox="-4 -4 308 88" style="width: 100%; height: auto; display: block;">
  {AXES}
  {bars}
</svg>"#
    )
}

fn line_chart_svg(values: &[f64]) -> String {
    let min_val = values.iter().copied().fold(f64::INFINITY, f64::min);
    let max_val = values.iter().copied().fold(f64::NEG_INFINITY, f64::max);
    let range = (max_val - min_val).max(0.5);
    let padding = range * 0.15;
    let y_min = min_val - padding;
    let y_range = (max_val + padding) - y_min;

    let w = 300.0_f64;
    let h = 80.0_f64;
    let n = values.len();

    let points: Vec<(f64, f64)> = values
        .iter()
        .enumerate()
        .map(|(i, &v)| {
            let x = if n > 1 { (i as f64 / (n - 1) as f64) * w } else { w / 2.0 };
            let y = h - ((v - y_min) / y_range) * h;
            (x, y)
        })
        .collect();

    let path: String = points
        .iter()
        .enumerate()
        .map(|(i, (x, y))| {
            if i == 0 { format!("M{:.1},{:.1}", x, y) } else { format!("L{:.1},{:.1}", x, y) }
        })
        .collect::<Vec<_>>()
        .join(" ");

    // The area fill and the connecting line only make sense for 2+ points.
    let fill = if n >= 2 {
        let fill_path = format!("{} L{:.1},{:.1} L0,{:.1} Z", path, w, h, h);
        format!(r#"<path d="{fill_path}" fill="var(--bulma-link)" opacity="0.1"/>"#)
    } else {
        String::new()
    };

    let dots: String = points
        .iter()
        .map(|(x, y)| format!(r#"<circle cx="{:.1}" cy="{:.1}" r="2.5" fill="var(--bulma-link)"/>"#, x, y))
        .collect::<Vec<_>>()
        .join("\n");

    format!(
        r#"<svg viewBox="-4 -4 308 88" style="width: 100%; height: auto; display: block;">
  {AXES}
  {fill}
  <path d="{path}" fill="none" stroke="var(--bulma-link)" stroke-width="2" stroke-linejoin="round" stroke-linecap="round" vector-effect="non-scaling-stroke"/>
  {dots}
</svg>"#
    )
}
