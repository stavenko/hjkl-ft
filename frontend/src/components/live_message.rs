use leptos::*;

use crate::services::support_chat::LiveMessage;

/// One Live-thread bubble. User messages align right (link-tinted), expert
/// messages align left (neutral card). No escalation banner, no attachments —
/// the Live thread is text-only.
#[component]
pub fn LiveBubble(msg: LiveMessage) -> impl IntoView {
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
}
