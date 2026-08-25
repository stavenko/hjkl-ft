//! Live support thread, backed by the support-worker (a SEPARATE server from the
//! AI `chat` store). The server is the source of truth; local IndexedDB holds a
//! message cache (keyed by server `seq`), the poll cursor, and an optimistic
//! outbox of in-flight / failed sends.
//!
//! FAIL LOUDLY: every transport path returns `Result<_, String>`; fire-and-forget
//! callers log on `Err` (never swallow). No sample data — the cache is only ever
//! populated from real server responses.

use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use wasm_bindgen::{JsCast, JsValue};
use wasm_bindgen_futures::JsFuture;

use super::{auth, config, db};

/// The two threads the `/chat` toggle switches between. AI = the existing local
/// AI chat; Live = this server-backed support thread.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ChatMode {
    Ai,
    Live,
}

/// Собеседник по умолчанию, пока приложение ещё не узнало, есть ли куратор.
pub const PEER_ADMIN: &str = "admin";

/// Кэш серверного сообщения, ключ — `"{peer}:{seq}"`.
///
/// Тред на сервере один на ПАРУ, поэтому `seq` уникален только внутри своего
/// собеседника: у переписки с админом и у переписки с куратором номера идут
/// каждый со своей единицы. Ключом стал составной id, иначе первое сообщение
/// куратора затёрло бы первое сообщение поддержки.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LiveMessage {
    /// `"{peer}:{seq}"` — ключ в IndexedDB.
    #[serde(default)]
    pub id: String,
    /// Собеседник: `admin` либо `curator:<id>`.
    #[serde(default = "default_peer")]
    pub peer: String,
    pub seq: u64,
    /// Idempotency key the sender generated; present on server rows. Used to
    /// reconcile an optimistic outbox item once it returns as a server message.
    #[serde(default)]
    pub client_id: String,
    pub sender: String, // "user" | "expert" — MATCHES the worker's field name
    pub text: String,
    pub created_at: String,
    /// Message kind: "text" (plain), "data_request" (curator asks for a dataset),
    /// or "data_share" (user's shared dataset). Old rows (no field) → "text".
    #[serde(default = "default_kind")]
    pub kind: String,
    /// Typed envelope, a RAW JSON STRING (or null for plain text). Parsed by the
    /// bubble renderer per `kind`. Old rows (no field) → None.
    #[serde(default)]
    pub payload: Option<String>,
    /// Имя куратора под его пузырём. У поддержки пусто.
    #[serde(default)]
    pub sender_name: Option<String>,
}

fn default_kind() -> String {
    "text".to_string()
}

fn default_peer() -> String {
    PEER_ADMIN.to_string()
}

/// Ключ сообщения в кэше.
fn msg_id(peer: &str, seq: u64) -> String {
    format!("{peer}:{seq}")
}

/// Optimistic outbox entry, keyed by `client_id` (idempotency key + IndexedDB
/// key). Acked items are deleted from the outbox once they become server messages.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OutboxItem {
    pub client_id: String,
    pub text: String,
    pub status: String, // "sending" | "failed"
    pub created_at: String,
    /// Message kind for this in-flight send (so a retried data_share keeps its
    /// envelope). Old rows (no field) → "text".
    #[serde(default = "default_kind")]
    pub kind: String,
    /// Typed envelope (RAW JSON STRING) for a data_share send; None for text.
    #[serde(default)]
    pub payload: Option<String>,
}

/// Курсор опроса — СВОЙ у каждого собеседника: номера в разных тредах свои, и
/// один общий курсор пропустил бы половину сообщений.
#[derive(Debug, Clone, Serialize, Deserialize)]
struct Cursor {
    key: String,
    after_seq: u64,
}

fn cursor_key(peer: &str) -> String {
    format!("{CURSOR_KEY}:{peer}")
}

#[derive(Serialize)]
struct SendReq<'a> {
    client_id: &'a str,
    text: &'a str,
    /// Omitted for a plain-text send (server defaults to "text"); set for a
    /// data_share message.
    #[serde(skip_serializing_if = "Option::is_none")]
    kind: Option<&'a str>,
    /// The typed envelope as a RAW JSON STRING; None for plain text.
    #[serde(skip_serializing_if = "Option::is_none")]
    payload: Option<&'a str>,
}

