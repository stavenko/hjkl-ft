use leptos::*;

use crate::services::curator_share::{self, Dataset};
use crate::services::i18n::t;
use crate::services::support_chat::{self, LiveMessage};

/// One Live-thread bubble. User messages align right (link-tinted), expert
/// messages align left (neutral card). A `data_request` message renders a
/// visually distinct curator panel with a "Поделиться" button (see [`RequestPanel`]);
/// every other kind (plain text, the user's own `data_share` confirmation) renders
/// as a normal text bubble.
#[component]
pub fn LiveBubble(msg: LiveMessage) -> impl IntoView {
    // A curator data-request → the distinct share panel (only when it names a
    // known dataset; an unknown/garbled request falls through to plain text so we
    // never silently drop the message).
    if msg.kind == "data_request" {
        if let Some(dataset) = request_dataset(&msg) {
            return view! { <RequestPanel dataset=dataset fallback=msg.text.clone() /> }.into_view();
        }
    }

    let is_user = msg.sender == "user";
    let bubble_style = if is_user {
        "background: var(--bulma-link); color: var(--bulma-link-invert); border-radius: 12px; padding: 14px 16px; max-width: 80%; margin-left: auto;"
    } else {
        "background: var(--bulma-scheme-main-bis); border-radius: 12px; padding: 14px 16px; max-width: 80%; margin-right: auto;"
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
fn RequestPanel(dataset: Dataset, fallback: String) -> impl IntoView {
    // "idle" | "sending" | "done" | error string
    let state = create_rw_signal(String::from("idle"));

    // The panel text: the localized dataset prompt, falling back to the message's
    // own RU text if the i18n key is somehow missing.
    let panel_text = {
        let localized = t(dataset.panel_key());
        if localized.is_empty() { fallback } else { localized.to_string() }
    };

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

    view! {
        <div attr:data-testid="live-request" attr:data-dataset=dataset_id(dataset)
            style="display: flex; flex-direction: column; margin-bottom: 10px;">
            <div style="background: var(--bulma-scheme-main-bis); border: 2px solid var(--bulma-link); \
                        border-radius: 12px; padding: 14px 16px; max-width: 85%; margin-right: auto;">
                <div style="display: flex; align-items: center; gap: 8px; margin-bottom: 8px;">
                    <span aria-hidden="true" style="font-size: 1.25rem; line-height: 1;">"📋"</span>
                    <span class="is-size-7 has-text-weight-semibold has-text-link">
                        {move || t("curator.request_title")}
                    </span>
                </div>
                <p class="is-size-6" style="white-space: pre-wrap; line-height: 1.45; margin: 0 0 12px 0;">
                    {panel_text}
                </p>
                <Show
                    when=move || state.get() == "done"
                    fallback=move || {
                        let sending = move || state.get() == "sending";
                        let err = move || {
                            let s = state.get();
                            (s != "idle" && s != "sending" && s != "done").then_some(s)
                        };
                        view! {
                            <button attr:data-testid="live-request-share"
                                class="button is-link is-small"
                                prop:disabled=sending
                                on:click=on_share>
                                {move || if sending() { t("curator.sharing") } else { t("curator.share") }}
                            </button>
                            <Show when=move || err().is_some() fallback=|| ()>
                                <p class="is-size-7 has-text-danger" style="margin: 6px 0 0 0;">
                                    {move || err().unwrap_or_default()}
                                </p>
                            </Show>
                        }
                    }
                >
                    <p class="is-size-7 has-text-success" style="margin: 0;" data-testid="live-request-done">
                        {move || t("curator.shared_done")}
                    </p>
                </Show>
            </div>
        </div>
    }
}

fn dataset_id(d: Dataset) -> &'static str {
    match d {
        Dataset::Body => "body",
        Dataset::Food => "food",
        Dataset::Story => "story",
        Dataset::Weight => "weight",
        Dataset::Steps => "steps",
        Dataset::All => "all",
    }
}
