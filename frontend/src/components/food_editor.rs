use std::collections::BTreeMap;
use std::rc::Rc;

use leptos::*;
use api_types::*;
use wasm_bindgen::{JsCast, JsValue};
use wasm_bindgen_futures::JsFuture;

use leptos_router::use_navigate;

use crate::services::ai;
use crate::services::i18n::t;
use crate::services::local;
use crate::services::subscription;

/// Decode any picked image (incl. iOS **HEIC**) and re-encode it as a downscaled
/// JPEG, returned as base64 (no data-URL prefix).
///
/// The on-prem vision server decodes images with PIL, which CANNOT read HEIC —
/// sending raw iPhone camera bytes mislabelled as `image/jpeg` made it reject
/// the request with `400 "cannot identify image file"`. Rendering every input
/// through a `<canvas>` normalises it to real JPEG and shrinks the payload /
/// vision-token count (cap longest side at `MAX_DIM`).
async fn file_to_jpeg_base64(file: &web_sys::File) -> Result<String, String> {
    const MAX_DIM: f64 = 1536.0;
    let window = web_sys::window().ok_or("no window")?;
    let blob: &web_sys::Blob = file.unchecked_ref();
    let promise = window
        .create_image_bitmap_with_blob(blob)
        .map_err(|e| format!("createImageBitmap: {e:?}"))?;
    let bitmap: web_sys::ImageBitmap = JsFuture::from(promise)
        .await
        .map_err(|e| format!("image decode failed (unsupported format?): {e:?}"))?
        .unchecked_into();
    let (w, h) = (bitmap.width() as f64, bitmap.height() as f64);
    let scale = (MAX_DIM / w.max(h)).min(1.0);
    let cw = (w * scale).round().max(1.0);
    let ch = (h * scale).round().max(1.0);

    let document = window.document().ok_or("no document")?;
    let canvas: web_sys::HtmlCanvasElement = document
        .create_element("canvas")
        .map_err(|e| format!("canvas create: {e:?}"))?
        .unchecked_into();
    canvas.set_width(cw as u32);
    canvas.set_height(ch as u32);
    let ctx: web_sys::CanvasRenderingContext2d = canvas
        .get_context("2d")
        .map_err(|e| format!("2d ctx: {e:?}"))?
        .ok_or("no 2d context")?
        .unchecked_into();
    ctx.draw_image_with_image_bitmap_and_dw_and_dh(&bitmap, 0.0, 0.0, cw, ch)
        .map_err(|e| format!("draw: {e:?}"))?;
    bitmap.close();

    let data_url = canvas
        .to_data_url_with_type_and_encoder_options("image/jpeg", &JsValue::from_f64(0.85))
        .map_err(|e| format!("toDataURL: {e:?}"))?;
    data_url
        .split_once(',')
        .map(|(_, b64)| b64.to_string())
        .ok_or_else(|| "malformed data URL".to_string())
}

