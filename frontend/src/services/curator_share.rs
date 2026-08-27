//! Curator data-share: gather a requested dataset from the REAL local stores into
//! the typed data_share envelope (see the support-chat data-request protocol) and
//! serialize it to the JSON string carried in the message `payload`.
//!
//! NO sample data — every field is read from IndexedDB via the existing services.
//! Missing values are honest `null`s, never fabricated.

use serde_json::{json, Value};

use crate::services::indicators::{self, IndicatorState};
use crate::services::weight_trend::{self, BalanceState, Direction, WeightTrend, DEFAULT_WINDOW_DAYS};
use crate::services::{i18n, local, profile};

/// The datasets a curator can request. Mirrors the protocol's `dataset` field.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Dataset {
    Body,
    Food,
    Weight,
    Steps,
    /// Environment/system diagnostics: browser, launch mode, passkey signals.
    System,
    All,
}

impl Dataset {
    /// Parse the `dataset` value from a data_request envelope.
    pub fn from_str(s: &str) -> Option<Dataset> {
        Some(match s {
            "body" => Dataset::Body,
            "food" => Dataset::Food,
            "weight" => Dataset::Weight,
            "steps" => Dataset::Steps,
            "system" => Dataset::System,
            "all" => Dataset::All,
            _ => return None,
        })
    }

    /// The i18n key of the request-panel RU text (also the message fallback text).
    pub fn panel_key(self) -> &'static str {
        match self {
            Dataset::Body => "curator.request_body",
            Dataset::Food => "curator.request_food",
            Dataset::Weight => "curator.request_weight",
            Dataset::Steps => "curator.request_steps",
            Dataset::System => "curator.request_system",
            Dataset::All => "curator.request_all",
        }
    }

    /// The i18n key of the short confirmation label used in the data_share `text`.
    pub fn shared_key(self) -> &'static str {
        match self {
            Dataset::Body => "curator.shared_body",
            Dataset::Food => "curator.shared_food",
            Dataset::Weight => "curator.shared_weight",
            Dataset::Steps => "curator.shared_steps",
            Dataset::System => "curator.shared_system",
            Dataset::All => "curator.shared_all",
        }
    }
}

// ── Per-dataset builders (real stores only) ──

async fn build_body() -> Value {
    let latest_kg = local::list_weight_entries().await.last().map(|e| e.weight_kg);
    let sex = profile::get_sex().map(|s| match s {
        profile::Sex::Male => "male",
        profile::Sex::Female => "female",
    });
    json!({
        "weight_kg": latest_kg,
        "height_cm": profile::get_height_cm(),
        "birth_year": profile::get_birth_year(),
        // Возраст считается ЗДЕСЬ: у куратора свой часовой пояс и своя дата, а
        // нормы железа ступеньками по возрасту — на границе (18/19, 50/51)
        // расхождение в год даёт другую норму.
        "age_years": profile::get_age_years(),
        "sex": sex,
    })
}

async fn build_weight() -> Value {
    let entries = local::list_weight_entries().await;
    let series: Vec<Value> = entries
        .iter()
        .map(|e| json!({ "date": e.date, "kg": e.weight_kg }))
        .collect();

    let trend = weight_trend::weight_trend(&entries, DEFAULT_WINDOW_DAYS);
    let balance = match trend.balance() {
        BalanceState::Deficit => "deficit",
        BalanceState::Surplus => "surplus",
        BalanceState::Maintenance => "maintenance",
    };
    // Slope / confidence / direction / days come from the trend estimate; each is
    // null when the window can't support it (honest, not fabricated).
    let (slope, confidence, direction, days) = match trend {
        WeightTrend::Insufficient { days } => (None, None, None, days),
        WeightTrend::Tentative { direction, slope_kg_per_week, days } => {
            (Some(slope_kg_per_week), None, Some(dir_str(direction)), days)
        }
        WeightTrend::Estimated { direction, slope_kg_per_week, confidence, days } => (
            Some(slope_kg_per_week),
            Some(confidence),
            Some(dir_str(direction)),
            days,
        ),
    };
    json!({
        "series": series,
        "balance": balance,
        "slope_kg_per_week": slope,
        "confidence": confidence,
        "direction": direction,
        "days": days,
    })
}

