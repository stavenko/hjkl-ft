use leptos::*;

use crate::services::curator_share::{self, Dataset};
use crate::services::support_chat::{self, LiveMessage};

/// One Live-thread bubble. User messages align right (link-tinted), expert
/// messages align left (neutral card). A `data_request` message renders a
/// visually distinct curator panel with a "Поделиться" button (see [`RequestPanel`]);
/// every other kind (plain text, the user's own `data_share` confirmation) renders
/// as a normal text bubble.
#[component]
pub fn LiveBubble(
    msg: LiveMessage,
    /// Every dataset share in the thread as `(seq, dataset_key)`. A request is
    /// «✓ Отправлено» only when a matching share arrived AFTER it (seq-aware), so a
    /// REPEAT request for the same dataset is still offered a fresh share button.
    #[prop(into)] shared: Signal<Vec<(u64, String)>>,
) -> impl IntoView {
    // A curator data-request → the compact share button (only when it names a
    // known dataset; an unknown/garbled request falls through to plain text so we
    // never silently drop the message). Fulfilled by a LATER share → shows as done.
    if msg.kind == "data_request" {
        if let Some(dataset) = request_dataset(&msg) {
            let req_seq = msg.seq;
            let key = dataset_id(dataset);
            let already = shared
                .get_untracked()
                .iter()
                .any(|(seq, k)| *seq > req_seq && k == key);
            return view! { <RequestPanel dataset=dataset already=already /> }.into_view();
        }
    }

    // The user's own `data_share` confirmation is redundant with the request's
    // «✓ Отправлено» state — don't draw a separate bubble for it.
    if msg.kind == "data_share" {
        return ().into_view();
    }

    // A curator `set_planka` directive — the app itself applies it (support_chat);
    // here we just show a centred system note so the user sees it happened.
    if msg.kind == "set_planka" {
        let amount = msg
            .payload
            .as_deref()
            .and_then(|p| serde_json::from_str::<serde_json::Value>(p).ok())
            .and_then(|v| v.get("amount").and_then(|a| a.as_f64()));
        let Some(amount) = amount else { return ().into_view() };
        // Текст собирается ЗДЕСЬ, на языке приложения: директива несёт только
        // число, и язык плашки не должен зависеть от настроек куратора.
        let text = crate::services::directives::set_planka_note("calories", amount);
        // Colours in a `let` (matches the bubble styles below) — soft info card.
        return view! { <chat_ui::Note text=text /> }.into_view();
    }

    // Правка любой планки — системной запиской: применяет её приложение, отвечать
    // здесь не на что. Текст собирается из своих строк, на языке человека.
    if msg.kind == "set_planka_v2" {
        let v = msg
            .payload
            .as_deref()
            .and_then(|p| serde_json::from_str::<serde_json::Value>(p).ok());
        let key = v
            .as_ref()
            .and_then(|v| v.get("key").and_then(|k| k.as_str()))
            .unwrap_or_default()
            .to_string();
        let amount = v.as_ref().and_then(|v| v.get("amount").and_then(|a| a.as_f64()));
        // Директива без числа — испорченная: планка это ЧИСЛО и ничего кроме.
        // Рисовать по ней плашку нечем.
        let Some(amount) = amount else { return view! {}.into_view() };
        let text = crate::services::directives::set_planka_note(&key, amount);
        return view! { <chat_ui::Note text=text /> }.into_view();
    }

    // Директива открытия темы — так же системной запиской: применяет её
    // приложение, отвечать здесь не на что.
    if msg.kind == "open_week" {
        let week = msg
            .payload
            .as_deref()
            .and_then(|p| serde_json::from_str::<serde_json::Value>(p).ok())
            .and_then(|v| v.get("week").and_then(|w| w.as_u64()));
        let text = crate::services::directives::open_week_note(week.map(|w| w as u32));
        return view! { <chat_ui::Note text=text /> }.into_view();
    }

    // Подпись под пузырём собеседника. Экран чата один на всех, и без имени
    // куратор неотличим от поддержки — особенно выше разделителя, когда человек
    // листает старую переписку.
    let sender_name = (msg.sender != "user")
        .then(|| msg.sender_name.clone())
        .flatten();

    // Пузырь рисует ОБЩИЙ крейт: тот же разговор виден куратору с его конца, и
    // выглядеть он обязан так же. `mine` — не «от кого пришло», а «моё ли»: у
    // худеющего своё то, что от него.
    view! {
        <div attr:data-testid="live-message" attr:data-role=msg.sender.clone()>
            <chat_ui::Bubble text=msg.text.clone() mine=msg.sender == "user"
                sender_name=sender_name />
        </div>
    }
    .into_view()
}