#[component]
pub fn FoodEditor(
    custom_nutrients: Signal<Vec<NutrientSpec>>,
    on_draft: Callback<(Food, Option<String>)>,
    /// Pre-fill the name field (e.g. with the search query that led here, so the
    /// user doesn't have to type the name twice).
    #[prop(optional, into)]
    initial_name: String,
) -> impl IntoView {
    let name = create_rw_signal(initial_name);
    let kcal = create_rw_signal(String::new());
    let protein = create_rw_signal(String::new());
    let fat = create_rw_signal(String::new());
    let carbs = create_rw_signal(String::new());
    let custom_values = create_rw_signal(BTreeMap::<String, String>::new());
    let ai_details = create_rw_signal(BTreeMap::<String, AiNutrientDetail>::new());
    // Which nutrient's "?" tooltip is currently open (tap to toggle). `title=`
    // alone only shows on hover, so it's invisible on touch — this drives a
    // tap-revealed popover.
    let open_tip = create_rw_signal(None::<String>);
    let draft_id = create_rw_signal(None::<String>);

    // Active tab: 0 = "by name" (text lookup), 1 = "by photo" (vision).
    let active_tab = create_rw_signal(0u8);

    // TWO independent detection channels so a name lookup and a photo lookup can
    // run at the SAME time: the user starts a name detection, switches to the
    // photo tab without waiting, adds photos and starts a vision detection. Each
    // channel owns its own progress state; both write into the shared nutrient
    // fields below (last completion wins) and into the single draft.
    //
    // phase 0=working (waiting), 1=thinking, 2=answering.
    let name_loading = create_rw_signal(false);
    let name_error = create_rw_signal(None::<String>);
    let name_phase = create_rw_signal(0u8);
    let name_think = create_rw_signal(0u32);
    let name_answer = create_rw_signal(0u32);
    let name_start = create_rw_signal(0f64);
    let name_tick = create_rw_signal(0u32);
    let name_interval = create_rw_signal(None::<i32>);

    let photo_loading = create_rw_signal(false);
    let photo_error = create_rw_signal(None::<String>);
    let photo_phase = create_rw_signal(0u8);
    let photo_think = create_rw_signal(0u32);
    let photo_answer = create_rw_signal(0u32);
    let photo_start = create_rw_signal(0f64);
    let photo_tick = create_rw_signal(0u32);
    let photo_interval = create_rw_signal(None::<i32>);
    // Async vision-queue status line ("in queue: N" / "recognizing…") and the
    // epoch-ms start of the current phase, so we can show seconds since it began.
    let photo_vision_msg = create_rw_signal(String::new());
    let photo_vision_start = create_rw_signal(0f64);

    let photos_base64 = create_rw_signal(Vec::<String>::new());
    let photo_count = create_rw_signal(0usize);

    // Paywall modal: shown (instead of silently navigating away) when recognition
    // is blocked by an inactive subscription — proactively, or on a backend 402.
    let show_paywall = create_rw_signal(false);

    let build_food = move || -> Food {
        // Normalise the decimal separator: mobile keyboards emit ',', so "25,0"
        // must parse as 25.0 (not fail → 0).
        let parse = |s: String| -> f64 { s.replace(',', ".").parse().unwrap_or(0.0) };
        let mut nutrients = BTreeMap::new();
        for (key, val) in custom_values.get_untracked() {
            let num_str: String = val.replace(',', ".").chars().take_while(|c| c.is_ascii_digit() || *c == '.').collect();
            if let Ok(v) = num_str.parse::<f64>() {
                if v != 0.0 {
                    nutrients.insert(key, v);
                }
            }
        }
        Food {
            id: uuid::Uuid::now_v7().to_string(),
            name: name.get_untracked(),
            kcal: parse(kcal.get_untracked()),
            protein: parse(protein.get_untracked()),
            fat: parse(fat.get_untracked()),
            carbs: parse(carbs.get_untracked()),
            nutrients,
            package_weight: None,
            is_recipe: false,
            recipe_id: None,
            archived: false,
            is_restaurant: false,
            is_snack: None, // classified later, in the background, at summary time
            created_at: chrono::Utc::now().to_rfc3339(),
            updated_at: chrono::Utc::now().to_rfc3339(),
        }
    };

    let apply_result = move |result: &AiLookupOutput| {
        if let Some(ref n) = result.name {
            name.set(n.clone());
        }
        kcal.set(format!("{:.1}", result.kcal.recommended.value));
        protein.set(format!("{:.1}", result.protein.recommended.value));
        fat.set(format!("{:.1}", result.fat.recommended.value));
        carbs.set(format!("{:.1}", result.carbs.recommended.value));

        let mut details = BTreeMap::new();
        details.insert("kcal".to_string(), result.kcal.clone());
        details.insert("protein".to_string(), result.protein.clone());
        details.insert("fat".to_string(), result.fat.clone());
        details.insert("carbs".to_string(), result.carbs.clone());

        let mut cv = BTreeMap::new();
        for (nutrient_name, detail) in &result.nutrients {
            cv.insert(nutrient_name.clone(), format!("{:.1}", detail.recommended.value));
            details.insert(nutrient_name.clone(), detail.clone());
        }
        custom_values.set(cv);
        ai_details.set(details);
    };

    let on_file_change = move |ev: leptos::ev::Event| {
        let input: web_sys::HtmlInputElement = ev.target().unwrap().unchecked_into();
        let files = match input.files() {
            Some(f) if f.length() > 0 => f,
            _ => return,
        };
        spawn_local(async move {
            let mut new_imgs = Vec::new();
            for i in 0..files.length() {
                let file = files.get(i).unwrap();
                // Normalise to JPEG (HEIC → JPEG, downscale). On failure, surface
                // it — never silently drop / send an undecodable image.
                match file_to_jpeg_base64(&file).await {
                    Ok(b64) => new_imgs.push(b64),
                    Err(e) => { photo_error.set(Some(e)); }
                }
            }
            // APPEND, don't replace: the camera returns one image per capture, so
            // front + back are added in separate taps. Then clear the input value
            // so picking another photo (even the same file) fires `change` again.
            photos_base64.update(|v| v.extend(new_imgs));
            photo_count.set(photos_base64.get_untracked().len());
            input.set_value("");
        });
    };

    let navigate = use_navigate();

    // Create-or-update the SINGLE draft from the current fields. Both detection
    // channels call this on completion; if a draft already exists (e.g. the
    // other channel finished first) we UPDATE it rather than spawning a second.
    let persist_result = move || {
        let food = build_food();
        let existing = draft_id.get_untracked();
        spawn_local(async move {
            match existing {
                Some(id) => { local::update_draft_fields(&id, &food).await; }
                None => {
                    let draft = local::save_draft(&food).await;
                    draft_id.set(Some(draft.id));
                }
            }
        });
    };

    // One routine drives both channels; `use_vision` selects which progress
    // signals it writes to and which backend path it takes. Wrapped in `Rc` so
    // the two button handlers can each hold a copy.
    let run_ai = Rc::new(move |use_vision: bool| {
        let loading = if use_vision { photo_loading } else { name_loading };
        let error_sig = if use_vision { photo_error } else { name_error };
        let phase = if use_vision { photo_phase } else { name_phase };
        let think = if use_vision { photo_think } else { name_think };
        let answer = if use_vision { photo_answer } else { name_answer };
        let start_sig = if use_vision { photo_start } else { name_start };
        let tick = if use_vision { photo_tick } else { name_tick };
        let interval = if use_vision { photo_interval } else { name_interval };
        // Vision-only status line / phase-start; untouched by the text channel.
        let vision_msg = photo_vision_msg;
        let vision_start = photo_vision_start;

        let images = photos_base64.get_untracked();
        let n = name.get_untracked();
        if use_vision {
            if images.is_empty() { return; }
        } else if n.is_empty() {
            return;
        }

        loading.set(true);
        error_sig.set(None);
        phase.set(0);
        think.set(0);
        answer.set(0);
        tick.set(0);
        if use_vision {
            vision_msg.set(String::new());
            vision_start.set(0.0);
        }
        start_sig.set(js_sys::Date::now());
        // 1s tick to drive the live "Working: Xs" display.
        {
            let win = web_sys::window().unwrap();
            let cb = wasm_bindgen::closure::Closure::<dyn Fn()>::new(move || tick.update(|v| *v += 1));
            if let Ok(id) = win.set_interval_with_callback_and_timeout_and_arguments_0(
                cb.as_ref().unchecked_ref(),
                1000,
            ) {
                interval.set(Some(id));
            }
            cb.forget();
        }
        let nutrients_list = custom_nutrients.get_untracked();
        spawn_local(async move {
            let stop_timer = move || {
                if let Some(id) = interval.get_untracked() {
                    web_sys::window().unwrap().clear_interval_with_handle(id);
                    interval.set(None);
                }
            };
            let finish = move |err: Option<String>| {
                stop_timer();
                // Only clear the vision status line from the vision channel — the
                // text channel must not wipe a photo job's message running in
                // parallel.
                if use_vision { vision_msg.set(String::new()); }
                loading.set(false);
                if let Some(e) = err {
                    // A backend 402 means the subscription lapsed between the
                    // proactive check and the job — explain it in a modal rather
                    // than dumping a raw error or navigating away silently.
                    if e.contains("HTTP 402") { show_paywall.set(true); } else { error_sig.set(Some(e)); }
                }
            };

            // Proactive gate: if the subscription is known to be inactive, show the
            // paywall modal instead of starting a doomed job. (On a network error we
            // proceed and let it fail downstream.)
            if let Ok(s) = subscription::status().await {
                if !s.active {
                    stop_timer();
                    loading.set(false);
                    show_paywall.set(true);
                    return;
                }
            }

            if use_vision {
                // Vision is async: submit, then a 2-state machine — POLL the queue
                // while `queued`, then SWITCH to the SSE STREAM while `processing`.
                let input = AiVisionInput { images, custom_nutrients: nutrients_list };
                // UPLOAD state: the image upload can take a while; show it.
                phase.set(0);
                vision_msg.set(t("food_editor.ai_uploading").to_string());
                let job_id = match ai::submit_vision(&input).await {
                    Ok(id) => id,
                    Err(e) => { finish(Some(e)); return; }
                };

                // State QUEUED: poll for position until processing/done/error
                // (generous cap so a busy queue never shows a false timeout).
                let mut processing = false;
                for _ in 0..600 {
                    match ai::poll_queue(&job_id, &input).await {
                        Ok(ai::QueuePhase::Done(out)) => {
                            apply_result(&out);
                            persist_result();
                            finish(None);
                            return;
                        }
                        Ok(ai::QueuePhase::Error(e)) => { finish(Some(e)); return; }
                        Ok(ai::QueuePhase::Processing { since_ms }) => {
                            if since_ms > 0.0 { vision_start.set(since_ms); }
                            processing = true;
                            break;
                        }
                        Ok(ai::QueuePhase::Queued { position, since_ms }) => {
                            if since_ms > 0.0 { vision_start.set(since_ms); }
                            phase.set(0);
                            vision_msg.set(if position > 0 {
                                format!("{} {}", t("food_editor.ai_queue"), position)
                            } else {
                                t("food_editor.ai_recognizing").to_string()
                            });
                        }
                        Err(_) => {} // transient; keep waiting
                    }
                    ai::sleep_ms(1500).await;
                }
                if !processing {
                    finish(Some(t("food_editor.ai_timeout").to_string()));
                    return;
                }

                // State PROCESSING: stream live LLM phase/tokens. Reuses the same
                // button rendering as text (phase 1 = thinking, 2 = answer).
                vision_msg.set(String::new());
                let on_progress = move |ph: u8, tt: u32, at: u32| match ph {
                    1 => { phase.set(1); think.set(tt); vision_msg.set(String::new()); }
                    2 => { phase.set(2); answer.set(at); vision_msg.set(String::new()); }
                    _ => { phase.set(0); vision_msg.set(t("food_editor.ai_recognizing").to_string()); }
                };
                match ai::stream_vision(&job_id, &input, on_progress).await {
                    Ok(out) => {
                        apply_result(&out);
                        persist_result();
                        finish(None);
                    }
                    Err(e) => finish(Some(e)),
                }
            } else {
                // Text lookup: streaming, blocking await (no queue).
                let on_token = move |ph: ai::AiPhase| match ph {
                    ai::AiPhase::Thinking => {
                        think.update(|v| *v += 1);
                        if phase.get_untracked() == 0 { phase.set(1); }
                    }
                    ai::AiPhase::Answer => {
                        answer.update(|v| *v += 1);
                        if phase.get_untracked() != 2 { phase.set(2); }
                    }
                };
                let input = AiLookupInput { name: n, custom_nutrients: nutrients_list };
                let result = ai::lookup(&input, on_token).await;
                match result {
                    Ok(output) => {
                        apply_result(&output);
                        persist_result();
                        finish(None);
                    }
                    Err(e) => finish(Some(e)),
                }
            }
        });
    });

    let run_name = run_ai.clone();
    let on_detect_name = move |_| run_name(false);
    let run_photo = run_ai.clone();
    let on_detect_photo = move |_| run_photo(true);

    create_effect(move |prev: Option<()>| {
        let _ = name.get();
        let _ = kcal.get();
        let _ = protein.get();
        let _ = fat.get();
        let _ = carbs.get();
        let _ = custom_values.get();
        if prev.is_some() {
            if let Some(id) = draft_id.get_untracked() {
                let food = build_food();
                spawn_local(async move {
                    local::update_draft_fields(&id, &food).await;
                });
            }
        }
    });

    let ai_hint = move |field_key: &str| {
        let key = field_key.to_string();
        view! {
            {move || {
                let details = ai_details.get();
                let key = key.clone();
                details.get(&key).map(|d| {
                    let unit = crate::services::i18n::unit_label(&d.recommended.unit);
                    let tip = format!(
                        "{:.1}–{:.1} {} ({}: {:.1})\n{}",
                        d.min_value.value, d.max_value.value, unit,
                        t("food_editor.recommended_abbr"), d.recommended.value, d.comment,
                    );
                    let key_click = key.clone();
                    let key_open = key.clone();
                    view! {
                        <span style="position: relative; display: inline-block;">
                            <button
                                type="button"
                                class="has-text-link is-size-7"
                                style="margin-left: 4px; cursor: pointer; text-decoration: underline; border: none; background: none; padding: 0; font: inherit; -webkit-appearance: none; appearance: none;"
                                title=tip.clone()
                                on:click=move |ev| {
                                    ev.stop_propagation();
                                    open_tip.update(|o| {
                                        if o.as_deref() == Some(key_click.as_str()) { *o = None; }
                                        else { *o = Some(key_click.clone()); }
                                    });
                                }
                            >"?"</button>
                            <Show when=move || open_tip.get().as_deref() == Some(key_open.as_str())>
                                <div
                                    class="is-size-7"
                                    style="position: absolute; z-index: 50; top: 1.4rem; left: 0; min-width: 12rem; max-width: 16rem; background: var(--bulma-scheme-main); color: var(--bulma-text); border: 1px solid var(--bulma-border); border-radius: 8px; box-shadow: 0 4px 16px rgba(0,0,0,0.2); padding: 8px 10px; white-space: pre-wrap; line-height: 1.4; text-align: left;"
                                    on:click=move |_| open_tip.set(None)
                                >{tip.clone()}</div>
                            </Show>
                        </span>
                    }
                })
            }}
        }
    };

    // Seconds since the current phase started (queue phase start for vision,
    // button-press for text). Reads ai_tick so it re-renders every second.
    let name_elapsed = move || -> u32 {
        name_tick.get();
        ((js_sys::Date::now() - name_start.get()) / 1000.0).max(0.0) as u32
    };
    let photo_elapsed = move || -> u32 {
        photo_tick.get();
        let start = if photo_vision_start.get() > 0.0 { photo_vision_start.get() } else { photo_start.get() };
        ((js_sys::Date::now() - start) / 1000.0).max(0.0) as u32
    };

    view! {
        <div on:keydown=move |ev: leptos::ev::KeyboardEvent| {
            if ev.key() == "Enter" { ev.prevent_default(); }
        }>
            // Two independent sub-forms behind underline tabs. Switching only
            // toggles visibility (display: none) — the DOM stays mounted and the
            // signals live on the component, so every field, photo and in-flight
            // detection is preserved across switches.
            //
            // Tabs are <button>, NOT Bulma's <a>: Leptos delegates click at the
            // document root, and iOS Safari only bubbles clicks to document from
            // natively-interactive elements (or ones with cursor:pointer). A
            // href-less <a> is not interactive on iOS, so it highlighted on tap
            // but the delegated on:click never fired — same bug the bottom nav hit
            // (fixed there with real <a href> links). A <button> is natively
            // clickable, matching the working "Форма/История" segmented control.
            <div style="display: flex; border-bottom: 1px solid var(--bulma-border); margin-bottom: 12px;">
                {[(0u8, "food_editor.tab_by_name"), (1u8, "food_editor.tab_by_photo")]
                    .into_iter()
                    .map(|(idx, label)| view! {
                        <button type="button"
                            style=move || format!(
                                "flex: 1; background: none; border: none; border-bottom: 2px solid {}; \
                                 margin-bottom: -1px; padding: 8px 0; cursor: pointer; font: inherit; \
                                 font-size: 0.875rem; {}",
                                if active_tab.get() == idx { "var(--bulma-link)" } else { "transparent" },
                                if active_tab.get() == idx {
                                    "color: var(--bulma-link); font-weight: 600;"
                                } else {
                                    "color: var(--bulma-text-weak);"
                                },
                            )
                            on:click=move |_| active_tab.set(idx)
                        >
                            {move || t(label)}
                        </button>
                    })
                    .collect_view()}
            </div>

            // Tab 1 — "By name": name field + detect-from-name button.
            <div style=move || if active_tab.get() == 0 { "" } else { "display: none;" }>
                <input type="text"
                    placeholder=t("food_editor.product_name")
                    class="is-size-6"
                    style="width: 100%; padding: 8px 12px; border: 1px solid var(--bulma-border); border-radius: 10px; background: var(--bulma-scheme-main); color: var(--bulma-text); outline: none; box-sizing: border-box; margin-bottom: 10px;"
                    prop:value=move || name.get()
                    on:input=move |ev| {
                        // Keep `draft_id` so the auto-sync effect propagates the new
                        // name into BOTH the draft and the Food created from it. (We
                        // used to clear it here, which orphaned the draft with the old
                        // name and left its Food un-renamed.)
                        name.set(event_target_value(&ev));
                    }
                />
                <button type="button"
                    class="button is-link is-fullwidth is-size-7"
                    style="padding: 8px 0; border: none; border-radius: 10px; cursor: pointer;"
                    disabled=move || name_loading.get() || name.get().is_empty()
                    on:click=on_detect_name
                >
                    {move || if name_loading.get() {
                        match name_phase.get() {
                            0 => format!("\u{231b} {}s", name_elapsed()),
                            1 => format!("\u{1f9e0} Thinking ({} tok) \u{00b7} {}s", name_think.get(), name_elapsed()),
                            _ => format!("\u{270d}\u{fe0f} Answer ({} tok) \u{00b7} {}s", name_answer.get(), name_elapsed()),
                        }
                    } else {
                        format!("\u{2728} {}", t("food_editor.detect_by_name"))
                    }}
                </button>
                {move || name_error.get().map(|e| view! {
                    <div class="has-text-danger is-size-7" style="padding: 8px 12px; margin-top: 10px; background: var(--bulma-danger-light); border-radius: 10px;">
                        {e}
                    </div>
                })}
            </div>

            // Tab 2 — "By photo": thumbnails + add-photo + detect-calories button.
            <div style=move || if active_tab.get() == 1 { "" } else { "display: none;" }>
                // Thumbnails of the photos already added (tap × to drop one). Photos
                // are stored as JPEG base64, so they render directly as data URLs.
                {move || {
                    let photos = photos_base64.get();
                    (!photos.is_empty()).then(|| view! {
                        <div style="display: flex; gap: 8px; flex-wrap: wrap; margin-bottom: 8px;">
                            {photos.into_iter().enumerate().map(|(i, b64)| view! {
                                <div style="position: relative; width: 56px; height: 56px;">
                                    <img src=format!("data:image/jpeg;base64,{b64}")
                                        style="width: 56px; height: 56px; object-fit: cover; border-radius: 8px; border: 1px solid var(--bulma-border-weak);" />
                                    <button type="button"
                                        style="position: absolute; top: -6px; right: -6px; width: 20px; height: 20px; padding: 0; line-height: 1; border: none; border-radius: 50%; background: var(--bulma-danger); color: var(--bulma-danger-invert); font-size: 13px; cursor: pointer;"
                                        on:click=move |_| {
                                            photos_base64.update(|v| { if i < v.len() { v.remove(i); } });
                                            photo_count.set(photos_base64.get_untracked().len());
                                        }
                                    >"\u{00d7}"</button>
                                </div>
                            }).collect_view()}
                        </div>
                    })
                }}

                <div style="display: flex; flex-direction: column; gap: 8px; margin-bottom: 12px;">
                    <input type="file" accept="image/*" multiple=true
                        id="food-photo-input"
                        style="display: none;"
                        on:change=on_file_change />
                    <button type="button"
                        class="is-size-7"
                        style="flex: 1; padding: 8px 0; border: 1px solid var(--bulma-border-weak); border-radius: 10px; background: var(--bulma-scheme-main); color: var(--bulma-text); cursor: pointer;"
                        on:click=move |_| {
                            let doc = web_sys::window().unwrap().document().unwrap();
                            let el = doc.get_element_by_id("food-photo-input").unwrap();
                            use wasm_bindgen::JsCast;
                            let input: &web_sys::HtmlInputElement = el.unchecked_ref();
                            input.click();
                        }
                    >
                        {move || {
                            // Label flips to "add another photo" once at least one is added.
                            let key = if photo_count.get() == 0 {
                                "food_editor.add_photo"
                            } else {
                                "food_editor.add_more_photo"
                            };
                            format!("\u{1f4f7} {}", t(key))
                        }}
                    </button>
                    <button type="button"
                        class="button is-link is-size-7"
                        style="flex: 1; padding: 8px 0; border: none; border-radius: 10px; cursor: pointer;"
                        disabled=move || photo_loading.get() || photo_count.get() == 0
                        on:click=on_detect_photo
                    >
                        {move || if photo_loading.get() {
                            match photo_phase.get() {
                                0 => {
                                    let msg = photo_vision_msg.get();
                                    if msg.is_empty() {
                                        format!("\u{231b} {}s", photo_elapsed())
                                    } else {
                                        format!("\u{231b} {msg} \u{00b7} {}s", photo_elapsed())
                                    }
                                }
                                1 => format!("\u{1f9e0} Thinking ({} tok) \u{00b7} {}s", photo_think.get(), photo_elapsed()),
                                _ => format!("\u{270d}\u{fe0f} Answer ({} tok) \u{00b7} {}s", photo_answer.get(), photo_elapsed()),
                            }
                        } else {
                            format!("\u{2728} {}", t("food_editor.detect_by_photo"))
                        }}
                    </button>
                </div>

                <p class="is-size-7 has-text-grey" style="margin: -4px 0 12px 0;">
                    {move || t("food_editor.photo_hint")}
                </p>

                {move || photo_error.get().map(|e| view! {
                    <div class="has-text-danger is-size-7" style="padding: 8px 12px; margin-bottom: 10px; background: var(--bulma-danger-light); border-radius: 10px;">
                        {e}
                    </div>
                })}
            </div>

            // Nutrient fields card. NB: no `overflow: hidden` — it would clip the
            // "?" hint popover that floats below the lower rows. The rounded look is
            // kept by making the card itself the rounded surface (scheme-main) with
            // transparent rows, rather than clipping opaque rows to the radius.
            <div style="background: var(--bulma-scheme-main); border-radius: 12px;">
                <NutrientRow label=t("food_editor.calories") unit=t("common.unit.kcal") placeholder="165"
                    value=kcal hint=ai_hint("kcal").into_view() last=false />
                <NutrientRow label=t("food_editor.protein") unit=t("common.unit.g") placeholder="31"
                    value=protein hint=ai_hint("protein").into_view() last=false />
                <NutrientRow label=t("food_editor.fat") unit=t("common.unit.g") placeholder="3.6"
                    value=fat hint=ai_hint("fat").into_view() last=false />
                <NutrientRow label=t("food_editor.carbs") unit=t("common.unit.g") placeholder="0"
                    value=carbs hint=ai_hint("carbs").into_view()
                    last=Signal::derive(move || custom_nutrients.get().is_empty()) />
                <For
                    each=move || custom_nutrients.get()
                    key=|s| s.key.clone()
                    children=move |spec| {
                        let key = spec.name.clone();
                        let key2 = spec.name.clone();
                        let hint_key = spec.name.clone();
                        let unit = spec.unit_label.clone();
                        let sig = create_rw_signal(
                            custom_values.get_untracked().get(&key).cloned().unwrap_or_default()
                        );
                        create_effect(move |_| {
                            let val = sig.get();
                            let k = key2.clone();
                            custom_values.update(|m| { m.insert(k, val); });
                        });
                        view! {
                            <NutrientRow label=spec.name.leak() unit=unit.leak() placeholder="0"
                                value=sig hint=ai_hint(&hint_key).into_view() last=true />
                        }
                    }
                />
            </div>

            // Add button
            <button type="button"
                class="button is-link is-size-6 has-text-weight-semibold"
                style="width: 100%; padding: 12px 0; margin-top: 16px; border: none; border-radius: 10px; cursor: pointer;"
                disabled=move || name.get().is_empty()
                on:click=move |_| on_draft.call((build_food(), draft_id.get_untracked()))
            >
                {move || t("food_editor.add")}
            </button>

            // Paywall modal — recognition blocked by an inactive subscription.
            // "Оплатить подписку" routes to the subscription management page (its
            // own subscribe action leads to checkout); "Не сейчас" just dismisses.
            <Show when=move || show_paywall.get()>
                <div class="modal is-active">
                    <div class="modal-background" on:click=move |_| show_paywall.set(false)></div>
                    <div class="modal-card" style="max-width: 22rem; margin: 0 1rem;">
                        <section class="modal-card-body" style="border-radius: 12px; text-align: center;">
                            <p class="is-size-5 has-text-weight-semibold mb-2">
                                {move || t("food_editor.paywall_title")}
                            </p>
                            <p class="mb-4" style="color: var(--bulma-text-weak);">{move || t("food_editor.paywall_body")}</p>
                            <button type="button"
                                class="button is-link is-fullwidth has-text-weight-semibold mb-2"
                                on:click={
                                    let navigate = navigate.clone();
                                    move |_| {
                                        show_paywall.set(false);
                                        navigate("/settings/subscription", Default::default());
                                    }
                                }
                            >
                                {move || t("food_editor.paywall_pay")}
                            </button>
                            <button type="button" class="button is-text is-fullwidth"
                                on:click=move |_| show_paywall.set(false)
                            >
                                {move || t("food_editor.paywall_dismiss")}
                            </button>
                        </section>
                    </div>
                </div>
            </Show>
        </div>
    }
}

#[component]
fn NutrientRow(
    label: &'static str,
    unit: &'static str,
    placeholder: &'static str,
    value: RwSignal<String>,
    hint: View,
    #[prop(into)] last: MaybeSignal<bool>,
) -> impl IntoView {
    view! {
        <div>
            <div style="display: flex; align-items: center; padding: 10px 12px;">
                <span class="is-size-6" style="color: var(--bulma-text); min-width: 80px;">
                    {label}
                </span>
                {hint}
                <div style="flex: 1;"></div>
                <input type="text" inputmode="decimal"
                    placeholder=placeholder
                    class="is-size-6"
                    style="width: 80px; text-align: right; padding: 4px 8px; border: none; background: var(--bulma-background); color: var(--bulma-text); border-radius: 8px; outline: none;"
                    prop:value=move || value.get()
                    on:input=move |ev| value.set(event_target_value(&ev))
                />
                <span class="has-text-grey-light is-size-7" style="margin-left: 6px; min-width: 30px;">
                    {unit}
                </span>
            </div>
            <Show when=move || !last.get()>
                <div style="border-bottom: 0.5px solid var(--bulma-border-weak); margin-left: 12px;"></div>
            </Show>
        </div>
    }
}
