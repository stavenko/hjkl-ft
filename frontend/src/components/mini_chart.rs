//! Shared mini line-chart used by the weight and steps widgets/modals.
//! The line/area/dots are SVG (scaled uniformly — no distortion), while the
//! date labels are plain HTML so they stay readable at any width.

const LABEL_ROW: &str = "display: flex; justify-content: space-between; font-size: 10px; color: var(--bulma-text-weak); margin-top: 2px; min-height: 12px;";

/// Ширина поля графика в координатах SVG. Одна на все размеры: ужимается только
/// высота, растягивать рисунок по горизонтали незачем.
const CH_W: f64 = 300.0;

/// Высота поля в раскрытой панели — та, что была у графика всегда.
pub const CH_FULL: f64 = 80.0;

/// Половинная высота — для плиток на дашборде, где место дороже подробности.
/// Рисунок тот же, просто ниже: форма кривой читается, а плитка занимает вдвое
/// меньше экрана.
pub const CH_HALF: f64 = 40.0;

/// Y axis, X axis and two faint gridlines — drawn in every chart (with or
/// without data). Доли высоты сохранены от исходных 20 и 50 при 80.
fn axes(h: f64) -> String {
    let (g1, g2) = (h * 0.25, h * 0.625);
    format!(
        r#"<line x1="0" y1="{g1:.1}" x2="{CH_W:.0}" y2="{g1:.1}" stroke="var(--bulma-border-weak)" stroke-width="1" stroke-dasharray="3 4" vector-effect="non-scaling-stroke" opacity="0.7"/>
  <line x1="0" y1="{g2:.1}" x2="{CH_W:.0}" y2="{g2:.1}" stroke="var(--bulma-border-weak)" stroke-width="1" stroke-dasharray="3 4" vector-effect="non-scaling-stroke" opacity="0.7"/>
  <line x1="0" y1="0" x2="0" y2="{h:.0}" stroke="var(--bulma-border)" stroke-width="1.5" vector-effect="non-scaling-stroke"/>
  <line x1="0" y1="{h:.0}" x2="{CH_W:.0}" y2="{h:.0}" stroke="var(--bulma-border)" stroke-width="1.5" vector-effect="non-scaling-stroke"/>"#
    )
}

/// `viewBox` поля: четыре пикселя полей со всех сторон, чтобы точки и штрихи на
/// краю не срезались.
fn view_box(h: f64) -> String {
    format!("-4 -4 {:.0} {:.0}", CH_W + 8.0, h + 8.0)
}

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
    chart_block_with_planka(dates, values, &[], CH_FULL)
}

