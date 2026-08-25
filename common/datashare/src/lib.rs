//! Rendering of typed data_share payloads shared by the user over the support
//! chat. A `data_share` message carries a `payload` (raw JSON) for one dataset
//! (body/weight/steps/food) or, for the "all" request, a nested object
//! keyed by dataset. We render each dataset as a labelled button in the thread;
//! tapping it opens a modal that mirrors the client's own views.
//!
//! FAIL LOUDLY: nothing here fabricates data — we render exactly what the user
//! shared, and unparsable payloads surface an explicit error rather than an
//! empty/placeholder view.

use leptos::*;
use serde::Deserialize;

pub mod report;

// ── Payload shapes (must match the protocol; every new field is #[serde(default)]
//    so older shares still parse) ──

#[derive(Debug, Clone, Deserialize)]
pub struct BodyShare {
    #[serde(default)]
    pub weight_kg: Option<f64>,
    #[serde(default)]
    pub height_cm: Option<f64>,
    #[serde(default)]
    pub birth_year: Option<i32>,
    #[serde(default)]
    pub sex: Option<String>, // "male" | "female"
}

#[derive(Debug, Clone, Deserialize)]
pub struct WeightPoint {
    pub date: String,
    pub kg: f64,
}

#[derive(Debug, Clone, Deserialize)]
pub struct WeightShare {
    #[serde(default)]
    pub series: Vec<WeightPoint>,
    #[serde(default)]
    pub balance: Option<String>, // "deficit" | "maintenance" | "surplus"
    #[serde(default)]
    pub slope_kg_per_week: Option<f64>,
    #[serde(default)]
    pub confidence: Option<f64>,
    #[serde(default)]
    pub direction: Option<String>, // "down" | "up"
    #[serde(default)]
    pub days: Option<i64>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct StepPoint {
    pub date: String,
    pub steps: f64,
}

#[derive(Debug, Clone, Deserialize)]
pub struct StepsShare {
    #[serde(default)]
    pub series: Vec<StepPoint>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct FoodEntry {
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub grams: f64,
    #[serde(default)]
    pub kcal: f64,
    #[serde(default)]
    pub protein: f64,
    #[serde(default)]
    pub fat: f64,
    #[serde(default)]
    pub carbs: f64,
}

#[derive(Debug, Clone, Deserialize)]
pub struct FoodTotals {
    #[serde(default)]
    pub kcal: f64,
    #[serde(default)]
    pub protein: f64,
    #[serde(default)]
    pub fat: f64,
    #[serde(default)]
    pub carbs: f64,
}

#[derive(Debug, Clone, Deserialize)]
pub struct FoodDay {
    pub date: String,
    #[serde(default)]
    pub entries: Vec<FoodEntry>,
    #[serde(default)]
    pub totals: FoodTotals,
    #[serde(default)]
    pub good: Vec<String>,
    #[serde(default)]
    pub improve: Vec<String>,
}

impl Default for FoodTotals {
    fn default() -> Self {
        FoodTotals { kcal: 0.0, protein: 0.0, fat: 0.0, carbs: 0.0 }
    }
}

/// One indicator's state in the food share: short key, RU label, colour state
/// ("green" | "orange" | "red" | "unknown").
#[derive(Debug, Clone, Deserialize)]
pub struct IndicatorShare {
    #[serde(default)]
    pub label: String,
    #[serde(default)]
    pub state: String,
}

/// One day's calorie-planka adherence: intake vs the planka that applied that day.
#[derive(Debug, Clone, Deserialize)]
pub struct PlankaDay {
    #[serde(default)]
    pub date: String,
    #[serde(default)]
    pub intake: f64,
    #[serde(default)]
    pub planka: Option<f64>,
    #[serde(default)]
    pub within: Option<bool>,
    /// The in-progress current day — shown neutrally («Сегодня»), not pass/fail.
    #[serde(default)]
    pub today: bool,
}

#[derive(Debug, Clone, Deserialize)]
pub struct FoodShare {
    #[serde(default)]
    pub days: Vec<FoodDay>,
    /// The full indicator board (calories/protein/…/steps). Empty on older clients.
    #[serde(default)]
    pub indicators: Vec<IndicatorShare>,
    /// Per-day calorie-planka adherence, newest first.
    #[serde(default)]
    pub planka_days: Vec<PlankaDay>,
}

/// One dataset extracted from a data_share message, ready to render as a button.
#[derive(Clone)]
pub enum Dataset {
    Body(BodyShare),
    Weight(WeightShare),
    Steps(StepsShare),
    Food(FoodShare),
    /// Environment diagnostics — a free-form key→value map (the client decides
    /// which signals to include; render generically so old admin builds never
    /// choke on new fields).
    System(serde_json::Map<String, serde_json::Value>),
}

impl Dataset {
    /// The button label (emoji + RU name) shown in the thread.
    pub fn label(&self) -> &'static str {
        match self {
            Dataset::Body(_) => "📋 Параметры тела",
            Dataset::Weight(_) => "📊 Дневник веса",
            Dataset::Steps(_) => "👟 Дневник шагов",
            Dataset::Food(_) => "🍽 Дневник питания",
            Dataset::System(_) => "🛠 Устройство и браузер",
        }
    }

