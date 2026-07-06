use leptos::*;
use wasm_bindgen::prelude::*;
use wasm_bindgen::JsCast;
use wasm_bindgen_futures::JsFuture;
use web_sys::{HtmlCanvasElement, HtmlVideoElement, MediaStream, MediaStreamConstraints};

use crate::services::i18n::t;

// ── BarcodeDetector binding (Chrome/Edge only) ──

#[wasm_bindgen]
extern "C" {
    #[wasm_bindgen(js_name = BarcodeDetector)]
    type BarcodeDetector;

    #[wasm_bindgen(constructor, js_class = "BarcodeDetector", catch)]
    fn new(options: &JsValue) -> Result<BarcodeDetector, JsValue>;

    #[wasm_bindgen(method, js_class = "BarcodeDetector", catch)]
    fn detect(this: &BarcodeDetector, source: &JsValue) -> Result<js_sys::Promise, JsValue>;
}

fn barcode_detector_available() -> bool {
    let global = js_sys::global();
    js_sys::Reflect::get(&global, &JsValue::from_str("BarcodeDetector"))
        .map(|v| !v.is_undefined())
        .unwrap_or(false)
}

fn create_barcode_detector() -> Result<BarcodeDetector, JsValue> {
    let formats = js_sys::Array::new();
    formats.push(&JsValue::from_str("qr_code"));
    let opts = js_sys::Object::new();
    js_sys::Reflect::set(&opts, &JsValue::from_str("formats"), &formats)?;
    BarcodeDetector::new(&opts.into())
}

// ── rqrr fallback decoder ──

fn decode_qr_from_grayscale(data: &[u8], width: u32, height: u32) -> Option<String> {
    let mut img = rqrr::PreparedImage::prepare_from_greyscale(
        width as usize,
        height as usize,
        |x, y| data[y * width as usize + x],
    );
    let grids = img.detect_grids();
    for grid in grids {
        if let Ok((_, content)) = grid.decode() {
            return Some(content);
        }
    }
    None
}

fn rgba_to_grayscale(rgba: &[u8], width: u32, height: u32) -> Vec<u8> {
    let len = (width * height) as usize;
    let mut gray = Vec::with_capacity(len);
    for i in 0..len {
        let r = rgba[i * 4] as u32;
        let g = rgba[i * 4 + 1] as u32;
        let b = rgba[i * 4 + 2] as u32;
        gray.push(((r * 299 + g * 587 + b * 114) / 1000) as u8);
    }
    gray
}

// ── Component ──

#[component]
pub fn QrScanner(on_scan: Callback<String>, on_close: Callback<()>) -> impl IntoView {
    let video_ref = create_node_ref::<leptos::html::Video>();
    let error_msg = create_rw_signal(Option::<String>::None);
    let stream_signal = create_rw_signal(Option::<MediaStream>::None);
    let scanning = create_rw_signal(true);

    let stop_stream = move || {
        scanning.set(false);
        if let Some(stream) = stream_signal.get_untracked() {
            let tracks = stream.get_tracks();
            for i in 0..tracks.length() {
                if let Some(track) = tracks.get(i).dyn_ref::<web_sys::MediaStreamTrack>() {
                    track.stop();
                }
            }
        }
        stream_signal.set(None);
    };

    let close = move |_| {
        stop_stream();
        on_close.call(());
    };

    create_effect(move |_| {
        spawn_local(async move {
            if let Err(e) = start_scanning(video_ref, stream_signal, scanning, on_scan).await {
                let raw = e.as_string()
                    .unwrap_or_else(|| format!("{:?}", e))
                    .to_lowercase();
                let msg = if raw.contains("not found") || raw.contains("notfounderror") {
                    t("qr.no_camera").to_string()
                } else if raw.contains("not allowed") || raw.contains("permission") {
                    t("qr.permission_denied").to_string()
                } else {
                    t("qr.camera_error").to_string()
                };
                error_msg.set(Some(msg));
            }
        });
    });

    on_cleanup(move || {
        stop_stream();
    });

    view! {
        <div class="modal is-active">
            <div class="modal-background" on:click=close></div>
            <div class="modal-card" style="max-width: 28rem;">
                <header class="modal-card-head">
                    <p class="modal-card-title is-size-6">{move || t("pair.scan_qr")}</p>
                    <button attr:data-testid="qr-scanner-btn-close" class="delete" on:click=close></button>
                </header>
                <section class="modal-card-body" style="padding: 0; position: relative;">
                    {move || error_msg.get().map(|msg| view! {
                        <div class="notification is-danger m-3">{msg}</div>
                    })}
                    <video
                        node_ref=video_ref
                        autoplay
                        playsinline
                        muted
                        style="width: 100%; display: block;"
                    ></video>
                </section>
                <footer class="modal-card-foot" style="justify-content: space-between;">
                    <button attr:data-testid="qr-scanner-btn-cancel" class="button" on:click=close>{move || t("common.cancel")}</button>
                    <button
                        attr:data-testid="qr-scanner-btn-paste"
                        class="button"
                        on:click=move |_| {
                            let on_scan = on_scan.clone();
                            spawn_local(async move {
                                if let Ok(text) = paste_from_clipboard().await {
                                    if !text.is_empty() {
                                        on_scan.call(text);
                                    }
                                }
                            });
                        }
                    >{move || t("qr.paste_link")}</button>
                </footer>
            </div>
        </div>
    }
}