/// То же, плюс ИСТОРИЯ планки поверх — по одному значению на точку графика.
///
/// Единицы не совпадают: планка калорий измеряется тысячами ккал, вес — десятками
/// килограммов, и в одной шкале линия планки ушла бы за поле. Поэтому она
/// НОРМИРУЕТСЯ: собственный диапазон планки растягивается на ту же высоту, что и
/// график. Видна не величина, а форма — когда планка росла, когда падала и как это
/// легло на вес. Постоянная планка рисуется посередине.
pub fn chart_block_with_planka(
    dates: &[&str],
    values: &[f64],
    planka: &[Option<f64>],
    h: f64,
) -> String {
    if values.is_empty() {
        let (vb, ax) = (view_box(h), axes(h));
        return format!(
            r#"<div><svg viewBox="{vb}" style="width: 100%; height: auto; display: block;">{ax}</svg><div style="{LABEL_ROW}"><span></span><span></span></div></div>"#
        );
    }
    let svg = line_chart_svg_with_planka(values, planka, h);
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

// ── Steps histogram ─────────────────────────────────────────────────────────
// The bars stay neutral blue; the planka/average is conveyed by the LINE colour
// only (green planka line vs pink average line). Green bars on a green line all
// blended together, so the target is shown by the line, not by recolouring bars.
const STEPS_LINE: &str = "#1fa463"; // the planka line (green)
const STEPS_AVG: &str = "#e0699b"; // the average line (before a planka is set)

/// Steps histogram block (compact bars + HTML date labels) with a reference line:
/// - `planka: Some(p)` → a GREEN target line at `p`.
/// - `planka: None`    → a PINK average line over the logged past days (today
///   excluded), shown BEFORE the steps planka has been computed/applied.
/// Bars are always the neutral link colour.
pub fn steps_bar_block(dates: &[&str], values: &[f64], planka: Option<f64>, h: f64) -> String {
    if values.is_empty() {
        let (vb, ax) = (view_box(h), axes(h));
        return format!(
            r#"<div><svg viewBox="{vb}" style="width: 100%; height: auto; display: block;">{ax}</svg><div style="{LABEL_ROW}"><span></span><span></span></div></div>"#
        );
    }
    let svg = steps_bar_svg(values, planka, h);
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

/// Average over the logged days EXCLUDING today (the last point — a still-partial
/// day). Unlogged (zero) days don't count. `None` when there's nothing to average.
pub fn avg_past(values: &[f64]) -> Option<f64> {
    if values.len() < 2 {
        return None;
    }
    let logged: Vec<f64> =
        values[..values.len() - 1].iter().copied().filter(|v| *v > 0.0).collect();
    (!logged.is_empty()).then(|| logged.iter().sum::<f64>() / logged.len() as f64)
}

fn steps_bar_svg(values: &[f64], planka: Option<f64>, h: f64) -> String {
    let w = CH_W;
    let n = values.len();

    // Reference line: the planka once it's set, otherwise the running average.
    let line = planka.or_else(|| avg_past(values));
    // Bars grow from zero, scaled to the tallest day OR the reference line (so the
    // line always fits on the chart).
    let max_val = values
        .iter()
        .copied()
        .fold(0.0_f64, f64::max)
        .max(line.unwrap_or(0.0))
        .max(1.0);

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

    // Reference line ON TOP of the bars: green for the planka, pink for the average.
    let line_color = if planka.is_some() { STEPS_LINE } else { STEPS_AVG };
    let target = line
        .map(|p| {
            let y = h - (p / max_val) * h;
            format!(
                r#"<line x1="0" y1="{y:.1}" x2="{w:.0}" y2="{y:.1}" stroke="{line_color}" stroke-width="1.5" stroke-dasharray="4 3" vector-effect="non-scaling-stroke"/>"#
            )
        })
        .unwrap_or_default();

    let (vb, ax) = (view_box(h), axes(h));
    format!(
        r#"<svg viewBox="{vb}" style="width: 100%; height: auto; display: block;">
  {ax}
  {bars}
  {target}
</svg>"#
    )
}

fn line_chart_svg_with_planka(values: &[f64], planka: &[Option<f64>], h: f64) -> String {
    let min_val = values.iter().copied().fold(f64::INFINITY, f64::min);
    let max_val = values.iter().copied().fold(f64::NEG_INFINITY, f64::max);
    let range = (max_val - min_val).max(0.5);
    let padding = range * 0.15;
    let y_min = min_val - padding;
    let y_range = (max_val + padding) - y_min;

    let w = CH_W;
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

    // Линия планки — НОРМИРОВАННАЯ: её собственный размах растягивается на ту же
    // высоту, что и график. Величины несопоставимы (ккал против килограммов), и в
    // общей шкале линия ушла бы за поле; здесь читается форма — когда планка росла и
    // как на это ответил вес. Постоянная планка ложится посередине.
    let known: Vec<f64> = planka.iter().flatten().copied().collect();
    let planka_path = if known.len() >= 2 || (known.len() == 1 && n >= 1) {
        let pmin = known.iter().copied().fold(f64::INFINITY, f64::min);
        let pmax = known.iter().copied().fold(f64::NEG_INFINITY, f64::max);
        let prange = pmax - pmin;
        let mut d = String::new();
        let mut started = false;
        for (i, p) in planka.iter().enumerate().take(n) {
            let Some(v) = p else { started = false; continue };
            let x = if n > 1 { (i as f64 / (n - 1) as f64) * w } else { w / 2.0 };
            // Нулевой размах (планка не менялась) → середина поля.
            // Держим линию в средних 70 % высоты: растянутая на всё поле, она упирается
            // в верхний и нижний края и выглядит обрезанной.
            let norm = if prange > f64::EPSILON { (v - pmin) / prange } else { 0.5 };
            let y = h - (0.15 + norm * 0.7) * h;
            d.push_str(&format!(
                "{}{:.1},{:.1} ",
                if started { "L" } else { "M" },
                x,
                y
            ));
            started = true;
        }
        // Разделитель `r##`, а не `r#`: внутри есть `"#1fa463"`, и последовательность
        // `"#` закрыла бы обычную raw-строку прямо на цвете.
        // Подписана КАЖДАЯ ступень на своём уровне. Линия нормирована, высота сама по
        // себе ничего не говорит — без чисел видно только, что планка менялась.
        let mut labels = String::new();
        let mut seen: Option<f64> = None;
        for (i, p) in planka.iter().enumerate().take(n) {
            let Some(v) = p else { continue };
            if seen.map_or(true, |s| (s - *v).abs() > f64::EPSILON) {
                let x = if n > 1 { (i as f64 / (n - 1) as f64) * w } else { w / 2.0 };
                let norm = if prange > f64::EPSILON { (v - pmin) / prange } else { 0.5 };
                let y = h - (0.15 + norm * 0.7) * h;
                // У правого края подпись уводим влево, иначе она уедет за поле.
                let (anchor, tx) = if x > w * 0.8 { ("end", x - 2.0) } else { ("start", x + 2.0) };
                // Обводка фоном: подпись ложится поверх линии веса и без неё сливается.
                labels.push_str(&format!(
                    r##"<text x="{tx:.1}" y="{:.1}" text-anchor="{anchor}" fill="#1fa463" font-size="7.5" font-weight="700" stroke="var(--bulma-scheme-main)" stroke-width="2.5" paint-order="stroke" stroke-linejoin="round">{v:.0}</text>"##,
                    y - 3.0
                ));
                seen = Some(*v);
            }
        }
        (
            format!(
                r##"<path d="{}" fill="none" stroke="#1fa463" stroke-width="1.5" stroke-dasharray="4 3" stroke-linejoin="round" vector-effect="non-scaling-stroke" opacity="0.85"/>"##,
                d.trim()
            ),
            labels,
        )
    } else {
        (String::new(), String::new())
    };
    // Подписи идут ПОСЛЕДНИМИ: линия веса и её точки рисуются поверх всего, что
    // раньше, и накрывали цифры.
    let (planka_path, planka_labels) = planka_path;

    let (vb, ax) = (view_box(h), axes(h));
    format!(
        r#"<svg viewBox="{vb}" style="width: 100%; height: auto; display: block;">
  {ax}
  {fill}
  {planka_path}
  <path d="{path}" fill="none" stroke="var(--bulma-link)" stroke-width="2" stroke-linejoin="round" stroke-linecap="round" vector-effect="non-scaling-stroke"/>
  {dots}
  {planka_labels}
</svg>"#
    )
}