#[derive(Deserialize)]
struct SendAck {
    seq: u64,
    created_at: String,
    /// Куда сервер это положил. Развилку «куратор или админ» решает он, и
    /// угадывать её на клиенте значит однажды разойтись.
    #[serde(default = "default_peer")]
    peer: String,
}

#[derive(Deserialize)]
struct PollResp {
    messages: Vec<LiveMessage>,
    next_after_seq: u64,
    has_more: bool,
    /// Тред, который сервер только что отдал.
    #[serde(default = "default_peer")]
    peer: String,
}

#[derive(Serialize)]
struct ReadReq {
    seq: u64,
}

fn now() -> String {
    chrono::Utc::now().to_rfc3339()
}

const MESSAGES_STORE: &str = "support_msgs";
const OUTBOX_STORE: &str = "support_outbox";
const META_STORE: &str = "support_meta";
const CURSOR_KEY: &str = "cursor";

// ── Transport (FAIL LOUDLY, JWT-authed; mirrors sync.rs / bug_report.rs) ──

/// POST `body` (JSON) to `{support_base_url}{path}` and parse the JSON response.
pub(crate) async fn post_json<O: DeserializeOwned>(path: &str, body: &str) -> Result<O, String> {
    let base = &config::get().support_base_url;
    if base.is_empty() {
        return Err("support_base_url is not configured".to_string());
    }
    let url = format!("{base}{path}");
    let token = auth::get_token().ok_or_else(|| "not authenticated".to_string())?;

    let opts = web_sys::RequestInit::new();
    opts.set_method("POST");
    opts.set_body(&JsValue::from_str(body));

    let headers = web_sys::Headers::new().map_err(|e| format!("{e:?}"))?;
    headers.set("Content-Type", "application/json").map_err(|e| format!("{e:?}"))?;
    headers.set("Authorization", &format!("Bearer {token}")).map_err(|e| format!("{e:?}"))?;
    opts.set_headers(&headers);

    let request =
        web_sys::Request::new_with_str_and_init(&url, &opts).map_err(|e| format!("{e:?}"))?;
    let window = web_sys::window().expect("no window");
    let resp_val = JsFuture::from(window.fetch_with_request(&request))
        .await
        .map_err(|e| format!("{e:?}"))?;
    let resp: web_sys::Response = resp_val.dyn_into().map_err(|_| "not a Response".to_string())?;

    let text = JsFuture::from(resp.text().map_err(|e| format!("{e:?}"))?)
        .await
        .map_err(|e| format!("{e:?}"))?;
    let text = text.as_string().ok_or("response not a string")?;

    if !resp.ok() {
        return Err(format!("HTTP {}: {}", resp.status(), text));
    }
    serde_json::from_str(&text).map_err(|e| format!("parse error: {e}"))
}

/// GET `{support_base_url}{path}` (query in the URL) and parse the JSON response.
pub(crate) async fn get_json<O: DeserializeOwned>(path: &str) -> Result<O, String> {
    let base = &config::get().support_base_url;
    if base.is_empty() {
        return Err("support_base_url is not configured".to_string());
    }
    let url = format!("{base}{path}");
    let token = auth::get_token().ok_or_else(|| "not authenticated".to_string())?;

    let opts = web_sys::RequestInit::new();
    opts.set_method("GET");

    let headers = web_sys::Headers::new().map_err(|e| format!("{e:?}"))?;
    headers.set("Authorization", &format!("Bearer {token}")).map_err(|e| format!("{e:?}"))?;
    opts.set_headers(&headers);

    let request =
        web_sys::Request::new_with_str_and_init(&url, &opts).map_err(|e| format!("{e:?}"))?;
    let window = web_sys::window().expect("no window");
    let resp_val = JsFuture::from(window.fetch_with_request(&request))
        .await
        .map_err(|e| format!("{e:?}"))?;
    let resp: web_sys::Response = resp_val.dyn_into().map_err(|_| "not a Response".to_string())?;

    let text = JsFuture::from(resp.text().map_err(|e| format!("{e:?}"))?)
        .await
        .map_err(|e| format!("{e:?}"))?;
    let text = text.as_string().ok_or("response not a string")?;

    if !resp.ok() {
        return Err(format!("HTTP {}: {}", resp.status(), text));
    }
    serde_json::from_str(&text).map_err(|e| format!("parse error: {e}"))
}

// ── Public API ──