fn dir_str(d: Direction) -> &'static str {
    match d {
        Direction::Down => "down",
        Direction::Up => "up",
    }
}

async fn build_steps() -> Value {
    let series: Vec<Value> = local::list_step_entries()
        .await
        .iter()
        .map(|e| json!({ "date": e.date, "steps": e.steps }))
        .collect();
    json!({ "series": series })
}

async fn build_food() -> Value {
    let foods: std::collections::BTreeMap<String, api_types::Food> =
        local::list_foods().await.into_iter().map(|f| (f.id.clone(), f)).collect();

    // Last 7 calendar days, newest first.
    let today = crate::services::local::today_date();
    let mut days: Vec<Value> = Vec::new();
    for i in 0..7 {
        let date = (today - chrono::Duration::days(i)).format("%Y-%m-%d").to_string();
        let diary = local::list_diary(&date).await;
        if diary.is_empty() {
            continue;
        }

        let mut entries: Vec<Value> = Vec::new();
        let (mut tk, mut tp, mut tf, mut tc) = (0.0, 0.0, 0.0, 0.0);
        for e in &diary {
            let Some(food) = foods.get(&e.food_id) else { continue };
            let eaten = (e.grams - e.waste_grams).max(0.0);
            let factor = eaten / 100.0;
            let kcal = food.effective_kcal() * factor;
            let protein = food.protein * factor;
            let fat = food.fat * factor;
            let carbs = food.carbs * factor;
            tk += kcal;
            tp += protein;
            tf += fat;
            tc += carbs;
            entries.push(json!({
                "name": food.name,
                "grams": eaten,
                "kcal": kcal,
                "protein": protein,
                "fat": fat,
                "carbs": carbs,
            }));
        }

        days.push(json!({
            "date": date,
            "entries": entries,
            "totals": { "kcal": tk, "protein": tp, "fat": tf, "carbs": tc },
        }));
    }

    // Full indicator board (incl. the calorie-planka adherence), so the curator sees
    // how healthy the diet is at a glance — not just the raw entries.
    let states = indicators::share_states().await;
    let indicators_json: Vec<Value> = states
        .iter()
        .map(|(k, s)| json!({ "key": k, "label": indicator_label(k), "state": state_str(*s) }))
        .collect();

    // Per-day calorie-planka adherence, last 7 days (newest first): intake vs the
    // planka that applied THAT day, and whether it was kept. Only logged days.
    let mut planka_days: Vec<Value> = Vec::new();
    for i in 0..7 {
        let date = (today - chrono::Duration::days(i)).format("%Y-%m-%d").to_string();
        let intake = local::kcal_on(&date).await;
        if intake <= 0.0 {
            continue;
        }
        let planka = indicators::calorie_planka_on(&date).await;
        planka_days.push(json!({
            "date": date,
            "intake": intake,
            "planka": planka,
            "within": planka.map(|p| intake <= p),
            // Today is still in progress — the admin marks it specially (not a
            // pass/fail day yet). `today_date()` is the user's local day.
            "today": i == 0,
        }));
    }

    json!({ "days": days, "indicators": indicators_json, "planka_days": planka_days })
}

/// Short indicator key → RU label for the share payload.
fn indicator_label(key: &str) -> &'static str {
    match key {
        "calories" => "Калории",
        "protein" => "Белок",
        "veg_fruit" => "Овощи и фрукты",
        "calcium" => "Кальций",
        "epa_dha" => "Омега-3 (EPA+DHA)",
        "fat_ratio" => "Баланс жира",
        "fiber" => "Клетчатка",
        "steps" => "Шаги",
        _ => "—",
    }
}

/// Indicator colour state → wire string.
fn state_str(s: IndicatorState) -> &'static str {
    match s {
        IndicatorState::Green => "green",
        IndicatorState::Orange => "orange",
        IndicatorState::Red => "red",
        IndicatorState::Unknown => "unknown",
    }
}

