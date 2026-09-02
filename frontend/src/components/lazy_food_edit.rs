//! Правка ленивой записи — ОДНА форма с двумя половинами.
//!
//! Верхняя половина — снимки и описание, то, что человек дал сам. Нижняя —
//! распознанные продукты с граммами, то, что из этого вышло.
//!
//! Половины ведут себя по-разному, и в этом весь смысл формы:
//!
//!   верхняя  правка ОБНУЛЯЕТ распознавание — запись снова становится
//!            нераспознанной и уходит в очередь. Иначе вышла бы ложь: снимки одни,
//!            а список продуктов от других;
//!   нижняя   правка граммов и удаление позиции НЕ ведут к перераспознанию. Человек
//!            уточняет наш ответ, а не задаёт новый вопрос.
//!
//! Форма одна на обе формы записи. У нераспознанной нижняя половина пуста —
//! показывать там нечего, но и прятать её незачем: пустота честно говорит, что
//! разбора ещё не было.

use api_types::{DiaryEntry, DiaryEntryKind, DiaryFoodItem, Food};
use leptos::*;

use crate::components::food_editor::file_to_jpeg_base64;
use crate::services::{db, i18n::t, images, lazy_food, local};

#[component]
pub fn LazyFoodEdit(
    entry: DiaryEntry,
    foods: Signal<Vec<Food>>,
    /// Запись сохранена — закрыть форму и обновить дневник.
    on_saved: Callback<()>,
    on_cancel: Callback<()>,
) -> impl IntoView {
    // Верхняя половина. Картинки держим ХЭШАМИ, а не содержимым: пока человек их не
    // трогал, перекладывать мегабайты незачем.
    let image_hashes = create_rw_signal(entry.images.clone());
    let new_photos = create_rw_signal(Vec::<String>::new());
    let description = create_rw_signal(entry.description.clone().unwrap_or_default());
    // Нижняя половина.
    let items = create_rw_signal(entry.items.clone());

    // `store_value`, а не обычные переменные: замыкание `top_changed` нужно и
    // обработчику сохранения, и разметке, а замыкание, захватившее String и Vec, не
    // копируется.
    let was_description = store_value(entry.description.clone().unwrap_or_default());
    let was_images = store_value(entry.images.clone());
    let saving = create_rw_signal(false);
    let photo_error = create_rw_signal(Option::<String>::None);

    // Тронул ли человек верхнюю половину. Считается СРАВНЕНИЕМ с тем, что было, а не
    // флажком «поле получало фокус»: заглянуть в описание и выйти, ничего не изменив,
    // не должно стирать распознавание.
    let top_changed = move || {
        description.get().trim() != was_description.get_value().trim()
            || image_hashes.get() != was_images.get_value()
            || !new_photos.get().is_empty()
    };

    let on_files = move |ev: leptos::ev::Event| {
        let input: web_sys::HtmlInputElement = event_target(&ev);
        let Some(files) = input.files().filter(|f| f.length() > 0) else { return };
        spawn_local(async move {
            let mut added = Vec::new();
            for i in 0..files.length() {
                let Some(file) = files.get(i) else { continue };
                match file_to_jpeg_base64(&file).await {
                    Ok(b64) => added.push(b64),
                    Err(e) => photo_error.set(Some(e)),
                }
            }
            new_photos.update(|v| v.extend(added));
            input.set_value("");
        });
    };

    let entry_for_save = entry.clone();
    let save = move |_| {
        if saving.get_untracked() {
            return;
        }
        saving.set(true);
        let base = entry_for_save.clone();
        let reset = top_changed();
        spawn_local(async move {
            let mut hashes = image_hashes.get_untracked();
            for b64 in new_photos.get_untracked() {
                hashes.push(images::put(&b64).await);
            }
            let updated = if reset {
                // Снимки или описание изменились — прежний разбор к ним не относится.
                // Запись возвращается в очередь, а не остаётся с чужим списком.
                DiaryEntry {
                    kind: DiaryEntryKind::Pending,
                    items: Vec::new(),
                    label: None,
                    recognized_at: None,
                    images: hashes,
                    description: (!description.get_untracked().trim().is_empty())
                        .then(|| description.get_untracked().trim().to_string()),
                    updated_at: local::now(),
                    ..base
                }
            } else {
                // Тронули только граммы или состав списка. Форма записи не меняется:
                // разобранная остаётся разобранной, нераспознанная — нераспознанной.
                DiaryEntry {
                    items: items.get_untracked(),
                    images: hashes,
                    updated_at: local::now(),
                    ..base
                }
            };
            db::put("diary", &updated).await;
            crate::services::sync::push_background();
            if reset {
                lazy_food::run_queue_background();
            }
            saving.set(false);
            on_saved.call(());
        });
    };

    let thumbs = move || -> Vec<(usize, String)> { new_photos.get().into_iter().enumerate().collect() };
    let rows = move || -> Vec<(usize, DiaryFoodItem)> { items.get().into_iter().enumerate().collect() };

    view! {
        <div attr:data-testid="lazy-food-edit" style="padding: 8px 0;">
            // ── верхняя половина ──
            <p class="is-size-7 has-text-weight-semibold">{move || t("lazy_edit.top_title")}</p>
            <input type="file" accept="image/*" multiple=true
                id="lazy-edit-photo-input" attr:data-testid="lazy-edit-photo-input"
                style="display: none;" on:change=on_files />

            <div style="display: flex; flex-wrap: wrap; gap: 8px; margin: 8px 0;">
                <For each=move || image_hashes.get() key=|h| h.clone() children=move |hash| {
                    let mine = hash.clone();
                    view! {
                        <div style="position: relative;">
                            <crate::components::other_food_panel::EntryThumbnails hashes=vec![hash.clone()] />
                            <button attr:data-testid="lazy-edit-thumb-remove" class="delete is-small"
                                style="position: absolute; top: -4px; right: -4px;"
                                on:click=move |_| { let m = mine.clone(); image_hashes.update(|v| v.retain(|x| x != &m)); }
                            ></button>
                        </div>
                    }
                } />
                <For each=thumbs key=|(i, b)| format!("{i}-{}", b.len()) children=move |(_, b64)| {
                    let mine = b64.clone();
                    view! {
                        <div style="position: relative;">
                            <img attr:data-testid="lazy-edit-new-thumb"
                                src=format!("data:image/jpeg;base64,{b64}")
                                style="width: 48px; height: 48px; object-fit: cover; border-radius: 4px;" />
                            <button class="delete is-small" style="position: absolute; top: -4px; right: -4px;"
                                on:click=move |_| { let m = mine.clone(); new_photos.update(|v| v.retain(|x| x != &m)); }
                            ></button>
                        </div>
                    }
                } />
                <label attr:for="lazy-edit-photo-input" attr:data-testid="lazy-edit-add-photo"
                    class="button is-light is-small"
                    style="width: 48px; height: 48px; display: flex; align-items: center; justify-content: center;"
                >"+"</label>
            </div>

            {move || photo_error.get().map(|e| view! { <p class="help is-danger">{e}</p> })}

            <textarea attr:data-testid="lazy-edit-description" class="textarea" rows="3"
                prop:value=move || description.get()
                on:input=move |ev| description.set(event_target_value(&ev))
            ></textarea>

            // Предупреждение появляется, только когда верх ДЕЙСТВИТЕЛЬНО изменён:
            // висеть постоянно оно значило бы пугать человека, который зашёл
            // поправить граммы.
            {move || top_changed().then(|| view! {
                <p attr:data-testid="lazy-edit-reset-warning" class="help is-warning" style="margin-top: 4px;">
                    {move || t("lazy_edit.will_reset")}
                </p>
            })}

            // ── нижняя половина ──
            <hr style="margin: 16px 0;" />
            <p class="is-size-7 has-text-weight-semibold">{move || t("lazy_edit.bottom_title")}</p>

            {move || items.get().is_empty().then(|| view! {
                <p attr:data-testid="lazy-edit-nothing-yet" class="help" style="margin: 8px 0;">
                    {move || t("lazy_edit.nothing_yet")}
                </p>
            })}

            <For each=rows key=|(_, it)| it.food_id.clone() children=move |(idx, it)| {
                let fid = it.food_id.clone();
                let fid_for_name = fid.clone();
                view! {
                    <div attr:data-testid="lazy-edit-item"
                        style="display: flex; align-items: center; gap: 8px; padding: 4px 0;">
                        <span style="flex: 1; min-width: 0;">
                            {move || foods.get().iter().find(|f| f.id == fid_for_name)
                                .map(|f| f.name.clone())
                                .unwrap_or_else(|| t("lazy_edit.unknown_food").to_string())}
                        </span>
                        <input attr:data-testid="lazy-edit-grams" class="input is-small" style="width: 5.5rem;"
                            type="number" inputmode="numeric"
                            prop:value=move || items.get().get(idx).map(|i| i.grams).unwrap_or(0.0)
                            on:input=move |ev| {
                                let v = event_target_value(&ev).parse::<f64>().unwrap_or(0.0);
                                items.update(|list| if let Some(i) = list.get_mut(idx) { i.grams = v.max(0.0); });
                            } />
                        <button attr:data-testid="lazy-edit-item-remove" class="delete"
                            on:click=move |_| { let f = fid.clone(); items.update(|l| l.retain(|i| i.food_id != f)); }
                        ></button>
                    </div>
                }
            } />

            <div style="display: flex; gap: 8px; margin-top: 16px;">
                <button attr:data-testid="lazy-edit-save" class="button is-primary is-fullwidth"
                    disabled=move || saving.get() on:click=save
                >{move || t("common.save")}</button>
                <button attr:data-testid="lazy-edit-cancel" class="button is-light"
                    on:click=move |_| on_cancel.call(())
                >{move || t("common.cancel")}</button>
            </div>
        </div>
    }
}
