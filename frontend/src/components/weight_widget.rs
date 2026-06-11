use leptos::*;
use api_types::WeightEntry;

use crate::services::i18n::{t, weight_unit_signal, WeightUnit};

const CARD: &str = "background: var(--bulma-scheme-main); border-radius: 12px; padding: 12px 16px; margin-bottom: 0.75rem;";

const PLACEHOLDER_CHART: &str = r#"<svg viewBox="-10 -5 320 100" style="width: 100%; height: 80px;" preserveAspectRatio="none">
  <path d="M0,10 C40,15 60,20 100,25 C140,30 160,22 200,40 C240,55 270,60 300,65" fill="none" stroke="var(--bulma-link)" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" vector-effect="non-scaling-stroke" opacity="0.35"/>
  <path d="M0,10 C40,15 60,20 100,25 C140,30 160,22 200,40 C240,55 270,60 300,65 L300,80 L0,80 Z" fill="var(--bulma-link)" opacity="0.08"/>
  <circle cx="0" cy="10" r="2.5" fill="var(--bulma-link)" opacity="0.35"/>
  <circle cx="100" cy="25" r="2.5" fill="var(--bulma-link)" opacity="0.35"/>
  <circle cx="200" cy="40" r="2.5" fill="var(--bulma-link)" opacity="0.35"/>
  <circle cx="300" cy="65" r="2.5" fill="var(--bulma-link)" opacity="0.35"/>
</svg>"#;

#[component]
pub fn WeightWidget(entries: Signal<Vec<WeightEntry>>) -> impl IntoView {
    let unit = weight_unit_signal();

    view! {
        {move || {
            let mut es = entries.get();
            if es.is_empty() {
                return view! {}.into_view();
            }

            es.sort_by(|a, b| a.date.cmp(&b.date));
            let u = unit.get();

            if es.len() < 3 {
                view! {
                    <div style=CARD>
                        <div style="position: relative;">
                            <div inner_html=PLACEHOLDER_CHART></div>
                            <div style="position: absolute; inset: 0; display: flex; align-items: center; justify-content: center; padding: 0 24px;">
                                <span class="is-size-7" style="text-align: center; color: var(--bulma-text-weak); background: color-mix(in srgb, var(--bulma-scheme-main) 70%, transparent); border-radius: 8px; padding: 4px 12px;">
                                    {move || t("weight.widget_placeholder")}
                                </span>
                            </div>
                        </div>
                    </div>
                }.into_view()
            } else {
                let chart = render_chart(&es, u);
                view! {
                    <div style=CARD>
                        <div style="display: flex; align-items: baseline; justify-content: space-between; margin-bottom: 4px;">
                            <span class="is-size-7 has-text-grey">{move || t("weight.widget_title")}</span>
                            {move || {
                                let es2 = entries.get();
                                let last = es2.last().unwrap();
                                let val = unit.get().from_kg(last.weight_kg);
                                let unit_label = match unit.get() {
                                    WeightUnit::Kg => t("weight.unit_kg"),
                                    WeightUnit::Lbs => t("weight.unit_lbs"),
                                };
                                view! {
                                    <span class="is-size-6 has-text-weight-semibold">
                                        {format!("{:.1} {}", val, unit_label)}
                                    </span>
                                }
                            }}
                        </div>
                        <div inner_html=chart></div>
                    </div>
                }.into_view()
            }
        }}
    }
}

fn render_chart(entries: &[WeightEntry], unit: WeightUnit) -> String {
    let values: Vec<f64> = entries.iter().map(|e| unit.from_kg(e.weight_kg)).collect();
    let dates: Vec<&str> = entries.iter().map(|e| e.date.as_str()).collect();

    let min_val = values.iter().copied().fold(f64::INFINITY, f64::min);
    let max_val = values.iter().copied().fold(f64::NEG_INFINITY, f64::max);
    let range = (max_val - min_val).max(0.5);
    let padding = range * 0.15;
    let y_min = min_val - padding;
    let y_max = max_val + padding;
    let y_range = y_max - y_min;

    let w = 300.0_f64;
    let h = 80.0_f64;
    let n = values.len();

    let points: Vec<(f64, f64)> = values
        .iter()
        .enumerate()
        .map(|(i, &v)| {
            let x = if n > 1 {
                (i as f64 / (n - 1) as f64) * w
            } else {
                w / 2.0
            };
            let y = h - ((v - y_min) / y_range) * h;
            (x, y)
        })
        .collect();

    let path: String = points
        .iter()
        .enumerate()
        .map(|(i, (x, y))| {
            if i == 0 {
                format!("M{:.1},{:.1}", x, y)
            } else {
                format!("L{:.1},{:.1}", x, y)
            }
        })
        .collect::<Vec<_>>()
        .join(" ");

    let fill_path = format!(
        "{} L{:.1},{:.1} L0,{:.1} Z",
        path,
        w,
        h,
        h
    );

    let dots: String = points
        .iter()
        .map(|(x, y)| {
            format!(
                r#"<circle cx="{:.1}" cy="{:.1}" r="2.5" fill="var(--bulma-link)"/>"#,
                x, y
            )
        })
        .collect::<Vec<_>>()
        .join("\n");

    let first_label = short_date(dates.first().unwrap_or(&""));
    let last_label = short_date(dates.last().unwrap_or(&""));

    format!(
        r#"<svg viewBox="-10 -5 320 100" style="width: 100%; height: 80px;" preserveAspectRatio="none">
  <path d="{fill_path}" fill="var(--bulma-link)" opacity="0.1"/>
  <path d="{path}" fill="none" stroke="var(--bulma-link)" stroke-width="2" stroke-linejoin="round" stroke-linecap="round" vector-effect="non-scaling-stroke"/>
  {dots}
  <text x="0" y="95" font-size="10" fill="var(--bulma-text-weak)" dominant-baseline="auto">{first_label}</text>
  <text x="300" y="95" font-size="10" fill="var(--bulma-text-weak)" text-anchor="end" dominant-baseline="auto">{last_label}</text>
</svg>"#
    )
}

fn short_date(date_str: &str) -> String {
    if let Ok(d) = chrono::NaiveDate::parse_from_str(date_str, "%Y-%m-%d") {
        format!("{}.{:02}", d.format("%d"), d.format("%m"))
    } else {
        date_str.to_string()
    }
}