/// Environment/system diagnostics — REAL browser signals only, no fabrication:
/// UA, launch (display-mode) flags, PWA verdict, WebAuthn/passkey signals and the
/// local auth/PWA flags. Everything the «why no passkey / wrong install screen»
/// debugging needs, without any secret values (presence booleans only).
async fn build_system() -> Value {
    use wasm_bindgen::JsCast;
    let win = web_sys::window().expect("no window");
    let nav = win.navigator();
    let mm = |q: &str| {
        win.match_media(q)
            .ok()
            .flatten()
            .map(|m| m.matches())
    };
    let nav_standalone = js_sys::Reflect::get(&nav, &wasm_bindgen::JsValue::from_str("standalone"))
        .ok()
        .and_then(|v| v.as_bool());

    // WebAuthn signals, raw: is the API there at all, and does the platform
    // authenticator answer available? (These two are exactly what the onboarding
    // passkey gate reads.)
    let pkc_val = js_sys::Reflect::get(&win, &wasm_bindgen::JsValue::from_str("PublicKeyCredential")).ok();
    let pkc_present = pkc_val
        .as_ref()
        .map(|v| !v.is_undefined() && !v.is_null())
        .unwrap_or(false);
    let mut iuvpaa: Option<bool> = None;
    if let Some(pkc) = pkc_val.filter(|v| !v.is_undefined() && !v.is_null()) {
        if let Ok(f) = js_sys::Reflect::get(
            &pkc,
            &wasm_bindgen::JsValue::from_str("isUserVerifyingPlatformAuthenticatorAvailable"),
        ) {
            if let Ok(func) = f.dyn_into::<js_sys::Function>() {
                if let Ok(p) = func.call0(&pkc) {
                    if let Ok(promise) = p.dyn_into::<js_sys::Promise>() {
                        iuvpaa = wasm_bindgen_futures::JsFuture::from(promise)
                            .await
                            .ok()
                            .and_then(|v| v.as_bool());
                    }
                }
            }
        }
    }

    let ls = win.local_storage().ok().flatten();
    let get = |k: &str| ls.as_ref().and_then(|s| s.get_item(k).ok().flatten());
    // navigator.serviceWorker.controller via Reflect (the typed web-sys accessor
    // needs a cargo feature we don't enable elsewhere).
    let sw_controller = js_sys::Reflect::get(&nav, &wasm_bindgen::JsValue::from_str("serviceWorker"))
        .ok()
        .filter(|v| !v.is_undefined() && !v.is_null())
        .and_then(|sw| js_sys::Reflect::get(&sw, &wasm_bindgen::JsValue::from_str("controller")).ok())
        .map(|c| !c.is_undefined() && !c.is_null())
        .unwrap_or(false);

    json!({
        "user_agent": nav.user_agent().unwrap_or_default(),
        "language": nav.language(),
        "platform": crate::pages::pwa_prompt::detect_platform(),
        "display_standalone": mm("(display-mode: standalone)"),
        "display_wco": mm("(display-mode: window-controls-overlay)"),
        "display_browser": mm("(display-mode: browser)"),
        "navigator_standalone": nav_standalone,
        "is_pwa": crate::services::platform::is_pwa(),
        "public_key_credential": pkc_present,
        "platform_authenticator": iuvpaa,
        "passkey_unavailable": crate::services::auth::passkey_unavailable().await,
        "auth_ctx": get("auth_ctx"),
        "pwa_dismissed": get("pwa_dismissed").is_some(),
        "has_user_id": get("user_id").is_some(),
        "has_token": get("auth_token").is_some(),
        "sw_controller": sw_controller,
        "online": nav.on_line(),
        "notification_permission": format!("{:?}", web_sys::Notification::permission()),
    })
}

// ── Отчёт куратору ───────────────────────────────────────────────────────────
//
// Куратор просит ОДНИМ действием и называет срок. Отчёт собирается здесь, на
// устройстве, из настоящих сторов — как и всё в этом файле, без выдуманных
// значений: отсутствующее уезжает честным null.
//
// В отчёт входит то, о чём договорились: факты и значения взвешиваний и шагов,
// состояние индикаторов, история планок. Дневник еды пока не входит.
//
// Суточный отчёт всё равно несёт НЕДЕЛЬНЫЕ агрегаты: железо, гем, жиры, мясо,
// яйца и клетчатка судятся неделями, и без них половина ряда индикаторов у
// куратора была бы серой.