async fn start_scanning(
    video_ref: NodeRef<leptos::html::Video>,
    stream_signal: RwSignal<Option<MediaStream>>,
    scanning: RwSignal<bool>,
    on_scan: Callback<String>,
) -> Result<(), JsValue> {
    let window = web_sys::window().expect("no window");
    let navigator = window.navigator();
    let media_devices = navigator
        .media_devices()
        .map_err(|_| JsValue::from_str("NotFoundError"))?;

    let constraints = MediaStreamConstraints::new();
    let video_constraints = js_sys::Object::new();
    let facing_mode = js_sys::Object::new();
    js_sys::Reflect::set(&facing_mode, &"ideal".into(), &"environment".into())?;
    js_sys::Reflect::set(&video_constraints, &"facingMode".into(), &facing_mode)?;
    constraints.set_video(&video_constraints.into());
    constraints.set_audio(&JsValue::FALSE);

    let stream_promise = media_devices.get_user_media_with_constraints(&constraints)?;
    let stream_js = JsFuture::from(stream_promise).await?;
    let stream: MediaStream = stream_js.unchecked_into();
    stream_signal.set(Some(stream.clone()));

    let video_el: HtmlVideoElement = loop {
        if let Some(el) = video_ref.get_untracked() {
            let html_el: &web_sys::HtmlElement = &el;
            break html_el.clone().unchecked_into::<HtmlVideoElement>();
        }
        sleep_ms(50).await;
    };

    video_el.set_src_object(Some(&stream));
    let play_promise = video_el.play().map_err(|e| JsValue::from_str(&format!("{:?}", e)))?;
    JsFuture::from(play_promise).await?;

    loop {
        if video_el.video_width() > 0 && video_el.video_height() > 0 {
            break;
        }
        sleep_ms(100).await;
    }

    let use_barcode_api = barcode_detector_available();
    let detector = if use_barcode_api {
        Some(create_barcode_detector()?)
    } else {
        None
    };

    // Create offscreen canvas for rqrr fallback
    let document = window.document().expect("no document");
    let canvas: HtmlCanvasElement = document
        .create_element("canvas")
        .map_err(|e| JsValue::from_str(&format!("{:?}", e)))?
        .unchecked_into();

    loop {
        if !scanning.get_untracked() {
            break;
        }

        let result = if let Some(ref det) = detector {
            detect_barcode_api(det, &video_el).await
        } else {
            detect_rqrr(&video_el, &canvas)
        };

        match result {
            Ok(Some(value)) => {
                scanning.set(false);
                if let Some(s) = stream_signal.get_untracked() {
                    let tracks = s.get_tracks();
                    for i in 0..tracks.length() {
                        if let Some(track) = tracks.get(i).dyn_ref::<web_sys::MediaStreamTrack>() {
                            track.stop();
                        }
                    }
                }
                stream_signal.set(None);
                on_scan.call(value);
                return Ok(());
            }
            Ok(None) => {}
            Err(_) => {}
        }

        sleep_ms(200).await;
    }

    Ok(())
}

async fn detect_barcode_api(detector: &BarcodeDetector, video: &HtmlVideoElement) -> Result<Option<String>, JsValue> {
    let promise = detector.detect(&video.into())?;
    let result = JsFuture::from(promise).await?;
    let barcodes: js_sys::Array = result.unchecked_into();

    if barcodes.length() == 0 {
        return Ok(None);
    }

    let first = barcodes.get(0);
    let raw_value = js_sys::Reflect::get(&first, &"rawValue".into())?;
    match raw_value.as_string() {
        Some(s) if !s.is_empty() => Ok(Some(s)),
        _ => Ok(None),
    }
}

fn detect_rqrr(video: &HtmlVideoElement, canvas: &HtmlCanvasElement) -> Result<Option<String>, JsValue> {
    let w = video.video_width();
    let h = video.video_height();
    if w == 0 || h == 0 {
        return Ok(None);
    }

    canvas.set_width(w);
    canvas.set_height(h);

    let ctx = canvas
        .get_context("2d")
        .map_err(|e| JsValue::from_str(&format!("{:?}", e)))?
        .ok_or_else(|| JsValue::from_str("no 2d context"))?
        .unchecked_into::<web_sys::CanvasRenderingContext2d>();

    ctx.draw_image_with_html_video_element(video, 0.0, 0.0)
        .map_err(|e| JsValue::from_str(&format!("{:?}", e)))?;

    let image_data = ctx
        .get_image_data(0.0, 0.0, w as f64, h as f64)
        .map_err(|e| JsValue::from_str(&format!("{:?}", e)))?;

    let rgba = image_data.data().to_vec();
    let gray = rgba_to_grayscale(&rgba, w, h);

    Ok(decode_qr_from_grayscale(&gray, w, h))
}

async fn paste_from_clipboard() -> Result<String, JsValue> {
    let window = web_sys::window().expect("no window");
    let clipboard = window.navigator().clipboard();
    let promise = clipboard.read_text();
    let val = JsFuture::from(promise).await?;
    Ok(val.as_string().unwrap_or_default())
}

async fn sleep_ms(ms: u32) {
    let promise = js_sys::Promise::new(&mut |resolve, _| {
        let window = web_sys::window().expect("no window");
        let _ = window.set_timeout_with_callback_and_timeout_and_arguments_0(&resolve, ms as i32);
    });
    let _ = JsFuture::from(promise).await;
}
