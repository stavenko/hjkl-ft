use leptos::*;

use crate::services::chat::ChatMessage;
use crate::services::i18n::t;

/// Format wall-clock seconds as "m:ss" for the voice-clip label.
fn fmt_duration(secs: f64) -> String {
    let total = secs.max(0.0) as u32;
    format!("{}:{:02}", total / 60, total % 60)
}

/// One chat bubble. User messages align right, assistant left. Renders any
/// staged image / voice attachment and, for assistant messages flagged as
/// escalated, a warning banner under the bubble.
#[component]
pub fn ChatMessage(msg: ChatMessage) -> impl IntoView {
    let is_user = msg.role == "user";

    // User: link-tinted bubble pushed right. Assistant: neutral card pushed left.
    let bubble_style = if is_user {
        "background: var(--bulma-link); color: var(--bulma-link-invert); border-radius: 12px; padding: 14px 16px; max-width: 80%; margin-left: auto;"
    } else {
        "background: var(--bulma-scheme-main-bis); border-radius: 12px; padding: 14px 16px; max-width: 80%; margin-right: auto;"
    };

    let text = msg.text.clone();
    let image = msg.image.clone();
    let audio = msg.audio.clone();
    let duration = msg.duration_secs;
    let escalated = msg.escalated;

    view! {
        <div attr:data-testid="chat-message" attr:data-role=msg.role.clone()
            style="display: flex; flex-direction: column; margin-bottom: 10px;">
            <div style=bubble_style>
                {(!text.is_empty()).then(|| view! {
                    <p class="is-size-6" style="white-space: pre-wrap; line-height: 1.45; margin: 0;">
                        {text.clone()}
                    </p>
                })}
                {image.clone().map(|src| view! {
                    <img src=src style="max-width: 100%; border-radius: 8px; margin-top: 6px; display: block;" />
                })}
                {audio.clone().map(|src| view! {
                    <div style="margin-top: 6px;">
                        <audio controls src=src style="max-width: 100%;"></audio>
                        {duration.map(|d| view! {
                            <span class="is-size-7 has-text-grey" style="margin-left: 6px;">
                                {fmt_duration(d)}
                            </span>
                        })}
                    </div>
                })}
            </div>
            <Show when=move || escalated>
                <div class="notification is-warning is-light"
                    style="margin: 6px 0 0 0; padding: 8px 12px; max-width: 80%; margin-right: auto;">
                    {move || t("chat.escalated_banner")}
                </div>
            </Show>
        </div>
    }
}
