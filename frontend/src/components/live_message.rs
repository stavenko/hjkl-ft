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
    /// The set of datasets already shared in this thread (top-level keys of every
    /// `data_share` payload) — so a fulfilled request renders as «✓ Отправлено».
    #[prop(into)] shared: Signal<std::collections::HashSet<String>>,
) -> impl IntoView {
    // A curator data-request → the compact share button (only when it names a
    // known dataset; an unknown/garbled request falls through to plain text so we
    // never silently drop the message). Already-shared → the button shows as done.
    if msg.kind == "data_request" {
        if let Some(dataset) = request_dataset(&msg) {
            let already = shared.get_untracked().contains(dataset_id(dataset));
            return view! { <RequestPanel dataset=dataset already=already /> }.into_view();
        }
    }

    // The user's own `data_share` confirmation is redundant with the request's
    // «✓ Отправлено» state — don't draw a separate bubble for it.
    if msg.kind == "data_share" {
        return ().into_view();
    }

    let is_user = msg.sender == "user";
    // re:Norma palette: the user bubble is the soft-emerald brand tint (not the
    // stock nuclear-blue link); the curator bubble is a white surface. Both carry
    // a light brand-coloured border so they read on the pastel wallpaper.
    let bubble_style = if is_user {
        "background: #DEF7EC; color: #04603F; border: 1px solid #A7E3CD; border-radius: 12px; padding: 14px 16px; max-width: 80%; margin-left: auto;"
    } else {
        "background: #FFFFFF; color: var(--bulma-text); border: 1px solid #E4E8F0; border-radius: 12px; padding: 14px 16px; max-width: 80%; margin-right: auto;"
    };

    let text = msg.text.clone();
    view! {
        <div attr:data-testid="live-message" attr:data-role=msg.sender.clone()
            style="display: flex; flex-direction: column; margin-bottom: 10px;">
            <div style=bubble_style>
                <p class="is-size-6" style="white-space: pre-wrap; line-height: 1.45; margin: 0;">
                    {text}
                </p>
            </div>
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
        Dataset::All => "все данные",
    }
}

fn dataset_id(d: Dataset) -> &'static str {
    match d {
        Dataset::Body => "body",
        Dataset::Food => "food",
        Dataset::Weight => "weight",
        Dataset::Steps => "steps",
        Dataset::All => "all",
    }
}