    /// Modal title.
    pub fn title(&self) -> &'static str {
        match self {
            Dataset::Body(_) => "Параметры тела",
            Dataset::Weight(_) => "Дневник веса",
            Dataset::Steps(_) => "Дневник шагов",
            Dataset::Food(_) => "Дневник питания",
            Dataset::System(_) => "Устройство и браузер",
        }
    }
}

/// Parse one dataset by key from a raw JSON value. Returns None if the key holds
/// null; returns Err with a loud message if present-but-unparsable.
fn parse_one(key: &str, v: &serde_json::Value) -> Result<Option<Dataset>, String> {
    if v.is_null() {
        return Ok(None);
    }
    let ds = match key {
        "body" => Dataset::Body(
            serde_json::from_value(v.clone()).map_err(|e| format!("body: {e}"))?,
        ),
        "weight" => Dataset::Weight(
            serde_json::from_value(v.clone()).map_err(|e| format!("weight: {e}"))?,
        ),
        "steps" => Dataset::Steps(
            serde_json::from_value(v.clone()).map_err(|e| format!("steps: {e}"))?,
        ),
        "food" => Dataset::Food(
            serde_json::from_value(v.clone()).map_err(|e| format!("food: {e}"))?,
        ),
        "system" => Dataset::System(
            v.as_object()
                .cloned()
                .ok_or_else(|| format!("system: not an object: {v}"))?,
        ),
        other => return Err(format!("unknown dataset key: {other}")),
    };
    Ok(Some(ds))
}

/// Split a data_share payload into its datasets. A single-dataset share is
/// distinguished from an "all" bundle by whether the payload contains the
/// dataset keys at the top level (the bundle) — otherwise we probe each known
/// key. FAIL LOUDLY on a present-but-broken payload.
pub fn datasets_from_payload(payload: &serde_json::Value) -> Result<Vec<Dataset>, String> {
    const KEYS: [&str; 5] = ["body", "weight", "steps", "food", "system"];
    let mut out = Vec::new();
    let mut matched_any_key = false;

    // "all" (or any single-dataset envelope) is an object whose keys are dataset
    // names. Iterate the known keys; whatever is present-and-non-null becomes a
    // button. This also covers a single dataset nested under its own key.
    if payload.is_object() {
        for key in KEYS {
            if let Some(v) = payload.get(key) {
                matched_any_key = true;
                if let Some(ds) = parse_one(key, v)? {
                    out.push(ds);
                }
            }
        }
    }

    // If none of the dataset keys were present at the top level, the payload is a
    // bare single-dataset object. We cannot tell which from structure alone, so
    // this path should not happen given the protocol always keys shares. Surface
    // it loudly rather than guessing.
    if !matched_any_key {
        return Err(
            "data_share payload has no known dataset key (body/weight/steps/food/system)".to_string(),
        );
    }
    Ok(out)
}

// ── Modal body renderers ──

fn body_view(b: &BodyShare) -> impl IntoView {
    let age = b.birth_year.map(|y| {
        let now_year = js_sys::Date::new_0().get_full_year() as i32;
        (now_year - y).max(0)
    });
    let sex = match b.sex.as_deref() {
        Some("male") => "мужской",
        Some("female") => "женский",
        _ => "—",
    };
    let fact = |label: &str, val: String| {
        view! {
            <div style="display:flex; justify-content:space-between; padding:9px 0; border-bottom:1px solid var(--line-soft);">
                <span style="color:var(--muted);">{label.to_string()}</span>
                <span class="mono" style="font-weight:600;">{val}</span>
            </div>
        }
    };
    view! {
        <div>
            {fact("Вес", b.weight_kg.map(|w| format!("{w:.1} кг")).unwrap_or_else(|| "—".into()))}
            {fact("Рост", b.height_cm.map(|h| format!("{h:.0} см")).unwrap_or_else(|| "—".into()))}
            {fact("Возраст", age.map(|a| format!("{a} лет")).unwrap_or_else(|| "—".into()))}
            {fact("Пол", sex.to_string())}
        </div>
    }
}