/// Send a new Live message. Writes an optimistic outbox item immediately, POSTs
/// (idempotent by `client_id`), and on ack reconciles into the message cache.
pub async fn send(text: String) -> Result<LiveMessage, String> {
    send_typed(text, "text".to_string(), None).await
}

/// Send a typed message — a data_share (kind="data_share", `payload` = the
/// envelope JSON string) or plain text. Same optimistic outbox + reconcile path
/// as [`send`]; the confirmation `text` is what shows optimistically.
pub async fn send_data_share(text: String, payload: String) -> Result<LiveMessage, String> {
    send_typed(text, "data_share".to_string(), Some(payload)).await
}

async fn send_typed(
    text: String,
    kind: String,
    payload: Option<String>,
) -> Result<LiveMessage, String> {
    let client_id = uuid::Uuid::now_v7().to_string();
    let item = OutboxItem {
        client_id: client_id.clone(),
        text: text.clone(),
        status: "sending".to_string(),
        created_at: now(),
        kind: kind.clone(),
        payload: payload.clone(),
    };
    db::put(OUTBOX_STORE, &item).await;
    post_with_outbox(client_id, text, item.created_at, kind, payload).await
}

/// Retry a failed outbox item: flip it back to "sending" and re-POST with the SAME
/// `client_id` (idempotent — not a new send path).
pub async fn retry(client_id: String) -> Result<LiveMessage, String> {
    let existing: Option<OutboxItem> = db::get(OUTBOX_STORE, &client_id).await;
    let Some(mut item) = existing else {
        return Err(format!("outbox item not found: {client_id}"));
    };
    item.status = "sending".to_string();
    db::put(OUTBOX_STORE, &item).await;
    post_with_outbox(client_id, item.text, item.created_at, item.kind, item.payload).await
}

/// Shared POST + reconcile path for `send` and `retry`. On success the acked
/// message lands in the cache and the outbox row is removed; on failure the outbox
/// row is marked "failed" (retryable) and the error is returned.
async fn post_with_outbox(
    client_id: String,
    text: String,
    created_at: String,
    kind: String,
    payload: Option<String>,
) -> Result<LiveMessage, String> {
    // Only send kind/payload when this is a typed (non-text) message.
    let (kind_field, payload_field) = if kind == "text" {
        (None, None)
    } else {
        (Some(kind.as_str()), payload.as_deref())
    };
    let body = serde_json::to_string(&SendReq {
        client_id: &client_id,
        text: &text,
        kind: kind_field,
        payload: payload_field,
    })
    .map_err(|e| e.to_string())?;

    match post_json::<SendAck>("/message", &body).await {
        Ok(ack) => {
            let msg = LiveMessage {
                id: msg_id(&ack.peer, ack.seq),
                peer: ack.peer.clone(),
                seq: ack.seq,
                client_id: client_id.clone(),
                sender: "user".to_string(),
                text,
                created_at: ack.created_at,
                kind,
                payload,
                sender_name: None,
            };
            db::put(MESSAGES_STORE, &msg).await;
            db::delete(OUTBOX_STORE, &client_id).await;
            // Advance the cursor past this seq so the next poll doesn't re-deliver it.
            let cursor = load_cursor(&ack.peer).await;
            if ack.seq >= cursor {
                store_cursor(&ack.peer, ack.seq).await;
            }
            set_current_peer(&ack.peer);
            Ok(msg)
        }
        Err(e) => {
            let failed = OutboxItem {
                client_id,
                text,
                status: "failed".to_string(),
                created_at,
                kind,
                payload,
            };
            db::put(OUTBOX_STORE, &failed).await;
            Err(e)
        }
    }
}

/// Poll the server from the stored cursor, paging until `has_more` is false. Each
/// message is upserted by `seq` (idempotent), and the cursor only advances forward.
/// One-shot poll (immediate): drain all pending pages and return at once.
pub async fn poll() -> Result<(), String> {
    poll_inner(0).await
}

/// Long-poll: the worker holds the FIRST request open for up to `wait_secs`
/// (returns the moment a newer message lands, else empty after the window); any
/// further pages are drained immediately. Lets the Live loop wait on the
/// connection instead of a fixed 4s tick — fewer requests, near-instant delivery.
pub async fn poll_wait(wait_secs: u32) -> Result<(), String> {
    poll_inner(wait_secs).await
}

