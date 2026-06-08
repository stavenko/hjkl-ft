use std::collections::BTreeMap;

use leptos::*;
use api_types::*;
use wasm_bindgen::JsCast;
use wasm_bindgen_futures::JsFuture;

use crate::services::api;
use crate::services::i18n::t;

/// Food creation form with AI fill.
/// `on_draft` is called automatically when AI fills the form — no manual save button.
#[component]
pub fn FoodEditor(
    custom_nutrients: Signal<Vec<NutrientSpec>>,
    on_draft: Callback<Food>,
) -> impl IntoView {
    let name = create_rw_signal(String::new());
    let kcal = create_rw_signal(String::new());
    let protein = create_rw_signal(String::new());
    let fat = create_rw_signal(String::new());
    let carbs = create_rw_signal(String::new());
    let custom_values = create_rw_signal(BTreeMap::<String, String>::new());
    let ai_loading = create_rw_signal(false);
    let ai_details = create_rw_signal(BTreeMap::<String, AiNutrientDetail>::new());

    let photos_base64 = create_rw_signal(Vec::<String>::new());
    let photo_count = create_rw_signal(0usize);

    let build_food = move || -> Food {
        let parse = |s: String| -> f64 { s.parse().unwrap_or(0.0) };
        let mut nutrients = BTreeMap::new();
        for (key, val) in custom_values.get_untracked() {
            let num_str: String = val.chars().take_while(|c| c.is_ascii_digit() || *c == '.').collect();
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

        on_draft.call(build_food());
    };

    let on_file_change = move |ev: leptos::ev::Event| {
        let input: web_sys::HtmlInputElement = ev.target().unwrap().unchecked_into();
        let count = input.files().unwrap().length() as usize;
        photo_count.set(count);
        spawn_local(async move {
            let mut results = Vec::new();
            let files = input.files().unwrap();
            for i in 0..files.length() {
                let file = files.get(i).unwrap();
                let array_buf = JsFuture::from(file.array_buffer()).await.unwrap();
                let uint8 = js_sys::Uint8Array::new(&array_buf);
                let bytes = uint8.to_vec();
                use base64::Engine;
                let b64 = base64::engine::general_purpose::STANDARD.encode(&bytes);
                results.push(b64);
            }
            photos_base64.set(results);
        });
    };

    let on_ai = move |_| {
        let images = photos_base64.get_untracked();
        let n = name.get_untracked();
        if images.is_empty() && n.is_empty() { return; }
        ai_loading.set(true);
        let nutrients_list = custom_nutrients.get_untracked();
        spawn_local(async move {
            if !images.is_empty() {
                let input = AiVisionInput { images, custom_nutrients: nutrients_list };
                match api::post::<_, AiLookupOutput>("/food/ai-vision", &input).await {
                    Ok(result) => apply_result(&result),
                    Err(_) => leptos::logging::error!("AI vision failed"),
                }
            } else {
                let input = AiLookupInput { name: n, custom_nutrients: nutrients_list };
                match api::post::<_, AiLookupOutput>("/food/ai-lookup", &input).await {
                    Ok(result) => apply_result(&result),
                    Err(_) => leptos::logging::error!("AI lookup failed"),
                }
            }
            ai_loading.set(false);
        });
    };

    let ai_hint = move |field_key: &str| {
        let key = field_key.to_string();
        view! {
            {move || {
                let details = ai_details.get();
                details.get(&key).map(|d| {
                    let tip = format!(
                        "{:.1}–{:.1} {} (rec: {:.1})\n{}",
                        d.min_value.value, d.max_value.value, d.recommended.unit,
                        d.recommended.value, d.comment,
                    );
                    view! {
                        <span
                            class="has-text-link is-size-7 ml-1"
                            style="cursor: help; text-decoration: underline; user-select: none;"
                            title=tip
                        >"?"</span>
                    }
                })
            }}
        }
    };

    view! {
        <div on:keydown=move |ev: leptos::ev::KeyboardEvent| {
            if ev.key() == "Enter" { ev.prevent_default(); }
        }>
            // Name
            <div class="field mb-3">
                <div class="control">
                    <input type="text" placeholder=t("food_editor.product_name") class="input is-small"
                        prop:value=move || name.get()
                        on:input=move |ev| name.set(event_target_value(&ev)) />
                </div>
            </div>

            // Photos
            <div class="field mb-3">
                <input type="file" accept="image/*" multiple=true
                    id="food-photo-input"
                    style="display: none;"
                    on:change=on_file_change />
                <div class="control">
                    <button type="button"
                        class="button is-small is-light is-fullwidth"
                        on:click=move |_| {
                            let doc = web_sys::window().unwrap().document().unwrap();
                            let el = doc.get_element_by_id("food-photo-input").unwrap();
                            use wasm_bindgen::JsCast;
                            let input: &web_sys::HtmlInputElement = el.unchecked_ref();
                            input.click();
                        }
                    >
                        {move || {
                            let c = photo_count.get();
                            if c == 0 {
                                t("food_editor.add_photo").to_string()
                            } else {
                                format!("{c} photo(s) attached")
                            }
                        }}
                    </button>
                </div>
            </div>

            // AI button
            <div class="field mb-4">
                <div class="control">
                    <button type="button"
                        class="button is-small is-link is-fullwidth"
                        disabled=move || ai_loading.get() || (name.get().is_empty() && photo_count.get() == 0)
                        on:click=on_ai
                    >
                        <span class="icon is-small mr-1">
                            <svg xmlns="http://www.w3.org/2000/svg" width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">
                                <path d="M12 2L15.09 8.26 22 9.27 17 14.14 18.18 21.02 12 17.77 5.82 21.02 7 14.14 2 9.27 8.91 8.26z"/>
                            </svg>
                        </span>
                        {move || if ai_loading.get() { t("food_editor.filling") } else { t("food_editor.fill_info") }}
                    </button>
                </div>
            </div>

            <hr class="my-2" />

            <div class="field is-horizontal mb-2">
                <div class="field-label is-small">
                    <label class="label">{t("food_editor.calories")} {ai_hint("kcal")}</label>
                </div>
                <div class="field-body">
                    <div class="field has-addons">
                        <div class="control is-expanded">
                            <input type="text" inputmode="decimal" placeholder="165" class="input is-small"
                                prop:value=move || kcal.get()
                                on:input=move |ev| kcal.set(event_target_value(&ev)) />
                        </div>
                        <div class="control">
                            <a class="button is-small is-static">{t("common.unit.kcal")}</a>
                        </div>
                    </div>
                </div>
            </div>
            <div class="field is-horizontal mb-2">
                <div class="field-label is-small">
                    <label class="label">{t("food_editor.protein")} {ai_hint("protein")}</label>
                </div>
                <div class="field-body">
                    <div class="field has-addons">
                        <div class="control is-expanded">
                            <input type="text" inputmode="decimal" placeholder="31" class="input is-small"
                                prop:value=move || protein.get()
                                on:input=move |ev| protein.set(event_target_value(&ev)) />
                        </div>
                        <div class="control">
                            <a class="button is-small is-static">{t("common.unit.g")}</a>
                        </div>
                    </div>
                </div>
            </div>
            <div class="field is-horizontal mb-2">
                <div class="field-label is-small">
                    <label class="label">{t("food_editor.fat")} {ai_hint("fat")}</label>
                </div>
                <div class="field-body">
                    <div class="field has-addons">
                        <div class="control is-expanded">
                            <input type="text" inputmode="decimal" placeholder="3.6" class="input is-small"
                                prop:value=move || fat.get()
                                on:input=move |ev| fat.set(event_target_value(&ev)) />
                        </div>
                        <div class="control">
                            <a class="button is-small is-static">{t("common.unit.g")}</a>
                        </div>
                    </div>
                </div>
            </div>
            <div class="field is-horizontal mb-2">
                <div class="field-label is-small">
                    <label class="label">{t("food_editor.carbs")} {ai_hint("carbs")}</label>
                </div>
                <div class="field-body">
                    <div class="field has-addons">
                        <div class="control is-expanded">
                            <input type="text" inputmode="decimal" placeholder="0" class="input is-small"
                                prop:value=move || carbs.get()
                                on:input=move |ev| carbs.set(event_target_value(&ev)) />
                        </div>
                        <div class="control">
                            <a class="button is-small is-static">{t("common.unit.g")}</a>
                        </div>
                    </div>
                </div>
            </div>

            // Custom nutrients
            <Show when=move || !custom_nutrients.get().is_empty()>
                <hr class="my-2" />
            </Show>
            <For
                each=move || custom_nutrients.get()
                key=|s| s.key.clone()
                children=move |spec| {
                    let key = spec.name.clone();
                    let key2 = spec.name.clone();
                    let hint_key = spec.name.clone();
                    let unit = spec.unit_label.clone();
                    view! {
                        <div class="field is-horizontal mb-2">
                            <div class="field-label is-small">
                                <label class="label">{spec.name.clone()} {ai_hint(&hint_key)}</label>
                            </div>
                            <div class="field-body">
                                <div class="field has-addons">
                                    <div class="control is-expanded">
                                        <input type="text" inputmode="decimal" placeholder="0" class="input is-small"
                                            prop:value=move || custom_values.get().get(&key).cloned().unwrap_or_default()
                                            on:input=move |ev| {
                                                let v = event_target_value(&ev);
                                                let k = key2.clone();
                                                custom_values.update(|m| { m.insert(k, v); });
                                            } />
                                    </div>
                                    <div class="control">
                                        <a class="button is-small is-static">{unit}</a>
                                    </div>
                                </div>
                            </div>
                        </div>
                    }
                }
            />
        </div>
    }
}