fn weight_view(w: &WeightShare) -> impl IntoView {
    // Balance line, coloured. deficit → accent (good default), surplus → danger, else muted.
    let (balance_ru, balance_color) = match w.balance.as_deref() {
        Some("deficit") => ("Дефицит", "var(--accent)"),
        Some("maintenance") => ("Поддержка", "var(--muted)"),
        Some("surplus") => ("Профицит", "var(--danger)"),
        _ => ("—", "var(--muted)"),
    };
    let slope = w
        .slope_kg_per_week
        .map(|s| format!("{s:+.2} кг/нед"))
        .unwrap_or_else(|| "—".into());
    let conf = w
        .confidence
        .map(|c| format!("{:.0}%", (c * 100.0).clamp(0.0, 100.0)))
        .unwrap_or_else(|| "—".into());
    let days = w.days.map(|d| format!("{d} дн")).unwrap_or_default();

    let svg = weight_svg(&w.series);

    view! {
        <div>
            <div style=format!(
                "display:flex; align-items:baseline; gap:10px; margin-bottom:10px; \
                 font-weight:700; font-size:1.05rem; color:{balance_color};")>
                {balance_ru}
                <span class="mono" style="font-weight:500; font-size:.85rem; color:var(--muted);">{slope}</span>
            </div>
            {svg}
            <div class="row__meta" style="margin-top:8px;">
                "уверенность "{conf}" · "{days}
            </div>
        </div>
    }
}

/// Inline SVG line chart over the FULL weight series (no external libs).
pub fn weight_svg(series: &[WeightPoint]) -> View {
    if series.len() < 2 {
        return view! {
            <div class="row__meta">"Недостаточно точек для графика"</div>
        }
        .into_view();
    }
    let (w, h) = (600.0_f64, 200.0_f64);
    let (pad_l, pad_r, pad_t, pad_b) = (8.0_f64, 8.0_f64, 10.0_f64, 10.0_f64);
    let min_kg = series.iter().map(|p| p.kg).fold(f64::INFINITY, f64::min);
    let max_kg = series.iter().map(|p| p.kg).fold(f64::NEG_INFINITY, f64::max);
    let span = (max_kg - min_kg).max(0.001);
    let n = series.len() as f64;
    let plot_w = w - pad_l - pad_r;
    let plot_h = h - pad_t - pad_b;

    let pt = |i: usize, kg: f64| -> (f64, f64) {
        let x = pad_l + (i as f64) / (n - 1.0) * plot_w;
        let y = pad_t + (1.0 - (kg - min_kg) / span) * plot_h;
        (x, y)
    };

    let mut d = String::new();
    for (i, p) in series.iter().enumerate() {
        let (x, y) = pt(i, p.kg);
        if i == 0 {
            d.push_str(&format!("M{x:.1} {y:.1}"));
        } else {
            d.push_str(&format!(" L{x:.1} {y:.1}"));
        }
    }
    // Endpoint dots.
    let (fx, fy) = pt(0, series[0].kg);
    let (lx, ly) = pt(series.len() - 1, series[series.len() - 1].kg);

    view! {
        <svg viewBox=format!("0 0 {w} {h}") preserveAspectRatio="none"
             style="width:100%; height:200px; display:block; background:var(--surface-2); border-radius:10px;">
            <path d=d fill="none" stroke="var(--accent)" stroke-width="2.5"
                  stroke-linecap="round" stroke-linejoin="round" vector-effect="non-scaling-stroke"/>
            <circle cx=format!("{fx:.1}") cy=format!("{fy:.1}") r="3.5" fill="var(--accent)"/>
            <circle cx=format!("{lx:.1}") cy=format!("{ly:.1}") r="3.5" fill="var(--accent)"/>
        </svg>
        <div class="row__meta" style="display:flex; justify-content:space-between;">
            <span class="mono">{format!("{min_kg:.1} кг")}</span>
            <span class="mono">{format!("{max_kg:.1} кг")}</span>
        </div>
    }
    .into_view()
}