async fn poll_inner(first_wait: u32) -> Result<(), String> {
    // Only the FIRST fetch holds open; subsequent drains use wait=0 so a backlog
    // empties without a 25s stall between pages.
    let mut wait = first_wait;
    // Опрашивается ТЕКУЩИЙ тред: архивные не меняются, и их история приезжает
    // синком. Кого считать текущим, решает сервер — он же и отвечает `peer`.
    let mut peer = current_peer();
    loop {
        let after = load_cursor(&peer).await;
        let r: PollResp =
            get_json(&format!("/messages?after_seq={after}&limit=100&wait={wait}")).await?;
        // Смена куратора между опросами меняет адресата: дальше идём по нему.
        peer = r.peer.clone();
        set_current_peer(&peer);
        for m in &r.messages {
            let m = LiveMessage {
                id: msg_id(&peer, m.seq),
                peer: peer.clone(),
                ..m.clone()
            };
            db::put(MESSAGES_STORE, &m).await;
            // Reconcile a lost-ack optimistic send: if this server message carries
            // a client_id we still have in the outbox, drop the outbox row (it's now
            // a real message) — prevents a permanent duplicate + stuck "sending".
            if !m.client_id.is_empty() {
                db::delete(OUTBOX_STORE, &m.client_id).await;
            }
        }
        // A curator `set_planka` directive is applied by THE APP here — the server
        // never writes the user's data. Idempotent (by seq).
        apply_planka_directives(&r.messages).await;
        apply_curator_planka_directives(&r.messages).await;
        apply_week_directives(&r.messages).await;
        store_cursor(&peer, r.next_after_seq).await;
        if !r.has_more {
            break;
        }
        wait = 0;
    }
    Ok(())
}

/// App-flag: the highest server `seq` of a `set_planka` directive already applied,
/// so a directive applies EXACTLY ONCE across polls / relaunches.
const PLANKA_DIRECTIVE_SEQ_KEY: &str = "planka_directive_seq";

/// Apply any NEW curator `set_planka` directive among `msgs`. The directive carries
/// `{amount}`; THE APP (not the server) writes the new calorie planka into the
/// user's own goals and syncs it — nothing outside the frontend ever touches user
/// data. Only the newest unhandled directive is applied (the planka is one value),
/// and its seq is recorded so it never re-applies.
async fn apply_planka_directives(msgs: &[LiveMessage]) {
    let last = crate::services::app_flags::get(PLANKA_DIRECTIVE_SEQ_KEY)
        .and_then(|s| s.parse::<u64>().ok())
        .unwrap_or(0);
    let mut newest: Option<(u64, f64)> = None;
    for m in msgs {
        if m.kind != "set_planka" || m.seq <= last {
            continue;
        }
        let Some(payload) = m.payload.as_deref() else { continue };
        let Ok(v) = serde_json::from_str::<serde_json::Value>(payload) else { continue };
        let Some(amount) = v.get("amount").and_then(|a| a.as_f64()) else { continue };
        if !amount.is_finite() || amount <= 0.0 || amount >= 20000.0 {
            continue;
        }
        if newest.map_or(true, |(s, _)| m.seq > s) {
            newest = Some((m.seq, amount));
        }
    }
    if let Some((seq, amount)) = newest {
        crate::services::local::set_calorie_goal(amount).await;
        crate::services::sync::push_background();
        crate::services::app_flags::set(PLANKA_DIRECTIVE_SEQ_KEY, &seq.to_string());
        // Tell the user (inbox letter + mail red-dot) that the curator changed it —
        // so they learn about it even without opening the chat.
        crate::services::letters::add(crate::services::letters::Letter {
            id: format!("planka-curator-{seq}"),
            created_at: chrono::Local::now().to_rfc3339(),
            body: crate::services::directives::set_planka_letter("calories", amount),
            read: false,
            action: None,
            action_done: false,
        });
    }
}

// ── Директива правки планки ──────────────────────────────────────────────────
//
// Одна директива на любой индикатор: `{key, amount?, locked?}`. Прежняя
// `set_planka` умела только калории и присылала готовый русский текст; эта несёт
// ЧИСЛО, а текст человек собирает у себя.
//
// Применяются ВСЕ новые, а не только последняя: планки разные, и две директивы
// подряд означают две правки, а не одну. Порядок — по seq, чтобы две правки
// одного индикатора легли в том порядке, в каком их сделал куратор.