/// Ряды по индикаторам: дневные — за запрошенный срок, недельные — своими
/// восемью неделями.
async fn build_indicators(days: u32) -> Value {
    let series = indicators::indicator_series_for(days).await;
    let out: Vec<Value> = series
        .iter()
        .map(|s| {
            let points: Vec<Value> = s
                .days
                .iter()
                .zip(s.met_days.iter())
                .map(|((date, value, ratio), met)| {
                    json!({ "date": date, "value": value, "ratio": ratio, "met": met })
                })
                .collect();
            json!({
                "key": s.key,
                "label": indicator_label(s.key),
                "state": state_str(s.state),
                "missed": s.missed,
                "labels": s.labels,
                "points": points,
            })
        })
        .collect();
    Value::Array(out)
}

/// Действующие планки — то, по чему индикаторы судятся ПРЯМО СЕЙЧАС, и то, что
/// куратор будет править.
///
/// Один проход по всем двенадцати видам: планка теперь живёт в одном месте, и
/// перечислять её по-разному для каждого индикатора больше незачем. Пометки «это
/// поставил куратор» тоже нет — различать авторство перестало иметь смысл.
fn build_targets() -> Value {
    use crate::services::plankas;
    let mut out = serde_json::Map::new();
    for kind in plankas::ALL {
        out.insert(
            kind.key().to_string(),
            match plankas::current(*kind) {
                Some(v) => json!(v),
                None => Value::Null,
            },
        );
    }
    Value::Object(out)
}

/// История планок за срок: какая планка действовала в какой день.
///
/// По ВСЕМ двенадцати видам, а не по трём подвижным. Планка любого из них теперь
/// может смениться — её сменил куратор, — и день обязан судиться по той, что
/// действовала тогда. У вида, которого никто не трогал, история просто пуста.
async fn build_planka_history(from: &str) -> Value {
    use crate::services::plankas;
    let mut out = serde_json::Map::new();
    for k in plankas::ALL {
        let kind = k.key();
        let rows: Vec<Value> = local::planka_history(kind)
            .await
            .into_iter()
            .filter(|e| e.date.as_str() >= from)
            .map(|e| json!({ "date": e.date, "amount": e.amount }))
            .collect();
        if rows.is_empty() {
            continue;
        }
        out.insert(kind.to_string(), Value::Array(rows));
    }
    Value::Object(out)
}

/// Собрать отчёт за `days` завершённых дней плюс сегодняшний.
/// Последний день, который вообще может попасть в отчёт, — ВЧЕРА.
///
/// Сегодняшний не едет никогда, и это не осторожность, а правило: день ещё
/// заполняется. Куратор, увидевший «съедено 400 ккал» в обед, прочтёт это как
/// недобор и начнёт лечить то, чего нет. Незаконченный день нельзя ни судить,
/// ни считать от него планку.
pub fn report_through() -> String {
    (local::today_date() - chrono::Duration::days(1)).format("%Y-%m-%d").to_string()
}

/// Самый ранний день, о котором у нас вообще есть данные, — начало «всей
/// истории». `None`, когда данных нет ни одного дня.
async fn earliest_day() -> Option<String> {
    let w = local::list_weight_entries().await.first().map(|e| e.date.clone());
    let s = local::list_step_entries().await.iter().map(|e| e.date.clone()).min();
    match (w, s) {
        (Some(a), Some(b)) => Some(a.min(b)),
        (a, b) => a.or(b),
    }
}