fn steps_view(s: &StepsShare) -> impl IntoView {
    let series = s.series.clone();
    if series.is_empty() {
        return view! { <div class="row__meta">"Нет данных о шагах"</div> }.into_view();
    }
    let max_steps = series.iter().map(|p| p.steps).fold(0.0_f64, f64::max).max(1.0);
    view! {
        <div style="display:flex; flex-direction:column; gap:6px;">
            {series.into_iter().map(|p| {
                let pct = (p.steps / max_steps * 100.0).clamp(0.0, 100.0);
                view! {
                    <div style="display:flex; align-items:center; gap:10px;">
                        <span class="mono" style="width:88px; flex:none; color:var(--muted); font-size:.82rem;">
                            {p.date}
                        </span>
                        <div style="flex:1; height:14px; background:var(--surface-2); border-radius:7px; overflow:hidden;">
                            <div style=format!(
                                "height:100%; width:{pct:.1}%; background:var(--accent); border-radius:7px;")></div>
                        </div>
                        <span class="mono" style="width:60px; flex:none; text-align:right; font-weight:600;">
                            {format!("{:.0}", p.steps)}
                        </span>
                    </div>
                }
            }).collect_view()}
        </div>
    }
    .into_view()
}

/// (soft background, ink) CSS-var pair for an indicator colour state.
pub fn state_colors(state: &str) -> (&'static str, &'static str) {
    match state {
        "green" => ("var(--accent-soft)", "var(--accent)"),
        "orange" => ("var(--warn-soft)", "var(--warn-ink)"),
        "red" => ("var(--danger-soft)", "var(--danger)"),
        _ => ("var(--surface-2)", "var(--muted)"),
    }
}

