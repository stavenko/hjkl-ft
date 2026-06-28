use leptos::*;
use leptos_router::*;
use wasm_bindgen::JsCast;
use wasm_bindgen_futures::JsFuture;

use crate::services::{db, i18n::t, local};

const PAGE_BG: &str = "background: var(--bulma-background); min-height: 100vh; padding: 0; margin: -0.75rem;";
const CARD: &str = "background: var(--bulma-scheme-main); border-radius: 12px; overflow: hidden;";

/// (pose key, i18n label key)
const POSE_LABELS: [(&str, &str); 3] =
    [("front", "progress.pose_front"), ("side", "progress.pose_side"), ("back", "progress.pose_back")];

fn click_input(id: &str) {
    let doc = web_sys::window().unwrap().document().unwrap();
    if let Some(el) = doc.get_element_by_id(id) {
        let input: &web_sys::HtmlInputElement = el.unchecked_ref();
        input.click();
    }
}

/// Read the picked file (camera or gallery) and store it for `pose`.
fn handle_file(pose: &'static str, ev: web_sys::Event) {
    let input: web_sys::HtmlInputElement = ev.target().unwrap().unchecked_into();
    let Some(files) = input.files() else { return };
    let Some(file) = files.get(0) else { return };
    spawn_local(async move {
        let mime = { let m = file.type_(); if m.is_empty() { "image/jpeg".to_string() } else { m } };
        let buf = JsFuture::from(file.array_buffer()).await.unwrap();
        let bytes = js_sys::Uint8Array::new(&buf).to_vec();
        use base64::Engine;
        let b64 = base64::engine::general_purpose::STANDARD.encode(&bytes);
        local::save_progress_photo(pose, &format!("data:{mime};base64,{b64}")).await;
    });
}

#[component]
pub fn ProgressPage() -> impl IntoView {
    let navigate = use_navigate();
    let ver = db::version("progress_photos");
    let photos = create_resource(move || ver.get(), |_| async { local::list_progress_photos().await });

    view! {
        <div style=PAGE_BG>
            <div style="display: flex; align-items: center; padding: 12px 16px;">
                <button
                    style="appearance: none; -webkit-appearance: none; border: none; background: none; cursor: pointer; padding: 4px; font: inherit;"
                    class="is-size-5"
                    on:click={ let nav = navigate.clone(); move |_| nav("/", Default::default()) }
                >
                    <span class="has-text-link">{move || t("common.back")}</span>
                </button>
            </div>

            <h1 class="is-size-1 has-text-weight-bold" style="margin: 0 16px 8px 16px;">{move || t("progress.title")}</h1>
            <p class="is-size-6 has-text-grey" style="margin: 0 16px 12px 16px;">{move || t("progress.subtitle")}</p>

            // Recommendations for consistent, comparable shots.
            <div style="margin: 0 16px 16px 16px;">
                <p class="is-size-7 has-text-grey-light" style="text-transform: uppercase; letter-spacing: 0.02em; margin: 0 0 6px 4px;">
                    {move || t("progress.tips_title")}
                </p>
                <ul style="margin: 0; padding-left: 22px; list-style: disc;">
                    <li class="is-size-6" style="margin-bottom: 4px;">{move || t("progress.tip_bg")}</li>
                    <li class="is-size-6">{move || t("progress.tip_height")}</li>
                </ul>
            </div>

            // Three pose slots — latest photo + capture button.
            <div style="padding: 0 16px; display: flex; flex-direction: column; gap: 12px;">
                {POSE_LABELS.iter().map(|(pose, label)| {
                    let pose = *pose;
                    let label = *label;
                    let cam_id = format!("progress-cam-{pose}");
                    let gal_id = format!("progress-gal-{pose}");
                    let cam_btn = cam_id.clone();
                    let gal_btn = gal_id.clone();
                    let latest = move || photos.get().unwrap_or_default().into_iter().find(|p| p.pose == pose).map(|p| p.image);
                    view! {
                        <div style=CARD>
                            // Camera (forces capture) and gallery (plain picker) — two inputs.
                            <input type="file" accept="image/*" capture="environment" id=cam_id style="display: none;"
                                on:change=move |ev| handle_file(pose, ev) />
                            <input type="file" accept="image/*" id=gal_id style="display: none;"
                                on:change=move |ev| handle_file(pose, ev) />
                            <div style="display: flex; align-items: center; gap: 8px; padding: 12px 16px;">
                                <span class="is-size-6 has-text-weight-medium" style="flex: 1;">{move || t(label)}</span>
                                <button class="button is-link is-small" on:click=move |_| click_input(&cam_btn)>
                                    {move || t("progress.take_photo")}
                                </button>
                                <button class="button is-small" on:click=move |_| click_input(&gal_btn)>
                                    {move || t("progress.from_gallery")}
                                </button>
                            </div>
                            {move || latest().map(|img| view! {
                                <img src=img style="display: block; width: 100%; max-height: 360px; object-fit: contain; background: var(--bulma-background);" />
                            })}
                        </div>
                    }
                }).collect_view()}
            </div>

            // History — all shots, newest first.
            <div style="padding: 16px;">
                <p class="is-size-7 has-text-grey-light" style="text-transform: uppercase; letter-spacing: 0.02em; margin: 0 0 8px 4px;">
                    {move || t("progress.history")}
                </p>
                {move || {
                    let all = photos.get().unwrap_or_default();
                    if all.is_empty() {
                        view! { <p class="is-size-6 has-text-grey">{move || t("progress.empty")}</p> }.into_view()
                    } else {
                        view! {
                            <div style="display: grid; grid-template-columns: repeat(3, 1fr); gap: 6px;">
                                {all.into_iter().map(|p| {
                                    let pose_label = POSE_LABELS.iter().find(|(k, _)| *k == p.pose).map(|(_, l)| *l).unwrap_or("");
                                    view! {
                                        <div style="position: relative;">
                                            <img src=p.image style="display: block; width: 100%; aspect-ratio: 3/4; object-fit: cover; border-radius: 8px;" />
                                            <span class="is-size-7" style="position: absolute; bottom: 4px; left: 4px; background: var(--overlay-scrim); color: var(--bulma-dark-invert); padding: 1px 6px; border-radius: 6px;">
                                                {format!("{} · {}", t(pose_label), p.date)}
                                            </span>
                                        </div>
                                    }
                                }).collect_view()}
                            </div>
                        }.into_view()
                    }
                }}
            </div>

            <div style="height: 40px;"></div>
        </div>
    }
}
