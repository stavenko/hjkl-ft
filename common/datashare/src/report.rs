//! Разбор и отрисовка ОТЧЁТА, который человек прислал куратору.
//!
//! Отчёт — не то же, что прежние датасетные снимки: он собирается одним
//! действием за названный срок и несёт то, по чему куратор ведёт работу — вес,
//! шаги, состояние индикаторов и историю планок.
//!
//! Никаких выдуманных значений: отсутствующее приходит `null` и таким же
//! показывается. Неразобранный отчёт — громкая ошибка, а не пустой экран.

use leptos::*;
use serde::Deserialize;

use crate::{state_colors, weight_svg, WeightPoint};

#[derive(Debug, Clone, Deserialize)]
pub struct Period {
    #[serde(default)]
    pub from: String,
    #[serde(default)]
    pub to: String,
    #[serde(default)]
    pub days: u32,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct Body {
    #[serde(default)]
    pub weight_kg: Option<f64>,
    #[serde(default)]
    pub height_cm: Option<f64>,
    #[serde(default)]
    pub birth_year: Option<i32>,
    /// Возраст, посчитанный на стороне ЧЕЛОВЕКА. У куратора своя дата, а нормы
    /// железа идут ступеньками по возрасту — на границе год разницы даёт другую
    /// норму. Старые отчёты его не везут: тогда считаем от года рождения.
    #[serde(default)]
    pub age_years: Option<i32>,
    #[serde(default)]
    pub sex: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct WeightRow {
    pub date: String,
    pub kg: f64,
    #[serde(default)]
    pub morning: bool,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct Weight {
    #[serde(default)]
    pub series: Vec<WeightRow>,
    #[serde(default)]
    pub balance: String,
    #[serde(default)]
    pub slope_kg_per_week: Option<f64>,
    #[serde(default)]
    pub confidence: Option<f64>,
    #[serde(default)]
    pub direction: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct StepRow {
    pub date: String,
    pub steps: u32,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct Steps {
    #[serde(default)]
    pub series: Vec<StepRow>,
}

/// Одна точка ряда индикатора: день (или неделя) со значением и вердиктом.
#[derive(Debug, Clone, Deserialize)]
pub struct Point {
    #[serde(default)]
    pub date: String,
    #[serde(default)]
    pub value: f64,
    #[serde(default)]
    pub ratio: Option<f64>,
    /// `None` — судить было не по чему (нет данных за этот день).
    #[serde(default)]
    pub met: Option<bool>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Indicator {
    pub key: String,
    #[serde(default)]
    pub label: String,
    #[serde(default)]
    pub state: String,
    #[serde(default)]
    pub missed: u32,
    #[serde(default)]
    pub labels: Vec<String>,
    #[serde(default)]
    pub points: Vec<Point>,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct Targets {
    #[serde(default)]
    pub calories: Option<f64>,
    #[serde(default)]
    pub protein: Option<f64>,
    #[serde(default)]
    pub steps: Option<f64>,
    #[serde(default)]
    pub veg_fruit: Option<f64>,
    #[serde(default)]
    pub calcium: Option<f64>,
    #[serde(default)]
    pub fiber: Option<f64>,
    #[serde(default)]
    pub iron: Option<f64>,
    #[serde(default)]
    pub heme: Option<f64>,
    #[serde(default)]
    pub epa_dha: Option<f64>,
    #[serde(default)]
    pub fat_ratio: Option<f64>,
    #[serde(default)]
    pub red_meat: Option<f64>,
    #[serde(default)]
    pub egg: Option<f64>,
}

impl Targets {
    /// Действующее число по ключу индикатора.
    pub fn value(&self, key: &str) -> Option<f64> {
        match key {
            "calories" => self.calories,
            "protein" => self.protein,
            "steps" => self.steps,
            "veg_fruit" => self.veg_fruit,
            "calcium" => self.calcium,
            "fiber" => self.fiber,
            "iron" => self.iron,
            "heme" => self.heme,
            "epa_dha" => self.epa_dha,
            "fat_ratio" => self.fat_ratio,
            "red_meat" => self.red_meat,
            "egg" => self.egg,
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct PlankaPoint {
    pub date: String,
    pub amount: f64,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Report {
    pub period: Period,
    #[serde(default)]
    pub generated_at: String,
    #[serde(default)]
    pub body: Body,
    #[serde(default)]
    pub weight: Weight,
    #[serde(default)]
    pub steps: Steps,
    #[serde(default)]
    pub indicators: Vec<Indicator>,
    #[serde(default)]
    pub targets: Targets,
    #[serde(default)]
    pub plankas: std::collections::BTreeMap<String, Vec<PlankaPoint>>,
    /// Среднее съеденное за 7 завершённых дней — вход `adherence`. `None`, когда
    /// дневник за эту неделю пуст: считать планку не от чего, и врать про это
    /// нулём нельзя.
    #[serde(default)]
    pub avg_kcal_7d: Option<f64>,
}

impl Report {
    /// Снимок человека для правил планок — ровно тот же, что приложение худеющего
    /// собирает из профиля. Пять чисел, и правило не должно знать, откуда они.
    pub fn snapshot(&self) -> plankas::Snapshot {
        plankas::Snapshot {
            sex: match self.body.sex.as_deref() {
                Some("male") => Some(plankas::Sex::Male),
                Some("female") => Some(plankas::Sex::Female),
                _ => None,
            },
            age_years: self.body.age_years,
            height_cm: self.body.height_cm,
            weight_kg: self.body.weight_kg,
            kcal_planka: self.targets.calories,
        }
    }

    /// Ряд веса в том виде, в каком его читает тренд. Условия замера в отчёте
    /// есть, но тренду они не нужны — он смотрит только дату и килограммы.
    pub fn weight_entries(&self) -> Vec<api_types::WeightEntry> {
        self.weight
            .series
            .iter()
            .map(|r| api_types::WeightEntry {
                id: String::new(),
                date: r.date.clone(),
                weight_kg: r.kg,
                no_water: false,
                no_food: false,
                no_wash: false,
                used_toilet: false,
                morning: r.morning,
                created_at: String::new(),
                updated_at: String::new(),
            })
            .collect()
    }

    /// Что предложить куратору: калории и следующий за ними белок, посчитанные
    /// ровно так же, как их посчитал бы недельный цикл у худеющего.
    ///
    /// `None` — планки по калориям ещё нет, отталкиваться не от чего. Это не
    /// ошибка: до второй недели её и не бывает.
    pub fn suggest(&self) -> Option<plankas::Suggestion> {
        let previous = self.targets.calories?;
        Some(plankas::suggest(
            &self.snapshot(),
            previous,
            &self.weight_entries(),
            self.avg_kcal_7d,
        ))
    }
}

#[derive(Debug, Clone, Deserialize)]
struct Envelope {
    report: Report,
}

/// Разобрать присланный отчёт. Ошибка возвращается ТЕКСТОМ и показывается —
/// пустой экран вместо непонятого отчёта скрыл бы поломку протокола.
pub fn parse(raw: &str) -> Result<Report, String> {
    serde_json::from_str::<Envelope>(raw)
        .map(|e| e.report)
        .map_err(|e| format!("отчёт не разобран: {e}"))
}

fn fmt_num(v: f64) -> String {
    if v.abs() >= 100.0 || (v - v.round()).abs() < 1e-9 {
        format!("{v:.0}")
    } else {
        format!("{v:.1}")
    }
}

/// Полоска ряда: по клетке на день или неделю. Зелёная — планка взята, красная —
/// нет, серая — судить было не по чему.
fn series_strip(points: &[Point]) -> View {
    let cells: Vec<View> = points
        .iter()
        .map(|p| {
            let color = match p.met {
                Some(true) => "var(--accent)",
                Some(false) => "var(--danger)",
                None => "var(--line)",
            };
            let title = format!("{} · {}", p.date, fmt_num(p.value));
            view! {
                <span attr:title=title
                    style=format!("flex: 1; height: 22px; border-radius: 4px; background: {color}; \
                                   opacity: .85;")></span>
            }
            .into_view()
        })
        .collect();
    view! { <div style="display: flex; gap: 3px; margin-top: 8px;">{cells}</div> }.into_view()
}

/// Строка индикатора: название, цвет, действующая планка и кнопка правки.
///
/// `on_edit` получает ключ индикатора — правку рисует само приложение, потому что
/// отправка директивы это уже его дело, а не дело разбора отчёта.
pub fn indicator_row(ind: &Indicator, targets: &Targets, on_edit: Callback<String>) -> View {
    let (bg, ink) = state_colors(&ind.state);
    let key = ind.key.clone();
    let key_for_click = key.clone();
    let target = targets.value(&key);
    let points = ind.points.clone();
    let label = if ind.label.is_empty() { key.clone() } else { ind.label.clone() };

    view! {
        <div class="card" style="margin-bottom: 10px;" attr:data-testid="indicator-row">
            <div style="display: flex; align-items: center; gap: 10px;">
                <span style=format!("width: 10px; height: 10px; border-radius: 50%; background: {ink};")></span>
                <span style="font-weight: 620; flex: 1;">{label}</span>
                {target.map(|v| view! {
                    <span style=format!("padding: 3px 9px; border-radius: 999px; background: {bg}; \
                                         color: {ink}; font-size: .8rem; font-weight: 600;")>
                        {fmt_num(v)}
                    </span>
                })}
                <button class="btn btn--icon btn--ghost" attr:data-testid="indicator-edit"
                    on:click=move |_| on_edit.call(key_for_click.clone())>
                    <svg viewBox="0 0 24 24"><path d="M12 20h9"/>
                        <path d="M16.5 3.5a2.12 2.12 0 0 1 3 3L7 19l-4 1 1-4z"/></svg>
                </button>
            </div>
            {(!points.is_empty()).then(|| series_strip(&points))}
        </div>
    }
    .into_view()
}

/// Весь отчёт: шапка периода, вес, шаги, индикаторы, история планок.
pub fn render(report: &Report, on_edit: Callback<String>) -> View {
    let weight_points: Vec<WeightPoint> = report
        .weight
        .series
        .iter()
        .map(|r| WeightPoint { date: r.date.clone(), kg: r.kg })
        .collect();
    let steps = report.steps.series.clone();
    let max_steps = steps.iter().map(|s| s.steps).max().unwrap_or(1).max(1) as f64;
    let indicators: Vec<View> = report
        .indicators
        .iter()
        .map(|i| indicator_row(i, &report.targets, on_edit))
        .collect();

    let planka_blocks: Vec<View> = report
        .plankas
        .iter()
        .filter(|(_, v)| !v.is_empty())
        .map(|(kind, points)| {
            let rows: Vec<View> = points
                .iter()
                .rev()
                .take(12)
                .map(|p| {
                    view! {
                        <div style="display: flex; justify-content: space-between; padding: 4px 0;">
                            <span class="row__meta">{p.date.clone()}</span>
                            <span style="font-weight: 600;">{fmt_num(p.amount)}</span>
                        </div>
                    }
                    .into_view()
                })
                .collect();
            view! {
                <div class="card" style="margin-bottom: 10px;">
                    <p style="font-weight: 620; margin-bottom: 6px;">{kind.clone()}</p>
                    {rows}
                </div>
            }
            .into_view()
        })
        .collect();

    let period = format!("{} — {}", report.period.from, report.period.to);
    view! {
        <div attr:data-testid="client-report">
            <p class="row__meta" style="margin-bottom: 10px;">{period}</p>

            {(!weight_points.is_empty()).then(|| view! {
                <div class="card" style="margin-bottom: 12px;">
                    <p style="font-weight: 620; margin-bottom: 8px;">"Вес"</p>
                    {weight_svg(&weight_points)}
                    <p class="row__meta" style="margin-top: 8px;">
                        {report.weight.slope_kg_per_week
                            .map(|s| format!("{:+.2} кг/нед", s))
                            .unwrap_or_else(|| "тренд не определён".to_string())}
                    </p>
                </div>
            })}

            {(!steps.is_empty()).then(|| {
                let bars: Vec<View> = steps.iter().map(|s| {
                    let h = (s.steps as f64 / max_steps * 100.0).max(2.0);
                    view! {
                        <span attr:title=format!("{} · {}", s.date, s.steps)
                            style=format!("flex: 1; height: {h}%; min-height: 2px; \
                                           background: var(--accent); opacity: .8; border-radius: 3px;")></span>
                    }.into_view()
                }).collect();
                view! {
                    <div class="card" style="margin-bottom: 12px;">
                        <p style="font-weight: 620; margin-bottom: 8px;">"Шаги"</p>
                        <div style="display: flex; align-items: flex-end; gap: 3px; height: 90px;">{bars}</div>
                    </div>
                }
            })}

            {indicators}

            {(!planka_blocks.is_empty()).then(|| view! {
                <p style="font-weight: 620; margin: 18px 0 8px;">"История планок"</p>
                {planka_blocks}
            })}
        </div>
    }
    .into_view()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Отчёт со всеми полями пустыми должен разбираться: значений может не быть,
    /// и это состояние, а не поломка.
    #[test]
    fn pustoj_otchet_razbiraetsya() {
        let raw = r#"{"report":{"period":{"from":"2026-01-01","to":"2026-01-02","days":1}}}"#;
        let r = parse(raw).expect("пустой отчёт обязан разбираться");
        assert_eq!(r.period.days, 1);
        assert!(r.indicators.is_empty());
        assert!(r.targets.calories.is_none());
    }

    /// Расчёт идёт по ВСЕМ данным отчёта, включая среднее съеденное. Проверяется
    /// это единственным честным способом: два отчёта, отличающиеся ТОЛЬКО им,
    /// обязаны дать разные числа.
    ///
    /// Здесь вес падает быстрее комфортной полосы — правило по весу зовёт планку
    /// вверх. Но если человек ест сильно НИЖЕ планки, дело не в её величине, а в
    /// неисполнении: поднимать нельзя, иначе разрыв между предписанным и съеденным
    /// растёт каждую неделю. Стопор — та самая механика, ради которой среднее
    /// съеденное и поехало в отчёте.
    #[test]
    fn raschyot_uchityvaet_sedennoe_a_ne_tolko_ves() {
        let body = r#""body":{"weight_kg":78,"height_cm":170,"age_years":35,"sex":"female"},
            "targets":{"calories":2000},
            "weight":{"series":[
                {"date":"2026-01-01","kg":80.0},{"date":"2026-01-03","kg":79.7},
                {"date":"2026-01-05","kg":79.4},{"date":"2026-01-07","kg":79.1},
                {"date":"2026-01-09","kg":78.8},{"date":"2026-01-11","kg":78.5},
                {"date":"2026-01-13","kg":78.2},{"date":"2026-01-15","kg":78.0}]}"#;
        let held = parse(&format!(
            r#"{{"report":{{"period":{{"from":"a","to":"b","days":14}},{body},"avg_kcal_7d":1400}}}}"#
        ))
        .unwrap()
        .suggest()
        .expect("планка есть — считать есть от чего");
        let moved = parse(&format!(
            r#"{{"report":{{"period":{{"from":"a","to":"b","days":14}},{body},"avg_kcal_7d":2000}}}}"#
        ))
        .unwrap()
        .suggest()
        .unwrap();

        assert_eq!(held.calories, 2000.0, "недоедающему планку не поднимаем");
        assert!(moved.calories > 2000.0, "{}", moved.calories);
        // Белок следует за калориями — значит расходится вместе с ними.
        assert!(held.protein.is_some() && moved.protein.is_some());
        assert_ne!(held.protein, moved.protein);
    }

    /// Планки по калориям ещё нет — предлагать нечего, и выдумывать нельзя.
    #[test]
    fn bez_dejstvuyushchej_planki_raschyota_net() {
        let raw = r#"{"report":{"period":{"from":"a","to":"b","days":7}}}"#;
        assert!(parse(raw).unwrap().suggest().is_none());
    }

    /// Снимок собирается из тела отчёта — теми же пятью числами, что приложение
    /// худеющего берёт из профиля.
    #[test]
    fn snimok_sobiraetsya_iz_tela_otchyota() {
        let raw = r#"{"report":{"period":{"from":"a","to":"b","days":7},
            "body":{"weight_kg":80,"height_cm":170,"age_years":35,"sex":"male"},
            "targets":{"calories":2200}}}"#;
        let s = parse(raw).unwrap().snapshot();
        assert_eq!(s.sex, Some(plankas::Sex::Male));
        assert_eq!(s.age_years, Some(35));
        assert_eq!(s.weight_kg, Some(80.0));
        assert_eq!(s.kcal_planka, Some(2200.0));
        // По нему сразу считаются наши нормы — те же, что у худеющего.
        assert_eq!(
            plankas::default_for(plankas::Kind::Fiber, &s),
            Some(2200.0 / 1000.0 * plankas::defaults::G_PER_1000_KCAL)
        );
    }

    /// Мусор — громкая ошибка, а не пустой экран.
    #[test]
    fn musor_eto_oshibka() {
        assert!(parse("{}").is_err());
        assert!(parse("не json").is_err());
    }

    /// Планка находится по ключу вида — и это ЕДИНСТВЕННЫЙ вопрос, который к ней
    /// осмысленно задавать. «Кто поставил» больше не различается: планка живёт в
    /// одном месте, и куратор правит ровно её.
    #[test]
    fn planka_nahoditsya_po_klyuchu() {
        let raw = r#"{"report":{"period":{"from":"a","to":"b","days":7},
            "targets":{"calories":1800,"fiber":32}}}"#;
        let r = parse(raw).unwrap();
        assert_eq!(r.targets.value("calories"), Some(1800.0));
        assert_eq!(r.targets.value("fiber"), Some(32.0));
        assert_eq!(r.targets.value("steps"), None);
        assert_eq!(r.targets.value("чушь"), None);
    }
}