/// App-flag: наибольший `seq` уже применённой директивы правки планки.
const CURATOR_PLANKA_SEQ_KEY: &str = "curator_planka_directive_seq";

/// Разумные пределы значения по индикатору. Не «валидация ради валидации»: сюда
/// приходит число из чужого приложения, и опечатка куратора не должна становиться
/// планкой в 200 000 ккал, от которой потом считается ещё и белок.
fn planka_range(key: &str) -> Option<(f64, f64)> {
    Some(match key {
        "calories" => (500.0, 20_000.0),
        "protein" => (10.0, 500.0),
        "steps" => (1_000.0, 100_000.0),
        "veg_fruit" => (100.0, 5_000.0),
        "calcium" => (100.0, 5_000.0),
        "fiber" => (5.0, 200.0),
        "iron" => (0.1, 100.0),
        "heme" => (0.0, 21.0),
        "epa_dha" => (0.1, 50.0),
        "fat_ratio" => (0.1, 20.0),
        "red_meat" => (0.0, 10_000.0),
        "egg" => (0.0, 70.0),
        _ => return None,
    })
}

/// Записать действующее число в историю планок — для тех трёх, у кого история
/// есть. Без этого день, в который куратор поменял планку, судился бы по
/// прежнему числу, а дневник показывал бы не ту планку.
async fn record_effective(key: &str) {
    use crate::services::local;
    match key {
        "calories" => {
            if let Some(v) = local::calorie_goal_amount().await {
                local::record_planka(local::PLANKA_CALORIES, v).await;
            }
            // Белок выводится из калорий — его норма поехала вместе с ними.
            local::record_protein_planka().await;
        }
        "steps" => {
            if let Some(v) = crate::services::profile::get_steps_planka() {
                local::record_planka(local::PLANKA_STEPS, v).await;
            }
        }
        "protein" => local::record_protein_planka().await,
        _ => {}
    }
}

async fn apply_curator_planka_directives(msgs: &[LiveMessage]) {
    use crate::services::{curator_plankas, directives, letters};

    let last = crate::services::app_flags::get(CURATOR_PLANKA_SEQ_KEY)
        .and_then(|s| s.parse::<u64>().ok())
        .unwrap_or(0);
    let mut ordered: Vec<(u64, String, Option<f64>, bool)> = Vec::new();
    for m in msgs {
        if m.kind != "set_planka_v2" || m.seq <= last {
            continue;
        }
        let Some(payload) = m.payload.as_deref() else { continue };
        let Ok(v) = serde_json::from_str::<serde_json::Value>(payload) else { continue };
        let Some(key) = v.get("key").and_then(|k| k.as_str()) else { continue };
        let Some((lo, hi)) = planka_range(key) else {
            leptos::logging::warn!("директива планки: неизвестный индикатор {key}");
            continue;
        };
        let amount = match v.get("amount").and_then(|a| a.as_f64()) {
            Some(a) if a.is_finite() && a >= lo && a <= hi => Some(a),
            // Число вне пределов — это не «поправим молча», а испорченная
            // директива: применять её нельзя, а терять молча тем более.
            Some(a) => {
                leptos::logging::error!("директива планки {key}: значение {a} вне [{lo}, {hi}]");
                continue;
            }
            None => None,
        };
        let locked = v.get("locked").and_then(|l| l.as_bool()).unwrap_or(false);
        ordered.push((m.seq, key.to_string(), amount, locked));
    }
    if ordered.is_empty() {
        return;
    }
    ordered.sort_by_key(|(seq, _, _, _)| *seq);

    let mut applied = last;
    for (seq, key, amount, locked) in ordered {
        curator_plankas::set(&key, amount, locked).await;
        record_effective(&key).await;
        applied = applied.max(seq);

        // Письмо — чтобы человек узнал о правке, даже не открывая чат.
        let body = match amount {
            Some(a) => directives::set_planka_letter(&key, a),
            None => directives::lock_letter(&key, locked),
        };
        letters::add(letters::Letter {
            id: format!("curator-planka-{seq}"),
            created_at: chrono::Local::now().to_rfc3339(),
            body,
            read: false,
            action: None,
            action_done: false,
        });
    }
    crate::services::app_flags::set(CURATOR_PLANKA_SEQ_KEY, &applied.to_string());
    crate::services::sync::push_background();
}