/// Собрать отчёт за отрезок `[from, to]` включительно.
///
/// `from = None` — вся история. Не «366 дней назад», как было при счёте днями:
/// человек, попросивший «все данные», имеет в виду все, а молчаливое обрезание
/// сроком выглядело бы как потеря.
pub async fn build_report(from: Option<String>, to: String) -> Value {
    let from = match from {
        Some(f) => f,
        None => earliest_day().await.unwrap_or_else(|| to.clone()),
    };
    // Сколько дней просить у индикаторов: они считают от сегодня назад.
    let days = {
        let f = chrono::NaiveDate::parse_from_str(&from, "%Y-%m-%d").unwrap_or(local::today_date());
        (local::today_date() - f).num_days().clamp(1, 3660) as u32
    };

    let weight = local::list_weight_entries().await;
    let weight_series: Vec<Value> = weight
        .iter()
        .filter(|e| e.date >= from)
        .map(|e| {
            json!({
                "date": e.date,
                "kg": e.weight_kg,
                // Условия замера — та же оговорка, что человек ставил себе: без
                // неё куратор сравнивал бы несравнимое.
                "morning": e.morning,
                "no_water": e.no_water,
                "no_food": e.no_food,
                "no_wash": e.no_wash,
                "used_toilet": e.used_toilet,
            })
        })
        .collect();
    let trend = weight_trend::weight_trend(&weight, DEFAULT_WINDOW_DAYS);
    let (slope, confidence, direction, trend_days) = match trend {
        WeightTrend::Insufficient { days } => (None, None, None, days),
        WeightTrend::Tentative { direction, slope_kg_per_week, days } => {
            (Some(slope_kg_per_week), None, Some(dir_str(direction)), days)
        }
        WeightTrend::Estimated { direction, slope_kg_per_week, confidence, days } => (
            Some(slope_kg_per_week),
            Some(confidence),
            Some(dir_str(direction)),
            days,
        ),
    };

    let steps_series: Vec<Value> = local::list_step_entries()
        .await
        .iter()
        .filter(|e| e.date >= from)
        .map(|e| json!({ "date": e.date, "steps": e.steps }))
        .collect();

    json!({ "report": {
        // `to` — последний день ДАННЫХ, а не день отправки. От него отсчитывается
        // следующий отчёт «только новое», и читается он прямо отсюда: отдельного
        // хранилища границы нет и не нужно.
        "period": { "from": from, "to": to, "days": days },
        "generated_at": chrono::Local::now().to_rfc3339(),
        "body": build_body().await,
        "weight": {
            "series": weight_series,
            "balance": match trend.balance() {
                BalanceState::Deficit => "deficit",
                BalanceState::Surplus => "surplus",
                BalanceState::Maintenance => "maintenance",
            },
            "slope_kg_per_week": slope,
            "confidence": confidence,
            "direction": direction,
            "trend_days": trend_days,
        },
        "steps": { "series": steps_series },
        // Среднее съеденное за 7 ЗАВЕРШЁННЫХ дней. Едет ради расчёта на стороне
        // куратора: это вход `adherence` — без него нельзя отличить «вес стоит,
        // потому что планка велика» от «вес стоит, потому что её не держат», а
        // именно на этом различии недельный цикл и решает, двигать ли планку.
        "avg_kcal_7d": local::avg_daily_kcal(7).await,
        "indicators": build_indicators(days).await,
        "targets": build_targets(),
        "plankas": build_planka_history(&from).await,
    }})
}

/// Готовое сообщение с отчётом: JSON-строка и короткая подпись.
pub async fn report_message(from: Option<String>, to: String) -> Result<(String, String), String> {
    let value = build_report(from, to).await;
    let payload = serde_json::to_string(&value).map_err(|e| format!("serialize error: {e}"))?;
    Ok((i18n::t("curator.report_sent").to_string(), payload))
}

/// Gather `dataset` into its typed data_share envelope value.
///
/// The envelope is ALWAYS an object keyed by dataset name — a single dataset is
/// `{"weight": {...}}`, "all" is the 4-key map. Keying single shares too keeps the
/// reader (admin `datasets_from_payload`) uniform and unambiguous: a bare
/// `{"series": …}` couldn't be told apart (weight vs steps both carry `series`).
pub async fn build(dataset: Dataset) -> Value {
    match dataset {
        Dataset::Body => json!({ "body": build_body().await }),
        Dataset::Weight => json!({ "weight": build_weight().await }),
        Dataset::Steps => json!({ "steps": build_steps().await }),
        Dataset::Food => json!({ "food": build_food().await }),
        Dataset::System => json!({ "system": build_system().await }),
        Dataset::All => json!({
            "body": build_body().await,
            "weight": build_weight().await,
            "steps": build_steps().await,
            "food": build_food().await,
            "system": build_system().await,
        }),
    }
}

/// Build the data_share message the user sends on "Поделиться": the payload JSON
/// STRING plus the short RU confirmation `text`. FAIL LOUDLY on a serialize error.
pub async fn share_message(dataset: Dataset) -> Result<(String, String), String> {
    let value = build(dataset).await;
    let payload = serde_json::to_string(&value).map_err(|e| format!("serialize error: {e}"))?;
    let text = i18n::t(dataset.shared_key()).to_string();
    Ok((text, payload))
}
