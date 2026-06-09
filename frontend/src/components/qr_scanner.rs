use leptos::*;
use wasm_bindgen::prelude::*;
use wasm_bindgen::JsCast;
use wasm_bindgen_futures::JsFuture;
use web_sys::{HtmlVideoElement, MediaStream, MediaStreamConstraints};

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
        if !barcode_detector_available() {
            error_msg.set(Some(
                "BarcodeDetector API is not available in this browser. Try Chrome or Edge on Android/desktop."
                    .to_string(),
            ));
            return;
        }

        spawn_local(async move {
            if let Err(e) = start_scanning(video_ref, stream_signal, scanning, on_scan)
                .await
            {
                let msg = e
                    .as_string()
                    .unwrap_or_else(|| format!("{:?}", e));
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
                    <p class="modal-card-title is-size-6">"Scan QR Code"</p>
                    <button class="delete" on:click=close></button>
                </header>
                <section class="modal-card-body" style="padding: 0; position: relative;">
                    {move || error_msg.get().map(|msg| view! {
                        <div class="notification is-danger m-3">
                            {msg}
                        </div>
                    })}
                    <video
                        node_ref=video_ref
                        autoplay
                        playsinline
                        muted
                        style="width: 100%; display: block;"
                    ></video>
                </section>
                <footer class="modal-card-foot">
                    <button class="button" on:click=close>"Close"</button>
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
        .map_err(|e| JsValue::from_str(&format!("No media devices: {:?}", e)))?;

    let constraints = MediaStreamConstraints::new();
    let video_constraints = js_sys::Object::new();
    let facing_mode = js_sys::Object::new();
    js_sys::Reflect::set(
        &facing_mode,
        &JsValue::from_str("ideal"),
        &JsValue::from_str("environment"),
    )?;
    js_sys::Reflect::set(
        &video_constraints,
        &JsValue::from_str("facingMode"),
        &facing_mode,
    )?;
    constraints.set_video(&video_constraints.into());
    constraints.set_audio(&JsValue::FALSE);

    let stream_promise = media_devices.get_user_media_with_constraints(&constraints)?;
    let stream_js = JsFuture::from(stream_promise).await?;
    let stream: MediaStream = stream_js.unchecked_into();

    stream_signal.set(Some(stream.clone()));

    // Wait for video element to be available
    let video_el: HtmlVideoElement = loop {
        if let Some(el) = video_ref.get_untracked() {
            let html_el: &web_sys::HtmlElement = &el;
            break html_el.clone().unchecked_into::<HtmlVideoElement>();
        }
        gloo_timers_sleep(50).await;
    };

    video_el.set_src_object(Some(&stream));

    // Wait for video to start playing
    let play_promise = video_el.play().map_err(|e| {
        JsValue::from_str(&format!("Failed to play video: {:?}", e))
    })?;
    JsFuture::from(play_promise).await?;

    // Wait for video dimensions to be known
    loop {
        if video_el.video_width() > 0 && video_el.video_height() > 0 {
            break;
        }
        gloo_timers_sleep(100).await;
    }

    let detector = create_barcode_detector()?;

    // Scan loop
    loop {
        if !scanning.get_untracked() {
            break;
        }

        match detect_from_video(&detector, &video_el).await {
            Ok(Some(value)) => {
                scanning.set(false);
                // Stop stream before calling back
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
            Err(_) => {
                // Detection can fail transiently; just retry
            }
        }

        gloo_timers_sleep(150).await;
    }

    Ok(())
}

async fn detect_from_video(
    detector: &BarcodeDetector,
    video: &HtmlVideoElement,
) -> Result<Option<String>, JsValue> {
    let promise = detector.detect(&video.into())?;
    let result = JsFuture::from(promise).await?;
    let barcodes: js_sys::Array = result.unchecked_into();

    if barcodes.length() == 0 {
        return Ok(None);
    }

    let first = barcodes.get(0);
    let raw_value = js_sys::Reflect::get(&first, &JsValue::from_str("rawValue"))?;
    match raw_value.as_string() {
        Some(s) if !s.is_empty() => Ok(Some(s)),
        _ => Ok(None),
    }
}

async fn gloo_timers_sleep(ms: u32) {
    let promise = js_sys::Promise::new(&mut |resolve, _reject| {
        let window = web_sys::window().expect("no window");
        let _ = window.set_timeout_with_callback_and_timeout_and_arguments_0(&resolve, ms as i32);
    });
    let _ = JsFuture::from(promise).await;
}