// ── Директива «открыть тему» ─────────────────────────────────────────────────
//
// Темы открываются гейтами — по заслугам, и это правильный порядок. Но иногда
// открыть надо руками: гейт опирается на дату открытия предыдущей темы, а её
// может не оказаться (стёрлась старой миграцией, приехала с другого устройства), и
// тогда человек с честно закрытыми неделями стоит перед закрытой дверью. Плюс
// автор ведёт истории впереди собственного прогресса.
//
// Применяет директиву САМ КЛИЕНТ, как и `set_planka`: сервер не пишет данные
// пользователя. Открытие идёт теми же функциями, что и у гейта, — тема это не один
// флаг, а ещё планка шагов, цель кальция, якорь своей недели и постановка еды в
// очередь на разбор.

/// App-flag: наибольший `seq` уже применённой директивы открытия — чтобы одна и та
/// же не применялась дважды между опросами и перезапусками.
const WEEK_DIRECTIVE_SEQ_KEY: &str = "week_directive_seq";

/// Открыть тему по номеру. Номера — те же, что у историй в ленте.
///
/// Первые две ничего не открывают: приложение и так начинается с них, а планка по
/// калориям выдаётся расчётом, а не флагом.
async fn open_week(week: u32) {
    use crate::services::indicators;
    match week {
        3 => indicators::open_activity_week().await,
        4 => indicators::open_calcium_week().await,
        5 => indicators::open_iron_week().await,
        6 => indicators::open_fat_week().await,
        7 => indicators::open_red_meat_week().await,
        8 => indicators::open_egg_week().await,
        9 => indicators::open_fiber_week().await,
        _ => {}
    }
}

/// Применить новые директивы `open_week` из пришедшей пачки.
///
/// В отличие от планки, здесь применяются ВСЕ новые, а не только последняя: темы
/// накапливаются, и две директивы подряд означают две открытые темы, а не одну.
async fn apply_week_directives(msgs: &[LiveMessage]) {
    let last = crate::services::app_flags::get(WEEK_DIRECTIVE_SEQ_KEY)
        .and_then(|s| s.parse::<u64>().ok())
        .unwrap_or(0);
    let mut applied = last;
    let mut ordered: Vec<(u64, u32)> = Vec::new();
    for m in msgs {
        if m.kind != "open_week" || m.seq <= last {
            continue;
        }
        let Some(payload) = m.payload.as_deref() else { continue };
        let Ok(v) = serde_json::from_str::<serde_json::Value>(payload) else { continue };
        let Some(week) = v.get("week").and_then(|w| w.as_u64()) else { continue };
        let week = week as u32;
        if crate::services::directives::week_key(week).is_none() {
            leptos::logging::warn!("директива open_week: нет темы с номером {week}");
            continue;
        }
        ordered.push((m.seq, week));
    }
    ordered.sort_by_key(|(seq, _)| *seq);
    for (seq, week) in ordered {
        open_week(week).await;
        applied = applied.max(seq);

        crate::services::letters::add(crate::services::letters::Letter {
            id: format!("week-open-{seq}"),
            created_at: chrono::Local::now().to_rfc3339(),
            body: crate::services::directives::open_week_letter(week),
            read: false,
            action: None,
            action_done: false,
        });
    }
    if applied > last {
        crate::services::app_flags::set(WEEK_DIRECTIVE_SEQ_KEY, &applied.to_string());
        crate::services::sync::push_background();
    }
}

/// Advance the server-side read marker. Fire-and-forget at the call site.
pub async fn read(seq: u64) -> Result<(), String> {
    let body = serde_json::to_string(&ReadReq { seq }).map_err(|e| e.to_string())?;
    let _: serde_json::Value = post_json("/read", &body).await?;
    Ok(())
}

/// Вся переписка человека — со всеми собеседниками разом, по времени.
///
/// Экран чата у него ОДИН, и история не обнуляется при смене куратора: между
/// тредами рисуется разделитель, а не пустота. Порядок по времени, а не по seq:
/// номера в разных тредах свои и между собой несравнимы.
pub async fn list_messages() -> Vec<LiveMessage> {
    let mut msgs: Vec<LiveMessage> = db::list_all(MESSAGES_STORE).await;
    msgs.sort_by(|a, b| a.created_at.cmp(&b.created_at).then(a.seq.cmp(&b.seq)));
    msgs
}