fn food_view(f: &FoodShare) -> impl IntoView {
    let days = f.days.clone();
    let indicators = f.indicators.clone();
    let planka_days = f.planka_days.clone();
    if days.is_empty() && indicators.is_empty() {
        return view! { <div class="row__meta">"Нет дней в дневнике"</div> }.into_view();
    }
    // Default to the newest day (index 0 — payload is newest-first).
    let sel = create_rw_signal(0usize);
    let days_for_sel = days.clone();

    view! {
        <div>
            // Indicator board — every indicator (incl. calorie-planka adherence),
            // coloured by state, so the curator reads diet health at a glance.
            {(!indicators.is_empty()).then(|| view! {
                <div style="display:flex; flex-wrap:wrap; gap:6px; margin-bottom:12px;">
                    {indicators.into_iter().map(|ind| {
                        let (bg, fg) = state_colors(&ind.state);
                        view! {
                            <span style=format!("padding:4px 9px; border-radius:8px; font-size:.82rem; \
                                                 background:{bg}; color:{fg}; font-weight:600;")>
                                {ind.label}
                            </span>
                        }
                    }).collect_view()}
                </div>
            })}
            // Calorie-planka adherence per day: intake / that-day's planka, green if
            // kept, red if over — "как соблюдалась планка последние дни".
            {(!planka_days.is_empty()).then(|| view! {
                <div style="margin-bottom:14px; padding:12px; background:var(--surface-2); border-radius:10px;">
                    <div style="font-weight:650; margin-bottom:6px;">"Планка по калориям"</div>
                    {planka_days.into_iter().map(|p| {
                        let within = p.within.unwrap_or(true);
                        let planka_s = p.planka.map(|v| format!("{v:.0}")).unwrap_or_else(|| "—".to_string());
                        // Today is unfinished → neutral colour + «Сегодня» label, so it
                        // doesn't read as a passed/failed day.
                        let (label, color, extra) = if p.today {
                            ("Сегодня".to_string(), "var(--muted)", " · день идёт")
                        } else {
                            (p.date.clone(), if within { "var(--accent)" } else { "var(--danger)" }, "")
                        };
                        let day_style = if p.today { "color:var(--muted); font-weight:600;" } else { "" };
                        view! {
                            <div class="row__meta mono" style="display:flex; justify-content:space-between; padding:3px 0;">
                                <span style=day_style>{label}</span>
                                <span style=format!("color:{color};")>
                                    {format!("{:.0} / {} ккал{}", p.intake, planka_s, extra)}
                                </span>
                            </div>
                        }
                    }).collect_view()}
                </div>
            })}
            // Day selector.
            <div style="display:flex; gap:6px; overflow-x:auto; padding-bottom:8px; margin-bottom:10px;">
                {days.iter().enumerate().map(|(i, d)| {
                    let date = d.date.clone();
                    view! {
                        <button
                            class=move || if sel.get() == i { "seg__btn seg__btn--on" } else { "seg__btn" }
                            style="flex:none; padding:6px 12px; border:1px solid var(--line); border-radius:8px;"
                            on:click=move |_| sel.set(i)>
                            {date}
                        </button>
                    }
                }).collect_view()}
            </div>

            {move || {
                let i = sel.get();
                let d = match days_for_sel.get(i) {
                    Some(d) => d.clone(),
                    None => return view! { <div class="row__meta">"—"</div> }.into_view(),
                };
                let entries = d.entries.clone();
                let good = d.good.clone();
                let improve = d.improve.clone();
                let t = d.totals.clone();
                view! {
                    <div>
                        // Entries.
                        <div style="display:flex; flex-direction:column;">
                            {entries.into_iter().map(|e| view! {
                                <div style="padding:8px 0; border-bottom:1px solid var(--line-soft);">
                                    <div style="display:flex; justify-content:space-between; gap:10px;">
                                        <span style="font-weight:600;">{e.name}</span>
                                        <span class="mono" style="flex:none; color:var(--muted);">
                                            {format!("{:.0} г", e.grams)}
                                        </span>
                                    </div>
                                    <div class="row__meta mono">
                                        {format!("{:.0} ккал · Б {:.0} · Ж {:.0} · У {:.0}",
                                            e.kcal, e.protein, e.fat, e.carbs)}
                                    </div>
                                </div>
                            }).collect_view()}
                        </div>
                        // Totals.
                        <div style="margin-top:12px; padding:12px; background:var(--surface-2); border-radius:10px;">
                            <div style="font-weight:700;" class="mono">
                                {format!("Итого: {:.0} ккал", t.kcal)}
                            </div>
                            <div class="row__meta mono">
                                {format!("Б {:.0} · Ж {:.0} · У {:.0}", t.protein, t.fat, t.carbs)}
                            </div>
                        </div>
                        // Good / improve.
                        {(!good.is_empty()).then(|| view! {
                            <div style="margin-top:12px;">
                                <div style="font-weight:650; color:var(--accent); margin-bottom:6px;">"Хорошо"</div>
                                {good.into_iter().map(|g| view! {
                                    <div style="padding:4px 0;">{g}</div>
                                }).collect_view()}
                            </div>
                        })}
                        {(!improve.is_empty()).then(|| view! {
                            <div style="margin-top:12px;">
                                <div style="font-weight:650; color:var(--warn-ink); margin-bottom:6px;">"Улучшить"</div>
                                {improve.into_iter().map(|g| view! {
                                    <div style="padding:4px 0;">{g}</div>
                                }).collect_view()}
                            </div>
                        })}
                    </div>
                }.into_view()
            }}
        </div>
    }
    .into_view()
}

/// Render one dataset's modal body.
pub fn render_dataset(ds: &Dataset) -> View {
    match ds {
        Dataset::Body(b) => body_view(b).into_view(),
        Dataset::Weight(w) => weight_view(w).into_view(),
        Dataset::Steps(s) => steps_view(s).into_view(),
        Dataset::Food(f) => food_view(f).into_view(),
        Dataset::System(m) => system_view(m).into_view(),
    }
}

/// Environment diagnostics: a plain key → value table, values stringified.
/// Generic on purpose — the client may add signals without an admin release.
fn system_view(m: &serde_json::Map<String, serde_json::Value>) -> impl IntoView {
    let rows: Vec<_> = m
        .iter()
        .map(|(k, v)| {
            let val = match v {
                serde_json::Value::String(s) => s.clone(),
                serde_json::Value::Null => "—".to_string(),
                other => other.to_string(),
            };
            view! {
                <div style="display: flex; gap: 10px; padding: 6px 0; border-bottom: 1px solid var(--line); align-items: baseline;">
                    <span class="mono" style="flex: 0 0 45%; font-size: 12px; color: #6B7491; word-break: break-all;">{k.clone()}</span>
                    <span class="mono" style="font-size: 12px; word-break: break-all;">{val}</span>
                </div>
            }
        })
        .collect();
    view! { <div>{rows}</div> }
}