/// Parse the requested dataset from a `data_request` message: prefer the typed
/// `payload` ({"dataset": …}); if absent/garbled, `None` (caller falls back).
fn request_dataset(msg: &LiveMessage) -> Option<Dataset> {
    let raw = msg.payload.as_deref()?;
    let v: serde_json::Value = serde_json::from_str(raw).ok()?;
    Dataset::from_str(v.get("dataset")?.as_str()?)
}

/// The curator request panel: a distinct accented card with an icon, the RU panel
/// text for the dataset, and a "Поделиться" button that gathers the real data and
/// sends it back as a data_share message.
#[component]
fn RequestPanel(dataset: Dataset, already: bool) -> impl IntoView {
    // "idle" | "sending" | "done" | error string. `already` = a data_share for this
    // dataset is already in the thread (fulfilled on a previous session).
    let state = create_rw_signal(String::from(if already { "done" } else { "idle" }));

    let on_share = move |_| {
        if state.get() == "sending" || state.get() == "done" {
            return;
        }
        state.set("sending".to_string());
        spawn_local(async move {
            match curator_share::share_message(dataset).await {
                Ok((text, payload)) => match support_chat::send_data_share(text, payload).await {
                    Ok(_) => state.set("done".to_string()),
                    Err(e) => {
                        logging::error!("curator share send: {e}");
                        state.set(e);
                    }
                },
                Err(e) => {
                    logging::error!("curator share build: {e}");
                    state.set(e);
                }
            }
        });
    };

    let done_label = format!("✓ Отправлено · {}", dataset_short(dataset));
    view! {
        <div attr:data-testid="live-request" attr:data-dataset=dataset_id(dataset)
            style="display: flex; flex-direction: column; align-items: flex-start; margin-bottom: 10px;">
            // The «Куратор запрашивает» caption stays in BOTH states.
            <span class="is-size-7" style="color: #6B7491; margin: 0 0 5px 2px;">
                "Куратор запрашивает"
            </span>
            <Show
                when=move || state.get() == "done"
                fallback=move || {
                    let sending = move || state.get() == "sending";
                    let err = move || {
                        let s = state.get();
                        (s != "idle" && s != "sending" && s != "done").then_some(s)
                    };
                    let label = format!("Отправить {}", dataset_short(dataset));
                    view! {
                        // Compact pill: the curator's data request is just a button.
                        <button attr:data-testid="live-request-share"
                            prop:disabled=sending
                            on:click=on_share
                            style="display: inline-flex; align-items: center; gap: 6px; \
                                   background: #DEF7EC; color: #04603F; border: 1px solid #A7E3CD; \
                                   border-radius: 999px; padding: 9px 15px; font-weight: 600; \
                                   font-size: 0.92rem; cursor: pointer;">
                            <span aria-hidden="true">"📤"</span>
                            {move || if sending() { "Отправляю…".to_string() } else { label.clone() }}
                        </button>
                        <Show when=move || err().is_some() fallback=|| ()>
                            <p class="is-size-7 has-text-danger" style="margin: 6px 0 0 0;">
                                {move || err().unwrap_or_default()}
                            </p>
                        </Show>
                    }
                }
            >
                <span class="is-size-7 has-text-weight-semibold" style="color: #04603F;"
                    data-testid="live-request-done">
                    {done_label.clone()}
                </span>
            </Show>
        </div>
    }
}

/// Short RU name of a dataset for the compact «Отправить …» button.
fn dataset_short(d: Dataset) -> &'static str {
    match d {
        Dataset::Body => "параметры тела",
        Dataset::Food => "дневник питания",
        Dataset::Weight => "дневник веса",
        Dataset::Steps => "дневник шагов",
        Dataset::System => "данные об устройстве",
        Dataset::All => "все данные",
    }
}

fn dataset_id(d: Dataset) -> &'static str {
    match d {
        Dataset::Body => "body",
        Dataset::Food => "food",
        Dataset::Weight => "weight",
        Dataset::Steps => "steps",
        Dataset::System => "system",
        Dataset::All => "all",
    }
}
