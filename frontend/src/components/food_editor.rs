use std::collections::BTreeMap;

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
    let ai_loading = create_rw_signal(false);
    let ai_error = create_rw_signal(None::<String>);
    let ai_details = create_rw_signal(BTreeMap::<String, AiNutrientDetail>::new());
    // Which nutrient's "?" tooltip is currently open (tap to toggle). `title=`
    // alone only shows on hover, so it's invisible on touch — this drives a
    // tap-revealed popover.
    let open_tip = create_rw_signal(None::<String>);
    let draft_id = create_rw_signal(None::<String>);

    // AI lookup progress: phase 0=working (waiting), 1=thinking, 2=answering.
    let ai_phase = create_rw_signal(0u8);
    let ai_think = create_rw_signal(0u32);
    let ai_answer = create_rw_signal(0u32);
    let ai_start = create_rw_signal(0f64);
    let ai_tick = create_rw_signal(0u32);
    let ai_interval = create_rw_signal(None::<i32>);
    // Async vision-queue status line ("in queue: N" / "recognizing…") and the
    // epoch-ms start of the current phase, so we can show seconds since it began.
    let ai_vision_msg = create_rw_signal(String::new());
    let ai_vision_start = create_rw_signal(0f64);

    let photos_base64 = create_rw_signal(Vec::<String>::new());
    let photo_count = create_rw_signal(0usize);

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
                    Err(e) => { ai_error.set(Some(e)); }
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

    let on_ai = move |_| {
        let images = photos_base64.get_untracked();
        let n = name.get_untracked();
        if images.is_empty() && n.is_empty() { return; }
        let navigate = navigate.clone();
        ai_loading.set(true);
        ai_error.set(None);
        ai_phase.set(0);
        ai_think.set(0);
        ai_answer.set(0);
        ai_tick.set(0);
        ai_vision_msg.set(String::new());
        ai_vision_start.set(0.0);
        ai_start.set(js_sys::Date::now());
        // 1s tick to drive the live "Working: Xs" display.
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
        let nutrients_list = custom_nutrients.get_untracked();
        spawn_local(async move {
            let stop_timer = move || {
                if let Some(id) = ai_interval.get_untracked() {
                    web_sys::window().unwrap().clear_interval_with_handle(id);
                    ai_interval.set(None);
                }
            };
            let finish = move |err: Option<String>, nav: &dyn Fn(&str)| {
                stop_timer();
                ai_vision_msg.set(String::new());
                ai_loading.set(false);
                if let Some(e) = err {
                    if e.contains("HTTP 402") { nav("/settings/subscription"); } else { ai_error.set(Some(e)); }
                }
            };

            // Proactive gate: if the subscription is known to be inactive, send
            // the user to the paywall instead of starting a doomed job. (On a
            // network error we proceed and let it fail downstream.)
            if let Ok(s) = subscription::status().await {
                if !s.active {
                    stop_timer();
                    ai_loading.set(false);
                    navigate("/settings/subscription", Default::default());
                    return;
                }
            }

            if !images.is_empty() {
                // Vision is async: submit, then a 2-state machine — POLL the queue
                // while `queued`, then SWITCH to the SSE STREAM while `processing`.
                let input = AiVisionInput { images, custom_nutrients: nutrients_list };
                // UPLOAD state: the image upload can take a while; show it.
                ai_phase.set(0);
                ai_vision_msg.set(t("food_editor.ai_uploading").to_string());
                let job_id = match ai::submit_vision(&input).await {
                    Ok(id) => id,
                    Err(e) => { finish(Some(e), &|p| { navigate(p, Default::default()); }); return; }
                };

                // State QUEUED: poll for position until processing/done/error
                // (generous cap so a busy queue never shows a false timeout).
                let mut processing = false;
                for _ in 0..600 {
                    match ai::poll_queue(&job_id, &input).await {
                        Ok(ai::QueuePhase::Done(out)) => {
                            apply_result(&out);
                            let draft = local::save_draft(&build_food()).await;
                            draft_id.set(Some(draft.id));
                            finish(None, &|p| { navigate(p, Default::default()); });
                            return;
                        }
                        Ok(ai::QueuePhase::Error(e)) => { finish(Some(e), &|p| { navigate(p, Default::default()); }); return; }
                        Ok(ai::QueuePhase::Processing { since_ms }) => {
                            if since_ms > 0.0 { ai_vision_start.set(since_ms); }
                            processing = true;
                            break;
                        }
                        Ok(ai::QueuePhase::Queued { position, since_ms }) => {
                            if since_ms > 0.0 { ai_vision_start.set(since_ms); }
                            ai_phase.set(0);
                            ai_vision_msg.set(if position > 0 {
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
                    finish(Some(t("food_editor.ai_timeout").to_string()), &|p| { navigate(p, Default::default()); });
                    return;
                }

                // State PROCESSING: stream live LLM phase/tokens. Reuses the same
                // button rendering as text (phase 1 = thinking, 2 = answer).
                ai_vision_msg.set(String::new());
                let on_progress = move |phase: u8, tt: u32, at: u32| match phase {
                    1 => { ai_phase.set(1); ai_think.set(tt); ai_vision_msg.set(String::new()); }
                    2 => { ai_phase.set(2); ai_answer.set(at); ai_vision_msg.set(String::new()); }
                    _ => { ai_phase.set(0); ai_vision_msg.set(t("food_editor.ai_recognizing").to_string()); }
                };
                match ai::stream_vision(&job_id, &input, on_progress).await {
                    Ok(out) => {
                        apply_result(&out);
                        let draft = local::save_draft(&build_food()).await;
                        draft_id.set(Some(draft.id));
                        finish(None, &|p| { navigate(p, Default::default()); });
                    }
                    Err(e) => finish(Some(e), &|p| { navigate(p, Default::default()); }),
                }
            } else {
                // Text lookup: streaming, blocking await (no queue).
                let on_token = move |phase: ai::AiPhase| match phase {
                    ai::AiPhase::Thinking => {
                        ai_think.update(|v| *v += 1);
                        if ai_phase.get_untracked() == 0 { ai_phase.set(1); }
                    }
                    ai::AiPhase::Answer => {
                        ai_answer.update(|v| *v += 1);
                        if ai_phase.get_untracked() != 2 { ai_phase.set(2); }
                    }
                };
                let input = AiLookupInput { name: n, custom_nutrients: nutrients_list };
                let result = ai::lookup(&input, on_token).await;
                match result {
                    Ok(output) => {
                        apply_result(&output);
                        let draft = local::save_draft(&build_food()).await;
                        draft_id.set(Some(draft.id));
                        finish(None, &|p| { navigate(p, Default::default()); });
                    }
                    Err(e) => finish(Some(e), &|p| { navigate(p, Default::default()); }),
                }
            }
        });
    };

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
    let elapsed = move || -> u32 {
        ai_tick.get();
        let start = if ai_vision_start.get() > 0.0 { ai_vision_start.get() } else { ai_start.get() };
        ((js_sys::Date::now() - start) / 1000.0).max(0.0) as u32
    };

    view! {
        <div on:keydown=move |ev: leptos::ev::KeyboardEvent| {
            if ev.key() == "Enter" { ev.prevent_default(); }
        }>
            // Name input
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

            // Photo + AI buttons — stacked in a column.
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
                    disabled=move || ai_loading.get() || (name.get().is_empty() && photo_count.get() == 0)
                    on:click=on_ai
                >
                    {move || if ai_loading.get() {
                        match ai_phase.get() {
                            0 => {
                                let msg = ai_vision_msg.get();
                                if msg.is_empty() {
                                    format!("\u{231b} {}s", elapsed())
                                } else {
                                    format!("\u{231b} {msg} \u{00b7} {}s", elapsed())
                                }
                            }
                            1 => format!("\u{1f9e0} Thinking ({} tok) \u{00b7} {}s", ai_think.get(), elapsed()),
                            _ => format!("\u{270d}\u{fe0f} Answer ({} tok) \u{00b7} {}s", ai_answer.get(), elapsed()),
                        }
                    } else {
                        format!("\u{2728} {}", t("food_editor.fill_info"))
                    }}
                </button>
            </div>

            <p class="is-size-7 has-text-grey" style="margin: -4px 0 12px 0;">
                {move || t("food_editor.photo_hint")}
            </p>

            {move || ai_error.get().map(|e| view! {
                <div class="has-text-danger is-size-7" style="padding: 8px 12px; margin-bottom: 10px; background: var(--bulma-danger-light); border-radius: 10px;">
                    {e}
                </div>
            })}

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