/// All outbox items (optimistic / failed), ordered by `created_at` ascending.
pub async fn list_outbox() -> Vec<OutboxItem> {
    let mut items: Vec<OutboxItem> = db::list_all(OUTBOX_STORE).await;
    items.sort_by(|a, b| a.created_at.cmp(&b.created_at));
    items
}

async fn load_cursor(peer: &str) -> u64 {
    let c: Option<Cursor> = db::get(META_STORE, &cursor_key(peer)).await;
    c.map(|c| c.after_seq).unwrap_or(0)
}

async fn store_cursor(peer: &str, after_seq: u64) {
    // Only ever advance forward (a stale page must not rewind the cursor).
    let current = load_cursor(peer).await;
    if after_seq < current {
        return;
    }
    db::put(META_STORE, &Cursor { key: cursor_key(peer), after_seq }).await;
}

// ── Состояние отчёта ─────────────────────────────────────────────────────────
//
// Виджет на дашборде считает своё состояние ЗДЕСЬ, из уже скачанного треда, а не
// отдельным запросом: приложение и так опрашивает чат, и второй источник правды
// разошёлся бы с первым.

/// App-flag: `seq` запроса, панель после которого уже открывали. Дребезжание
/// гасится самим ОТКРЫТИЕМ — человек увидел, и дальше это его дело. Device-local:
/// увидеть надо на том устройстве, где смотрят.
const REPORT_SEEN_FLAG: &str = "report_request_seen_seq";

#[derive(Debug, Clone, Default, PartialEq)]
pub struct ReportStatus {
    /// Открытый запрос куратора: на сколько дней. `None` — запроса нет.
    pub request_days: Option<u32>,
    /// Когда отчёт отправляли в последний раз.
    pub last_report_at: Option<String>,
    /// Запрос есть, и панель после него ещё не открывали.
    pub needs_attention: bool,
}

/// Разобрать срок из запроса куратора. Старые запросы админки несут `dataset`, а
/// не `days`, — они живут своей панелью в чате, и виджету до них дела нет.
fn request_days(m: &LiveMessage) -> Option<u32> {
    if m.kind != "data_request" {
        return None;
    }
    let v: serde_json::Value = serde_json::from_str(m.payload.as_deref()?).ok()?;
    let d = v.get("days")?.as_u64()?;
    (d >= 1).then_some(d as u32)
}

/// Отчёт ли это (а не старый датасетный обмен).
fn is_report(m: &LiveMessage) -> bool {
    m.kind == "data_share"
        && m.sender == "user"
        && m.payload
            .as_deref()
            .and_then(|p| serde_json::from_str::<serde_json::Value>(p).ok())
            .map(|v| v.get("report").is_some())
            .unwrap_or(false)
}

pub async fn report_status() -> ReportStatus {
    let peer = current_peer();
    let msgs: Vec<LiveMessage> = list_messages()
        .await
        .into_iter()
        .filter(|m| m.peer == peer)
        .collect();

    let last_request = msgs.iter().filter_map(|m| request_days(m).map(|d| (m.seq, d))).last();
    let last_report = msgs.iter().filter(|m| is_report(m)).last();
    // Запрос закрыт отчётом, ПРИШЕДШИМ ПОСЛЕ него: повторный запрос за тем же
    // сроком должен снова ждать ответа, а не считаться выполненным старым.
    let open = match (last_request, last_report) {
        (Some((rseq, _)), Some(rep)) if rep.seq > rseq => None,
        (Some((rseq, days)), _) => Some((rseq, days)),
        (None, _) => None,
    };
    let seen = crate::services::app_flags::get(REPORT_SEEN_FLAG)
        .and_then(|s| s.parse::<u64>().ok())
        .unwrap_or(0);
    ReportStatus {
        request_days: open.map(|(_, d)| d),
        last_report_at: last_report.map(|m| m.created_at.clone()),
        needs_attention: open.map(|(seq, _)| seq > seen).unwrap_or(false),
    }
}

