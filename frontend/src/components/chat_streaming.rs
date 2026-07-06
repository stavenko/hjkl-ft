use leptos::*;

use crate::services::i18n::t;

/// Transient assistant bubble shown while a reply streams in. Mirrors the
/// food-editor phase UI (requesting → thinking(tok) → answer(tok) + elapsed
/// seconds) but routes the labels through `t()` and streams the answer text
/// into the bubble live. Phase 3 marks a tool running mid-loop (`ai_tool` holds
/// its name). Not persisted — replaced by a real ChatMessage on completion.
#[component]
pub fn StreamingBubble(
    ai_phase: RwSignal<u8>,
    ai_think: RwSignal<u32>,
    ai_answer: RwSignal<u32>,
    ai_tick: RwSignal<u32>,
    ai_start: RwSignal<f64>,
    streaming_text: RwSignal<String>,
    ai_tool: RwSignal<Option<String>>,
) -> impl IntoView {
    // Seconds since the request started; reads ai_tick so it re-renders each second.
    let elapsed = move || -> u32 {
        ai_tick.get();
        ((js_sys::Date::now() - ai_start.get()) / 1000.0).max(0.0) as u32
    };

    let header = move || match ai_phase.get() {
        0 => format!("\u{231b} {} \u{00b7} {}s", t("chat.requesting"), elapsed()),
        1 => format!("\u{1f9e0} {} ({} tok) \u{00b7} {}s", t("chat.thinking"), ai_think.get(), elapsed()),
        3 => format!("\u{1f527} {} {} \u{00b7} {}s", t("chat.tool_running"), ai_tool.get().unwrap_or_default(), elapsed()),
        _ => format!("\u{270d}\u{fe0f} {} ({} tok) \u{00b7} {}s", t("chat.answer"), ai_answer.get(), elapsed()),
    };

    view! {
        <div attr:data-testid="chat-streaming"
            style="display: flex; flex-direction: column; margin-bottom: 10px;">
            <div style="background: var(--bulma-scheme-main-bis); border-radius: 12px; padding: 14px 16px; max-width: 80%; margin-right: auto;">
                <p class="is-size-7 has-text-grey" style="margin: 0;">{header}</p>
                <Show when=move || { ai_phase.get() == 2 }>
                    <p class="is-size-6" style="white-space: pre-wrap; line-height: 1.45; margin: 6px 0 0 0;">
                        {move || streaming_text.get()}
                    </p>
                </Show>
            </div>
        </div>
    }
}
