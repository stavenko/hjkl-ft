use std::cell::Cell;
use std::rc::Rc;

use leptos::*;
use leptos_router::use_navigate;
use wasm_bindgen::JsCast;

use crate::components::chat_input::ChatInput;
use crate::components::chat_message::ChatMessage as ChatMessageBubble;
use crate::components::chat_streaming::StreamingBubble;
use crate::components::live_message::LiveBubble;
use crate::components::mode_toggle::ModeToggle;
use crate::services::chat::{self, ChatMessage};
use crate::services::i18n::t;
use crate::services::support_chat::{self, ChatMode, LiveMessage, OutboxItem};
use crate::services::{ai, db, i18n};

/// Master switch for the Live-human / AI mode selector. OFF for now: the AI vs
/// Live UX isn't right yet, so the chat is Live-human-only — the `ModeToggle` is
/// hidden and the chat opens (and stays) in `ChatMode::Live`. The whole AI
/// subtree/paths are left intact behind `mode == Ai` so flipping this back to
/// `true` restores the selector. TODO: rework the AI/Live UX, then re-enable.
const MODE_SELECTOR_ENABLED: bool = false;

/// Single support chat. History is loaded from / persisted to IndexedDB (one
/// record per message). The assistant reply streams in (requesting → thinking →
/// answer) like the food lookup; it may escalate to a human via the
/// escalate_to_human tool, surfaced as a banner under that message.
#[component]
pub fn ChatPage() -> impl IntoView {
    let messages = create_rw_signal(Vec::<ChatMessage>::new());
    let input_text = create_rw_signal(String::new());
    let pending_image = create_rw_signal(None::<String>);
    let pending_audio = create_rw_signal(None::<(String, f64)>);

    // Live thread (server-backed support-worker). Entirely separate signals + stores
    // from the AI thread above — switching `mode` only swaps which subtree renders
    // and where `do_send` routes; neither thread reads or writes the other's store.
    // A push nudge from the support-worker deep-links here as `/chat?notif=1` —
    // open the Live thread (not the AI thread) and persist that choice.
    let from_notif = web_sys::window()
        .map(|w| w.location())
        .and_then(|l| l.search().ok())
        .unwrap_or_default()
        .contains("notif=1");
    // With the selector disabled the chat is Live-only; a push deep-link also
    // forces Live. Otherwise honour the saved choice.
    let initial_mode = if !MODE_SELECTOR_ENABLED || from_notif {
        ChatMode::Live
    } else {
        support_chat::load_mode()
    };
    if !MODE_SELECTOR_ENABLED || from_notif {
        support_chat::save_mode(ChatMode::Live);
    }
    let mode = create_rw_signal(initial_mode);
    let live_messages = create_rw_signal(Vec::<LiveMessage>::new());
    // Datasets already shared in this thread (top-level keys of every data_share
    // payload) — drives the «✓ Отправлено» state on the matching curator request.
    let shared_datasets = Signal::derive(move || {
        let mut set = std::collections::HashSet::<String>::new();
        for m in live_messages.get() {
            if m.kind != "data_share" {
                continue;
            }
            if let Some(p) = m.payload.as_deref() {
                if let Ok(v) = serde_json::from_str::<serde_json::Value>(p) {
                    if let Some(obj) = v.as_object() {
                        for k in obj.keys() {
                            set.insert(k.clone());
                        }
                    }
                }
            }
        }
        set
    });
    let live_outbox = create_rw_signal(Vec::<OutboxItem>::new());
    let live_sending = create_rw_signal(false);
    let live_input = create_rw_signal(String::new());
    // Unused attachment signals for the Live ChatInput (Live is text-only).
    let live_no_image = create_rw_signal(None::<String>);
    let live_no_audio = create_rw_signal(None::<(String, f64)>);
    let live_no_recording = create_rw_signal(false);
    let live_rec_start = create_rw_signal(0f64);
    let live_rec_tick = create_rw_signal(0u32);

    // Streaming / phase signals (mirror food_editor): phase 0 requesting,
    // 1 thinking, 2 answer.
    let ai_loading = create_rw_signal(false);
    let ai_phase = create_rw_signal(0u8);
    let ai_think = create_rw_signal(0u32);
    let ai_answer = create_rw_signal(0u32);
    let ai_start = create_rw_signal(0f64);
    let ai_tick = create_rw_signal(0u32);
    let ai_interval = create_rw_signal(None::<i32>);
    let streaming_text = create_rw_signal(String::new());
    // Name of the tool running mid-loop (phase 3), if any.
    let ai_tool = create_rw_signal(None::<String>);

    // Recording signals (driven by ChatInput; shared so the page can lay out).
    let recording = create_rw_signal(false);
    let rec_start = create_rw_signal(0f64);
    let rec_tick = create_rw_signal(0u32);

    // Auto-scroll to the newest message (normal chat UX): the list opens pinned to
    // the bottom and follows new messages, but if the user scrolls up to read
    // history we stop yanking them down until they return near the bottom.
    let messages_ref = create_node_ref::<leptos::html::Div>();
    let stick_bottom = create_rw_signal(true);
    let scroll_to_bottom = move || {
        if let Some(el) = messages_ref.get() {
            el.set_scroll_top(el.scroll_height());
        }
    };
    // On any scroll, remember whether the user is at/near the bottom.
    let on_messages_scroll = move |_| {
        if let Some(el) = messages_ref.get() {
            let dist = el.scroll_height() - el.scroll_top() - el.client_height();
            stick_bottom.set(dist < 120);
        }
    };
    // Follow new content when pinned. Tracks every source that grows the list
    // (AI stream + Live thread + optimistic outbox) so it fires on each update.
    create_effect(move |_| {
        messages.get();
        streaming_text.get();
        ai_loading.get();
        live_messages.get();
        live_outbox.get();
        if stick_bottom.get_untracked() {
            request_animation_frame(scroll_to_bottom);
        }
    });

    // Load history on mount.
    spawn_local(async move {
        let msgs = chat::list_messages().await;
        messages.set(msgs);
    });

    // Re-query the Live cache whenever its stores change (after every db::put from
    // send/retry/poll) and whenever we (re)enter Live mode. The poll loop never
    // writes these signals directly — it only writes the DB; the store-version
    // signals drive the re-render.
    create_effect(move |_| {
        db::version("support_messages").get();
        db::version("support_outbox").get();
        if mode.get() == ChatMode::Live {
            spawn_local(async move {
                live_messages.set(support_chat::list_messages().await);
                live_outbox.set(support_chat::list_outbox().await);
            });
        }
    });

    // Safe polling timer (NO setInterval): a component-owned alive-guard set false
    // on unmount, plus a per-entry effect that re-checks `mode`/visibility each tick
    // and self-terminates on leaving Live so loops never stack.
    let alive = Rc::new(Cell::new(true));
    on_cleanup({
        let a = alive.clone();
        move || a.set(false)
    });
    // A generation counter so rapid Live→AI→Live toggling can't STACK poll loops:
    // each (re)entry into Live bumps it; an older loop sees the bump and exits.
    let poll_gen = create_rw_signal(0u32);
    create_effect({
        let alive = alive.clone();
        move |_| {
            if mode.get() != ChatMode::Live {
                return;
            }
            poll_gen.update(|g| *g += 1);
            let my_gen = poll_gen.get_untracked();
            let alive = alive.clone();
            spawn_local(async move {
                // Immediate poll on entering Live (fast first paint of new messages).
                if let Err(e) = support_chat::poll().await {
                    logging::warn!("support poll: {e}");
                }
                loop {
                    if !alive.get() {
                        break; // unmounted
                    }
                    if mode.get_untracked() != ChatMode::Live {
                        break; // left Live
                    }
                    if poll_gen.get_untracked() != my_gen {
                        break; // a newer poll loop superseded this one
                    }
                    if document_hidden() {
                        // Don't hold a long-poll open while hidden; re-check shortly.
                        ai::sleep_ms(4000).await;
                        continue;
                    }
                    // LONG-POLL: the worker holds this ~25s and returns the instant a
                    // new message lands (or empty at the deadline). One request / 25s
                    // instead of every 4s — and the CORS preflight rate drops with it.
                    if let Err(e) = support_chat::poll_wait(25).await {
                        logging::warn!("support poll: {e}");
                        ai::sleep_ms(2000).await; // back off on transient error
                    }
                }
            });
        }
    });

    // Poll once on focus / visibility-restore while in Live mode.
    {
        let win = web_sys::window().expect("no window");
        let cb = wasm_bindgen::closure::Closure::<dyn Fn()>::new(move || {
            if mode.get_untracked() == ChatMode::Live && !document_hidden() {
                spawn_local(async move {
                    if let Err(e) = support_chat::poll().await {
                        logging::warn!("support poll: {e}");
                    }
                });
            }
        });
        let cb_ref = cb.as_ref().unchecked_ref::<js_sys::Function>().clone();
        let _ = win.add_event_listener_with_callback("focus", &cb_ref);
        let _ = win.add_event_listener_with_callback("visibilitychange", &cb_ref);
        on_cleanup(move || {
            let win = web_sys::window().expect("no window");
            let _ = win.remove_event_listener_with_callback("focus", &cb_ref);
            let _ = win.remove_event_listener_with_callback("visibilitychange", &cb_ref);
            drop(cb);
        });
    }

    let navigate = use_navigate();

    let do_send = move |_: ()| {
        // Live mode: route to the support-worker thread and return BEFORE touching
        // any AI signal/timer/stream (AI behavior stays byte-for-byte identical).
        if mode.get_untracked() == ChatMode::Live {
            if live_sending.get_untracked() {
                return;
            }
            let text = live_input.get_untracked().trim().to_string();
            if text.is_empty() {
                return;
            }
            live_input.set(String::new());
            live_sending.set(true);
            spawn_local(async move {
                // The optimistic outbox item is written inside send(); the
                // store-version effect re-renders messages + outbox.
                if let Err(e) = support_chat::send(text).await {
                    // FAIL LOUDLY (the outbox item stays "failed" and retryable).
                    logging::error!("support send: {e}");
                }
                live_sending.set(false);
            });
            return;
        }

        if ai_loading.get_untracked() {
            return;
        }
        let text = input_text.get_untracked().trim().to_string();
        let image = pending_image.get_untracked();
        let audio_pair = pending_audio.get_untracked();
        if text.is_empty() && image.is_none() && audio_pair.is_none() {
            return;
        }

        // Display text + the model note describing any attachment (the binary is
        // NOT sent — the model is text-only).
        let mut display = text.clone();
        let mut note = text.clone();
        if image.is_some() {
            let tag = t("chat.attached_image");
            display = if display.is_empty() { tag.to_string() } else { format!("{display}\n{tag}") };
            note = if note.is_empty() { tag.to_string() } else { format!("{note}\n{tag}") };
        }
        if let Some((_, dur)) = &audio_pair {
            let secs = *dur as u32;
            let tag = format!("{} {}:{:02}", t("chat.attached_voice"), secs / 60, secs % 60);
            display = if display.is_empty() { tag.clone() } else { format!("{display}\n{tag}") };
            note = if note.is_empty() { tag.clone() } else { format!("{note}\n{tag}") };
        }

        let audio = audio_pair.as_ref().map(|(url, _)| url.clone());
        let duration = audio_pair.as_ref().map(|(_, d)| *d);

        input_text.set(String::new());
        pending_image.set(None);
        pending_audio.set(None);

        let navigate = navigate.clone();
        ai_loading.set(true);
        ai_phase.set(0);
        ai_think.set(0);
        ai_answer.set(0);
        ai_tick.set(0);
        streaming_text.set(String::new());
        ai_start.set(js_sys::Date::now());

        // 1Hz tick to drive the live elapsed-seconds display.
        {
            let win = web_sys::window().unwrap();
            let cb = wasm_bindgen::closure::Closure::<dyn Fn()>::new(move || ai_tick.update(|v| *v += 1));
            if let Ok(id) = win.set_interval_with_callback_and_timeout_and_arguments_0(
                cb.as_ref().unchecked_ref(),
                1000,
            ) {
                ai_interval.set(Some(id));
            }
            cb.forget();
        }

        spawn_local(async move {
            // Persist + render the user message immediately.
            let m = chat::append_user(display, image, audio, duration).await;
            messages.update(|v| v.push(m));

            // Build the snapshot the read_progress tool reads, the registry, the
            // model-facing tool descriptions, and the system preamble.
            let snapshot = ai::build_progress_snapshot().await;
            let tools = ai::tool_descriptions(&ai::chat_registry(snapshot.clone()));
            let system = build_system(&tools, &ai::doc_index());

            // Transcript = persisted history as turns. The last (just-added) user
            // message uses the model-facing `note` (attachment note), not display.
            let msgs = messages.get_untracked();
            let last = msgs.len();
            let transcript: Vec<ai::ChatTurn> = msgs
                .iter()
                .enumerate()
                .map(|(i, mm)| ai::ChatTurn {
                    role: if mm.role == "user" { ai::ChatRole::User } else { ai::ChatRole::Assistant },
                    text: if i + 1 == last { note.clone() } else { mm.text.clone() },
                })
                .collect();

            let stop_timer = move || {
                if let Some(id) = ai_interval.get_untracked() {
                    web_sys::window().unwrap().clear_interval_with_handle(id);
                    ai_interval.set(None);
                }
            };

            // Map loop events onto the streaming-bubble signals. `Requesting`
            // resets per-turn counters so each turn's thinking/answer are fresh
            // and a prior tool turn's (empty) answer never lingers.
            let on_event = move |ev: ai::ChatEvent| match ev {
                ai::ChatEvent::Requesting => {
                    ai_phase.set(0);
                    ai_think.set(0);
                    ai_answer.set(0);
                    ai_tool.set(None);
                    streaming_text.set(String::new());
                }
                ai::ChatEvent::Thinking => {
                    ai_think.update(|v| *v += 1);
                    if ai_phase.get_untracked() == 0 {
                        ai_phase.set(1);
                    }
                }
                ai::ChatEvent::Answer(chunk) => {
                    ai_answer.update(|v| *v += 1);
                    if ai_phase.get_untracked() != 2 {
                        ai_phase.set(2);
                    }
                    streaming_text.update(|s| s.push_str(chunk));
                }
                ai::ChatEvent::ToolCall(name, params) => {
                    ai_tool.set(Some(format!("{name} {params}")));
                    ai_phase.set(3);
                }
                ai::ChatEvent::ToolDone(_) => {}
            };

            match ai::chat_agent(system, transcript, snapshot, on_event).await {
                Ok(ai::ChatOutcome { answer, escalated, tools }) => {
                    stop_timer();
                    ai_loading.set(false);
                    streaming_text.set(String::new());
                    // Persist each tool call (shown inline as "Assistant requested
                    // tool: …" and gathered into the Context section) BEFORE the
                    // final answer, so the chat reads call → answer in order.
                    for inv in tools {
                        let tm = chat::append_tool_call(inv.name, inv.params, inv.result).await;
                        messages.update(|v| v.push(tm));
                    }
                    let am = chat::append_assistant(answer, escalated).await;
                    messages.update(|v| v.push(am));
                }
                Err(e) => {
                    stop_timer();
                    ai_loading.set(false);
                    streaming_text.set(String::new());
                    if e.contains("HTTP 402") {
                        navigate("/settings/subscription", Default::default());
                    } else {
                        // FAIL LOUDLY: surface the error as an assistant-styled
                        // bubble and log it — never drop it silently.
                        leptos::logging::error!("chat_agent error: {e}");
                        let am = chat::append_assistant(format!("\u{26a0}\u{fe0f} {e}"), false).await;
                        messages.update(|v| v.push(am));
                    }
                }
            }
        });
    };

    // One callback shared by both ChatInput instances (it self-routes by `mode`).
    let on_send = Callback::new(do_send);

    // Messages shown in the stream: everything except tool_call records (those
    // live only in the "Context" section and the model's context).
    let stream_messages = move || {
        messages.get().into_iter().filter(|m| m.role != "tool_call").collect::<Vec<_>>()
    };

    // The "Context" section: the last ≤7 tool calls (with results) across the
    // whole chat — the same bounded tool context the model is fed.
    let context_tools = move || {
        let all: Vec<ChatMessage> =
            messages.get().into_iter().filter(|m| m.role == "tool_call").collect();
        let from = all.len().saturating_sub(7);
        all[from..].to_vec()
    };

    view! {
        // Pinned app-shell: position: fixed makes the page immune to document
        // scroll/overscroll (so the title never slides up under the status bar),
        // and padding-top: safe-area-inset keeps it below the notch. The header is
        // flex-shrink: 0 (pinned); only the messages area scrolls under it. The
        // Soft pastel gradient wallpaper (blue base with purple / periwinkle /
        // green glows) fills the whole viewport behind the messages.
        <div style="position: fixed; inset: 0; padding: env(safe-area-inset-top) 0.75rem 0; display: flex; flex-direction: column; \
                background: \
                    radial-gradient(120% 80% at 0% 12%, #E7CCFB 0%, rgba(231,204,251,0) 60%), \
                    radial-gradient(120% 90% at 0% 100%, #A5B3F9 0%, rgba(165,179,249,0) 60%), \
                    radial-gradient(120% 90% at 100% 100%, #DDE9CE 0%, rgba(221,233,206,0) 62%), \
                    #C1E1FC;">

            // Live/AI selector hidden while MODE_SELECTOR_ENABLED is off (Live-only).
            {MODE_SELECTOR_ENABLED.then(|| view! { <ModeToggle mode=mode /> })}

            <Show when=move || mode.get() == ChatMode::Ai && !context_tools().is_empty()>
                <details attr:data-testid="chat-context"
                    style="flex-shrink: 0; max-width: 30rem; width: 100%; margin: 0 auto 0.5rem;">
                    <summary class="is-size-7 has-text-grey" style="cursor: pointer;">
                        {move || format!("{} ({})", t("chat.context"), context_tools().len())}
                    </summary>
                    <div style="max-height: 30vh; overflow-y: auto; background: var(--bulma-scheme-main-bis); border-radius: 8px; padding: 8px 10px; margin-top: 4px;">
                        <For
                            each=move || context_tools()
                            key=|m| m.id.clone()
                            children=move |m| {
                                let name = m.tool_name.clone().unwrap_or_default();
                                let params = m.tool_params.clone().unwrap_or_default();
                                let result = m.tool_result.clone().unwrap_or_default();
                                view! {
                                    <div style="margin-bottom: 8px;">
                                        <p class="is-size-7" style="margin: 0; font-family: monospace; word-break: break-word;">
                                            <strong>{name}</strong>" "{params}
                                        </p>
                                        <p class="is-size-7 has-text-grey" style="margin: 2px 0 0; font-family: monospace; word-break: break-word; white-space: pre-wrap;">
                                            {format!("\u{2192} {result}")}
                                        </p>
                                    </div>
                                }
                            }
                        />
                    </div>
                </details>
            </Show>

            // Scroll container. `min-height: 0` is REQUIRED for a `flex: 1` item to
            // actually scroll instead of growing its parent (flex items default to
            // min-height: auto). `-webkit-overflow-scrolling: touch` gives iOS native
            // momentum. The inner wrapper (`min-height: 100%; justify-content: flex-end`)
            // anchors messages to the BOTTOM when the thread is short — the reference's
            // pattern — so the newest message shows without a jump.
            <div node_ref=messages_ref on:scroll=on_messages_scroll attr:data-testid="chat-messages" attr:data-ios-scroll="1" style="flex: 1; min-height: 0; overflow-y: auto; -webkit-overflow-scrolling: touch; overscroll-behavior: contain; max-width: 30rem; width: 100%; margin: 0 auto;">
              <div style="position: relative; min-height: 100%;">
                // Line-art pattern OVERLAY blended onto the gradient, behind the
                // messages (so the bubbles stay crisp). Repeats down the thread.
                <div style="position: absolute; inset: 0; z-index: 0; pointer-events: none; \
                        background-image: url('/chat-bg-pattern.svg'); background-repeat: repeat-y; \
                        background-size: 100% auto; background-position: top center; mix-blend-mode: overlay;"></div>
                <div style="position: relative; z-index: 1; display: flex; flex-direction: column; min-height: 100%; justify-content: flex-end; padding-bottom: 9rem;">
                // ── AI thread ──
                <Show when=move || mode.get() == ChatMode::Ai>
                    <Show
                        when=move || !messages.get().is_empty() || ai_loading.get()
                        fallback=move || view! {
                            <p class="has-text-grey has-text-centered" style="margin-top: 2rem;">
                                {move || t("chat.empty")}
                            </p>
                        }
                    >
                        <For
                            // tool_call messages are NOT drawn in the stream; they live
                            // only in the collapsible "Context" section (and the model's
                            // context). Only user / assistant bubbles render here.
                            each=move || stream_messages()
                            key=|m| m.id.clone()
                            children=move |m| view! { <ChatMessageBubble msg=m /> }
                        />
                        <Show when=move || ai_loading.get()>
                            <StreamingBubble
                                ai_phase=ai_phase ai_think=ai_think ai_answer=ai_answer
                                ai_tick=ai_tick ai_start=ai_start streaming_text=streaming_text
                                ai_tool=ai_tool />
                        </Show>
                    </Show>
                </Show>

                // ── Live thread (server-backed support-worker) ──
                <Show when=move || mode.get() == ChatMode::Live>
                    <Show
                        when=move || !live_messages.get().is_empty() || !live_outbox.get().is_empty()
                        fallback=move || view! {
                            <p class="has-text-grey has-text-centered" style="margin-top: 2rem;">
                                {move || t("chat.live_empty")}
                            </p>
                        }
                    >
                        <For
                            each=move || live_messages.get()
                            key=|m| m.seq
                            children=move |m| view! { <LiveBubble msg=m shared=shared_datasets /> }
                        />
                        // Optimistic outbox: right-aligned bubbles with a sending /
                        // retry affordance.
                        <For
                            // Hide an outbox item once its message has landed as a
                            // server row (reconciled by client_id) — avoids a transient
                            // double-render in the lost-ack window.
                            each=move || {
                                let seen: std::collections::HashSet<String> =
                                    live_messages.get().into_iter().map(|m| m.client_id).collect();
                                live_outbox.get().into_iter().filter(|o| !seen.contains(&o.client_id)).collect::<Vec<_>>()
                            }
                            key=|o| o.client_id.clone()
                            children=move |o| {
                                let failed = o.status == "failed";
                                let client_id = o.client_id.clone();
                                view! {
                                    <div attr:data-testid="live-outbox" attr:data-status=o.status.clone()
                                        style="display: flex; flex-direction: column; margin-bottom: 10px;">
                                        <div style="background: #DEF7EC; color: #04603F; border: 1px solid #A7E3CD; border-radius: 12px; padding: 14px 16px; max-width: 80%; margin-left: auto; opacity: 0.6;">
                                            <p class="is-size-6" style="white-space: pre-wrap; line-height: 1.45; margin: 0;">
                                                {o.text.clone()}
                                            </p>
                                        </div>
                                        {if failed {
                                            let client_id = client_id.clone();
                                            view! {
                                                <button attr:data-testid="live-retry"
                                                    class="is-size-7 has-text-danger"
                                                    style="background: none; border: none; cursor: pointer; margin: 4px 0 0 auto; padding: 0;"
                                                    on:click=move |_| {
                                                        let client_id = client_id.clone();
                                                        spawn_local(async move {
                                                            if let Err(e) = support_chat::retry(client_id).await {
                                                                logging::error!("support retry: {e}");
                                                            }
                                                        });
                                                    }>
                                                    {move || t("chat.live_retry")}
                                                </button>
                                            }.into_view()
                                        } else {
                                            view! {
                                                <span class="is-size-7 has-text-grey" style="margin: 4px 0 0 auto;">
                                                    {move || t("chat.live_sending")}
                                                </span>
                                            }.into_view()
                                        }}
                                    </div>
                                }
                            }
                        />
                    </Show>
                </Show>
                </div>
              </div>
            </div>

            // AI input (with attachments). Hidden in Live mode.
            <Show when=move || mode.get() == ChatMode::Ai>
                <ChatInput
                    input_text=input_text
                    pending_image=pending_image
                    pending_audio=pending_audio
                    recording=recording
                    rec_start=rec_start
                    rec_tick=rec_tick
                    sending=ai_loading
                    on_send=on_send
                />
            </Show>

            // Live input (text-only; attachment signals are inert). Separate draft
            // signal so toggling never clobbers the AI draft.
            <Show when=move || mode.get() == ChatMode::Live>
                <ChatInput
                    input_text=live_input
                    pending_image=live_no_image
                    pending_audio=live_no_audio
                    recording=live_no_recording
                    rec_start=live_rec_start
                    rec_tick=live_rec_tick
                    sending=live_sending
                    on_send=on_send
                />
            </Show>
        </div>
    }
}

