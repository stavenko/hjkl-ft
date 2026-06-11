use std::collections::BTreeMap;

use leptos::*;
use api_types::*;
use wasm_bindgen::JsCast;
use wasm_bindgen_futures::JsFuture;

use crate::services::ai;
use crate::services::i18n::t;
use crate::services::local;

#[component]
pub fn FoodEditor(
    custom_nutrients: Signal<Vec<NutrientSpec>>,
    on_draft: Callback<(Food, Option<String>)>,
) -> impl IntoView {
    let name = create_rw_signal(String::new());
    let kcal = create_rw_signal(String::new());
    let protein = create_rw_signal(String::new());
    let fat = create_rw_signal(String::new());
    let carbs = create_rw_signal(String::new());
    let custom_values = create_rw_signal(BTreeMap::<String, String>::new());
    let ai_loading = create_rw_signal(false);
    let ai_error = create_rw_signal(None::<String>);
    let ai_details = create_rw_signal(BTreeMap::<String, AiNutrientDetail>::new());
    let draft_id = create_rw_signal(None::<String>);

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
        ai_error.set(None);
        let nutrients_list = custom_nutrients.get_untracked();
        spawn_local(async move {
            let result = if !images.is_empty() {
                let input = AiVisionInput { images, custom_nutrients: nutrients_list };
                ai::vision(&input).await
            } else {
                let input = AiLookupInput { name: n, custom_nutrients: nutrients_list };
                ai::lookup(&input).await
            };
            match result {
                Ok(output) => {
                    apply_result(&output);
                    let draft = local::save_draft(&build_food()).await;
                    draft_id.set(Some(draft.id));
                }
                Err(e) => {
                    ai_error.set(Some(e));
                }
            }
            ai_loading.set(false);
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
                details.get(&key).map(|d| {
                    let tip = format!(
                        "{:.1}–{:.1} {} (rec: {:.1})\n{}",
                        d.min_value.value, d.max_value.value, d.recommended.unit,
                        d.recommended.value, d.comment,
                    );
                    view! {
                        <span
                            class="has-text-link is-size-7"
                            style="margin-left: 4px; cursor: help; text-decoration: underline;"
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
            // Name input
            <input type="text"
                placeholder=t("food_editor.product_name")
                class="is-size-6"
                style="width: 100%; padding: 10px 12px; border: none; border-radius: 10px; background: var(--bulma-background); color: var(--bulma-text); outline: none; box-sizing: border-box; margin-bottom: 10px;"
                prop:value=move || name.get()
                on:input=move |ev| name.set(event_target_value(&ev))
            />

            // Photo + AI buttons row
            <div style="display: flex; gap: 8px; margin-bottom: 12px;">
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
                        let c = photo_count.get();
                        if c == 0 {
                            format!("\u{1f4f7} {}", t("food_editor.add_photo"))
                        } else {
                            format!("\u{1f4f7} {c}")
                        }
                    }}
                </button>
                <button type="button"
                    class="button is-link is-size-7"
                    style="flex: 1; padding: 8px 0; border: none; border-radius: 10px; cursor: pointer;"
                    disabled=move || ai_loading.get() || (name.get().is_empty() && photo_count.get() == 0)
                    on:click=on_ai
                >
                    {move || if ai_loading.get() {
                        format!("\u{2728} {}", t("food_editor.filling"))
                    } else {
                        format!("\u{2728} {}", t("food_editor.fill_info"))
                    }}
                </button>
            </div>

            {move || ai_error.get().map(|e| view! {
                <div class="has-text-danger is-size-7" style="padding: 8px 12px; margin-bottom: 10px; background: var(--bulma-danger-light); border-radius: 10px;">
                    {e}
                </div>
            })}

            // Nutrient fields card
            <div style="background: var(--bulma-background); border-radius: 12px; overflow: hidden;">
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
            <div style="display: flex; align-items: center; padding: 10px 12px; background: var(--bulma-scheme-main);">
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