/// Отметить, что панель открывали: дребезжать больше не о чем. Запрос при этом
/// остаётся невыполненным — открыть и не отправить человек вправе.
pub async fn mark_report_seen() {
    let peer = current_peer();
    let newest = list_messages()
        .await
        .into_iter()
        .filter(|m| m.peer == peer)
        .filter_map(|m| request_days(&m).map(|_| m.seq))
        .last()
        .unwrap_or(0);
    if newest > 0 {
        crate::services::app_flags::set(REPORT_SEEN_FLAG, &newest.to_string());
    }
}

/// Отправить отчёт за `days` дней. Тот же путь, что у любого сообщения, — с
/// очередью отправки и повторами.
pub async fn send_report(days: u32) -> Result<LiveMessage, String> {
    let (text, payload) = crate::services::curator_share::report_message(days).await?;
    send_data_share(text, payload).await
}

// ── Текущий собеседник ───────────────────────────────────────────────────────
//
// Кто адресат — решает сервер, приложение лишь запоминает его ответ. Флаг
// device-local: это не данные человека, а состояние опроса на этом устройстве, и
// после смены куратора оно само выправится первым же ответом.

const CURRENT_PEER_FLAG: &str = "support_current_peer";

/// Собеседник, которого сервер назвал последним. До первого ответа — админ: без
/// куратора так оно и есть, а с куратором первый же опрос это поправит.
pub fn current_peer() -> String {
    crate::services::app_flags::get(CURRENT_PEER_FLAG).unwrap_or_else(default_peer)
}

fn set_current_peer(peer: &str) {
    if current_peer() != peer {
        crate::services::app_flags::set(CURRENT_PEER_FLAG, peer);
    }
}

/// Есть ли у человека куратор — по тому же признаку, по которому идут сообщения.
pub fn has_curator() -> bool {
    current_peer().starts_with("curator:")
}

// ── Persisted mode toggle (per-user-per-device, in app_flags; NOT synced) ──

const MODE_FLAG: &str = "support_chat_mode";

pub fn load_mode() -> ChatMode {
    match crate::services::app_flags::get(MODE_FLAG).as_deref() {
        Some("live") => ChatMode::Live,
        _ => ChatMode::Ai,
    }
}

pub fn save_mode(m: ChatMode) {
    crate::services::app_flags::set(MODE_FLAG, mode_str(m));
}

fn mode_str(m: ChatMode) -> &'static str {
    match m {
        ChatMode::Live => "live",
        ChatMode::Ai => "ai",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn predely_planok_zadany_dlya_vseh_indikatorov() {
        // Каждый индикатор, который куратор может править, обязан иметь предел:
        // без него директива с опечаткой прошла бы как есть.
        for k in [
            "calories", "protein", "steps", "veg_fruit", "calcium", "fiber", "iron", "heme",
            "epa_dha", "fat_ratio", "red_meat", "egg",
        ] {
            let (lo, hi) = planka_range(k).unwrap_or_else(|| panic!("{k} без предела"));
            assert!(lo < hi, "{k}: пустой диапазон");
        }
        // Колбасы числом не выражаются — правки для них нет вовсе.
        assert!(planka_range("processed_meat").is_none());
        assert!(planka_range("чушь").is_none());
    }

    #[test]
    fn predel_kalorij_lovit_opechatku_a_ne_zhiznennoe_znachenie() {
        let (lo, hi) = planka_range("calories").unwrap();
        assert!(1800.0 >= lo && 1800.0 <= hi, "обычная планка обязана проходить");
        assert!(200_000.0 > hi, "лишний ноль обязан отсекаться");
        assert!(50.0 < lo, "полсотни ккал — не планка");
    }

    #[test]
    fn mode_str_round_trips() {
        assert_eq!(mode_str(ChatMode::Ai), "ai");
        assert_eq!(mode_str(ChatMode::Live), "live");
    }

    #[test]
    fn mode_default_is_ai() {
        // The string mapping `load_mode` relies on: anything that isn't "live"
        // (including a missing flag) is AI.
        assert!(matches!(
            match None::<&str> {
                Some("live") => ChatMode::Live,
                _ => ChatMode::Ai,
            },
            ChatMode::Ai
        ));
        assert!(matches!(
            match Some("ai") {
                Some("live") => ChatMode::Live,
                _ => ChatMode::Ai,
            },
            ChatMode::Ai
        ));
        assert!(matches!(
            match Some("live") {
                Some("live") => ChatMode::Live,
                _ => ChatMode::Ai,
            },
            ChatMode::Live
        ));
    }
}