/// True when the document is hidden (background tab / app backgrounded) — used to
/// skip Live polling ticks while the page isn't visible.
fn document_hidden() -> bool {
    web_sys::window()
        .and_then(|w| w.document())
        .map(|d| d.hidden())
        .unwrap_or(false)
}

fn lang_name() -> &'static str {
    match i18n::get_lang() {
        i18n::Lang::Ru => "Russian",
        i18n::Lang::En => "English",
    }
}

/// Fixed system preamble for the chat loop: assistant role, language, the
/// tool-use protocol (`[[tool]]` to call, `[[final]]` to finish), and the
/// attachment-note convention. The conversation itself is appended by
/// `ai::chat_agent` from the transcript; this is just the preamble.
fn build_system(tools: &str, docs: &str) -> String {
    format!(
        "You are a friendly in-app support assistant for a weight-loss tracker. \
         Help the user navigate the tracker and articles, and answer their questions. \
         Reply in {lang}. Plain text only.\n\n\
         You can use these tools:\n{tools}\n\n\
         To CALL a tool, output ONLY this on its own line and nothing else:\n\
         {tool_prefix} <tool_name> <json-arguments>\n\
         Example: {tool_prefix} read_progress {{\"days\": 30}}\n\
         You will then receive a line `Tool result: <name> -> <json>`; after it you may \
         call another tool or answer.\n\n\
         When you are ready to answer the user, output your answer prefixed with \
         {final_prefix} (the rest of the line and below is the answer), e.g.:\n\
         {final_prefix} Your weight is trending down — keep it up!\n\
         ALWAYS finish with a {final_prefix} answer. Never mention these tools or markers to the user.\n\n\
         Use read_progress before giving progress feedback so you cite real numbers.\n\
         Use read_documentation to fetch a how-to doc BEFORE explaining how a feature works, \
         so your advice matches the real app. Available docs (doc_id: topic):\n{docs}\n\
         When the user reports something broken or wrong, help them file it: gather a short \
         title, the area, steps to reproduce, what they expected and what actually happened, \
         then call file_bug_report. Confirm to the user once it is submitted.\n\
         Use escalate_to_human only when a real operator is needed (billing, account issues, \
         anything you cannot resolve).\n\n\
         Attachments are referenced in the user's text as notes like \"{img_tag}\" \
         or \"{voice_tag} 0:07\"; you cannot see their contents, only that they exist.",
        lang = lang_name(),
        tools = tools,
        docs = docs,
        tool_prefix = ai::TOOL_PREFIX,
        final_prefix = ai::FINAL_PREFIX,
        img_tag = t("chat.attached_image"),
        voice_tag = t("chat.attached_voice"),
    )
}
