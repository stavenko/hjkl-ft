use std::cell::RefCell;
use std::rc::Rc;

use leptos::*;
use wasm_bindgen::closure::Closure;
use wasm_bindgen::{JsCast, JsValue};
use wasm_bindgen_futures::JsFuture;
use web_sys::{Blob, MediaRecorder, MediaStream};

use crate::services::i18n::t;

/// Format wall-clock seconds as "m:ss" for the live recording / preview label.
fn fmt_duration(secs: f64) -> String {
    let total = secs.max(0.0) as u32;
    format!("{}:{:02}", total / 60, total % 60)
}

/// Bottom input bar: text + image attach + voice record + send. Staged
/// attachments (image data URL, audio data URL + duration) are surfaced through
/// the provided signals so the page can send + persist them. FAIL LOUDLY:
/// permission / API errors are surfaced into `error` (and logged), never swallowed.
#[component]
pub fn ChatInput(
    input_text: RwSignal<String>,
    pending_image: RwSignal<Option<String>>,
    pending_audio: RwSignal<Option<(String, f64)>>,
    recording: RwSignal<bool>,
    rec_start: RwSignal<f64>,
    rec_tick: RwSignal<u32>,
    sending: RwSignal<bool>,
    on_send: Callback<()>,
) -> impl IntoView {
    let error = create_rw_signal(None::<String>);

    // Live MediaRecorder + stream + collected chunks + the 1Hz tick interval.
    let recorder = store_value(None::<MediaRecorder>);
    let stream = store_value(None::<MediaStream>);
    let chunks: StoredValue<Rc<RefCell<Vec<Blob>>>> = store_value(Rc::new(RefCell::new(Vec::new())));
    let rec_interval = store_value(None::<i32>);

    let stop_tracks = move || {
        if let Some(s) = stream.get_value() {
            let tracks = s.get_tracks();
            for i in 0..tracks.length() {
                if let Some(track) = tracks.get(i).dyn_ref::<web_sys::MediaStreamTrack>() {
                    track.stop();
                }
            }
        }
        stream.set_value(None);
        if let Some(id) = rec_interval.get_value() {
            web_sys::window().unwrap().clear_interval_with_handle(id);
            rec_interval.set_value(None);
        }
    };

    // ── Image attach ──
    let image_input_id = "chat-image-input";
    let on_image_change = move |ev: leptos::ev::Event| {
        let input: web_sys::HtmlInputElement = ev.target().unwrap().unchecked_into();
        let files = match input.files() {
            Some(f) => f,
            None => return,
        };
        let file = match files.get(0) {
            Some(f) => f,
            None => return,
        };
        let mime = file.type_();
        spawn_local(async move {
            // Surface read failures into `error` like the voice path, rather than
            // panicking the wasm task.
            let array_buf = match JsFuture::from(file.array_buffer()).await {
                Ok(b) => b,
                Err(e) => {
                    let msg = format!("image read failed: {e:?}");
                    leptos::logging::error!("{msg}");
                    error.set(Some(msg));
                    return;
                }
            };
            let uint8 = js_sys::Uint8Array::new(&array_buf);
            let bytes = uint8.to_vec();
            use base64::Engine;
            let b64 = base64::engine::general_purpose::STANDARD.encode(&bytes);
            let mime = if mime.is_empty() { "image/png".to_string() } else { mime };
            pending_image.set(Some(format!("data:{mime};base64,{b64}")));
        });
    };

    // ── Voice record toggle ──
    let toggle_record = move |_| {
        error.set(None);
        if recording.get_untracked() {
            // Second tap: assemble the recording, then stop.
            if let Some(rec) = recorder.get_value() {
                let chunks_rc = chunks.get_value();
                let dur = (js_sys::Date::now() - rec_start.get_untracked()) / 1000.0;
                // onstop assembles the chunks into one Blob -> base64 data URL.
                let onstop = Closure::<dyn FnMut()>::new(move || {
                    let parts = js_sys::Array::new();
                    for b in chunks_rc.borrow().iter() {
                        parts.push(b);
                    }
                    let opts = web_sys::BlobPropertyBag::new();
                    opts.set_type("audio/webm");
                    let blob = Blob::new_with_blob_sequence_and_options(&parts, &opts)
                        .expect("failed to assemble audio blob");
                    spawn_local(async move {
                        let array_buf = JsFuture::from(blob.array_buffer()).await
                            .expect("audio array_buffer failed");
                        let uint8 = js_sys::Uint8Array::new(&array_buf);
                        let bytes = uint8.to_vec();
                        use base64::Engine;
                        let b64 = base64::engine::general_purpose::STANDARD.encode(&bytes);
                        pending_audio.set(Some((format!("data:audio/webm;base64,{b64}"), dur)));
                    });
                });
                rec.set_onstop(Some(onstop.as_ref().unchecked_ref()));
                onstop.forget();
                rec.stop().expect("MediaRecorder stop failed");
            }
            recording.set(false);
            stop_tracks();
            recorder.set_value(None);
        } else {
            // First tap (user gesture): request the mic and start recording.
            spawn_local(async move {
                let window = web_sys::window().expect("no window");
                let media_devices = match window.navigator().media_devices() {
                    Ok(md) => md,
                    Err(_) => {
                        error.set(Some(t("chat.mic_denied").to_string()));
                        return;
                    }
                };
                let constraints = web_sys::MediaStreamConstraints::new();
                constraints.set_audio(&JsValue::TRUE);
                constraints.set_video(&JsValue::FALSE);
                let promise = match media_devices.get_user_media_with_constraints(&constraints) {
                    Ok(p) => p,
                    Err(_) => {
                        error.set(Some(t("chat.mic_denied").to_string()));
                        return;
                    }
                };
                let stream_js = match JsFuture::from(promise).await {
                    Ok(s) => s,
                    Err(e) => {
                        let raw = e.as_string().unwrap_or_else(|| format!("{e:?}")).to_lowercase();
                        let msg = if raw.contains("not allowed") || raw.contains("permission") {
                            t("chat.mic_denied").to_string()
                        } else {
                            format!("microphone error: {raw}")
                        };
                        leptos::logging::error!("voice record: {msg}");
                        error.set(Some(msg));
                        return;
                    }
                };
                let media_stream: MediaStream = stream_js.unchecked_into();
                stream.set_value(Some(media_stream.clone()));

                let rec = MediaRecorder::new_with_media_stream(&media_stream)
                    .expect("MediaRecorder::new failed");

                // Collect chunks as they arrive.
                let chunks_rc = chunks.get_value();
                chunks_rc.borrow_mut().clear();
                let on_data = Closure::<dyn FnMut(web_sys::BlobEvent)>::new(move |ev: web_sys::BlobEvent| {
                    if let Some(blob) = ev.data() {
                        chunks_rc.borrow_mut().push(blob);
                    }
                });
                rec.set_ondataavailable(Some(on_data.as_ref().unchecked_ref()));
                on_data.forget();

                rec.start().expect("MediaRecorder start failed");
                recorder.set_value(Some(rec));
                rec_start.set(js_sys::Date::now());
                recording.set(true);

                // 1Hz tick for the live "0:07" label.
                let cb = Closure::<dyn Fn()>::new(move || rec_tick.update(|v| *v += 1));
                if let Ok(id) = window.set_interval_with_callback_and_timeout_and_arguments_0(
                    cb.as_ref().unchecked_ref(),
                    1000,
                ) {
                    rec_interval.set_value(Some(id));
                }
                cb.forget();
            });
        }
    };

    on_cleanup(move || {
        // `recording` is owned by the PARENT (ChatPage). On navigation away, the parent
        // may dispose it before this child cleanup runs — reading it with get_untracked()
        // would panic ("already disposed") and abort WASM, so the route never switches
        // (the chat wouldn't close). try_get_untracked() returns None instead of panicking.
        if recording.try_get_untracked().unwrap_or(false) {
            if let Some(rec) = recorder.get_value() {
                let _ = rec.stop();
            }
            stop_tracks();
        }
    });

    // Auto-growing textarea: reset to one line, then size to content (capped by
    // CSS max-height, after which it scrolls). Ported from the hjkl-chat widget.
    let textarea_ref = create_node_ref::<leptos::html::Textarea>();
    let resize = move || {
        if let Some(el) = textarea_ref.get_untracked() {
            let el: &web_sys::HtmlTextAreaElement = &el;
            el.style().set_property("height", "auto").ok();
            let h = el.scroll_height();
            el.style().set_property("height", &format!("{h}px")).ok();
        }
    };
    let reset_height = move || {
        if let Some(el) = textarea_ref.get_untracked() {
            let el: &web_sys::HtmlTextAreaElement = &el;
            el.style().set_property("height", "auto").ok();
        }
    };

    // There is something to send (typed text or a staged attachment).
    let has_content = move || {
        !input_text.get().trim().is_empty()
            || pending_image.get().is_some()
            || pending_audio.get().is_some()
    };
    let can_send = move || has_content() && !recording.get() && !sending.get();

    let do_send = move || {
        if can_send() {
            on_send.call(());
            reset_height();
        }
    };

    // Circular icon-button styles (Bulma theme vars).
    const BTN: &str = "display: inline-flex; align-items: center; justify-content: center; width: 2.5rem; height: 2.5rem; border-radius: 9999px; flex-shrink: 0; cursor: pointer; padding: 0; transition: transform 0.1s ease, background-color 0.15s ease;";
    let ghost_btn = format!("{BTN} border: 1px solid var(--bulma-border); background: var(--bulma-scheme-main-bis); color: var(--bulma-text-weak);");
    let send_btn = format!("{BTN} border: none; background: var(--bulma-link); color: #fff;");
    let stop_btn = format!("{BTN} border: none; background: var(--bulma-danger); color: #fff;");
    let ghost_btn_mic = ghost_btn.clone();

    view! {
        <div style="position: fixed; bottom: 4.75rem; left: 50%; transform: translateX(-50%); z-index: 35; width: min(26rem, calc(100% - 1.5rem)); background: var(--bulma-scheme-main); border-radius: 1.25rem; box-shadow: 0 4px 24px rgba(0,0,0,0.15); padding: 0.5rem 0.6rem;">
            // Staged-attachment preview row.
            {move || pending_image.get().map(|src| view! {
                <div style="display: flex; align-items: center; gap: 8px; margin: 2px 4px 8px 4px;">
                    <img src=src style="height: 40px; border-radius: 6px;" />
                    <button class="delete" on:click=move |_| pending_image.set(None)></button>
                </div>
            })}
            {move || pending_audio.get().map(|(_, dur)| view! {
                <div style="display: flex; align-items: center; gap: 8px; margin: 2px 4px 8px 4px;">
                    <span class="tag is-info is-light">{format!("\u{1f3a4} {}", fmt_duration(dur))}</span>
                    <button class="delete" on:click=move |_| pending_audio.set(None)></button>
                </div>
            })}
            {move || recording.get().then(|| view! {
                <div style="display: flex; align-items: center; gap: 8px; margin: 2px 4px 8px 8px;">
                    <span style="width: 9px; height: 9px; border-radius: 9999px; background: var(--bulma-danger); display: inline-block;"></span>
                    <span class="is-size-7 has-text-danger has-text-weight-semibold">
                        {move || fmt_duration({ rec_tick.get(); (js_sys::Date::now() - rec_start.get()) / 1000.0 })}
                    </span>
                    <span class="is-size-7 has-text-grey">{move || t("chat.recording")}</span>
                </div>
            })}
            {move || error.get().map(|e| view! {
                <div class="notification is-danger is-light" style="padding: 6px 10px; margin: 2px 4px 8px 4px;">{e}</div>
            })}

            // Row: [paperclip]  [expanding textarea]  [mic | send | stop]
            <div style="display: flex; align-items: flex-end; gap: 0.5rem;">
                <input type="file" accept="image/*" id=image_input_id
                    style="display: none;" on:change=on_image_change />
                <button type="button" attr:data-testid="chat-attach-image"
                    style=ghost_btn
                    title=move || t("chat.attach_image")
                    on:click=move |_| {
                        let doc = web_sys::window().unwrap().document().unwrap();
                        let el = doc.get_element_by_id(image_input_id).unwrap();
                        let input: &web_sys::HtmlInputElement = el.unchecked_ref();
                        input.click();
                    }
                >
                    <svg xmlns="http://www.w3.org/2000/svg" width="20" height="20" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">
                        <path d="m21.44 11.05-9.19 9.19a6 6 0 0 1-8.49-8.49l8.57-8.57A4 4 0 1 1 18 8.84l-8.59 8.57a2 2 0 0 1-2.83-2.83l8.49-8.48"/>
                    </svg>
                </button>

                <textarea attr:data-testid="chat-input" rows="1"
                    node_ref=textarea_ref
                    style="flex: 1; min-width: 0; min-height: 2.5rem; max-height: 9rem; padding: 0.55rem 0.85rem; border: 1px solid var(--bulma-border); border-radius: 1.25rem; background: var(--bulma-scheme-main-bis); color: var(--bulma-text); outline: none; resize: none; line-height: 1.4; overflow-y: auto; font: inherit; box-sizing: border-box;"
                    placeholder=move || t("chat.input_placeholder")
                    prop:value=move || input_text.get()
                    on:input=move |ev| { input_text.set(event_target_value(&ev)); resize(); }
                    on:keydown=move |ev: leptos::ev::KeyboardEvent| {
                        if ev.key() == "Enter" && !ev.shift_key() {
                            ev.prevent_default();
                            do_send();
                        }
                    }
                ></textarea>

                // Right control: stop while recording, send when there's content,
                // otherwise the mic (record voice).
                {move || {
                    if recording.get() {
                        view! {
                            <button type="button" attr:data-testid="chat-record-voice"
                                style=stop_btn.clone() title=move || t("chat.record_voice")
                                on:click=toggle_record>
                                <svg xmlns="http://www.w3.org/2000/svg" width="16" height="16" viewBox="0 0 24 24" fill="currentColor">
                                    <rect x="6" y="6" width="12" height="12" rx="2"/>
                                </svg>
                            </button>
                        }.into_view()
                    } else if has_content() {
                        let send_btn = send_btn.clone();
                        view! {
                            <button type="button" attr:data-testid="chat-send"
                                style=move || if sending.get() { format!("{send_btn} opacity: 0.5; pointer-events: none;") } else { send_btn.clone() }
                                title=move || t("chat.send")
                                on:click=move |_| do_send()>
                                <svg xmlns="http://www.w3.org/2000/svg" width="20" height="20" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">
                                    <path d="m22 2-7 20-4-9-9-4z"/>
                                    <path d="M22 2 11 13"/>
                                </svg>
                            </button>
                        }.into_view()
                    } else {
                        view! {
                            <button type="button" attr:data-testid="chat-record-voice"
                                style=ghost_btn_mic.clone() title=move || t("chat.record_voice")
                                on:click=toggle_record>
                                <svg xmlns="http://www.w3.org/2000/svg" width="20" height="20" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">
                                    <path d="M12 2a3 3 0 0 0-3 3v7a3 3 0 0 0 6 0V5a3 3 0 0 0-3-3z"/>
                                    <path d="M19 10v2a7 7 0 0 1-14 0v-2"/>
                                    <line x1="12" x2="12" y1="19" y2="22"/>
                                </svg>
                            </button>
                        }.into_view()
                    }
                }}
            </div>
        </div>
    }
}
