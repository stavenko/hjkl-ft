use leptos::*;

use crate::services::i18n::t;
use crate::services::support_chat::{self, ChatMode};

/// Binary segmented control in the chat header: AI thread vs Live (server-backed)
/// support thread. Writing the signal also persists the choice (per-user-per-device)
/// so it survives reload.
#[component]
pub fn ModeToggle(mode: RwSignal<ChatMode>) -> impl IntoView {
    let set_mode = move |m: ChatMode| {
        support_chat::save_mode(m);
        mode.set(m);
    };

    view! {
        <div class="mode-toggle" attr:data-testid="chat-mode-toggle"
             style="display:flex; gap:4px; max-width:30rem; width:100%; margin:0 auto 0.5rem; flex-shrink:0;">
            <button attr:data-testid="chat-mode-ai" class="button is-small"
                class:is-link=move || mode.get() == ChatMode::Ai
                on:click=move |_| set_mode(ChatMode::Ai)>
                {move || t("chat.mode_ai")}
            </button>
            <button attr:data-testid="chat-mode-live" class="button is-small"
                class:is-link=move || mode.get() == ChatMode::Live
                on:click=move |_| set_mode(ChatMode::Live)>
                {move || t("chat.mode_live")}
            </button>
        </div>
    }
}
