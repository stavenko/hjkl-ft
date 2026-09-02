//! «Другая еда» — один экран вместо трёх вкладок.
//!
//! Раньше запись еды не из базы делилась на три пути: по описанию, по этикетке, по
//! фотографии. Человеку приходилось решать за нас, каким из них воспользоваться, — а
//! он просто хочет записать, что съел. Здесь снимки и описание лежат рядом, и
//! разбираться, что из этого этикетка, а что тарелка, — наша работа, не его.
//!
//! Запись ложится в дневник СРАЗУ и нераспознанной. Разбор идёт фоном и требует
//! сети; сети нет — запись остаётся нераспознанной и продолжает так выглядеть.
//! Поэтому кнопка называется «Добавить», а не «Распознать»: она добавляет запись, а
//! не запускает ожидание.

use leptos::*;

use crate::components::food_editor::file_to_jpeg_base64;
use crate::services::{i18n::t, images, lazy_food};

#[component]
pub fn OtherFoodPanel(
    /// День, в который пишем.
    date: String,
    /// Приём пищи, если человек добавляет внутрь него.
    #[prop(optional)]
    meal_label: Option<String>,
    /// Запись создана — закрыть панель и обновить дневник.
    on_added: Callback<()>,
    /// Человек передумал.
    on_cancel: Callback<()>,
) -> impl IntoView {
    let photos = create_rw_signal(Vec::<String>::new());
    let description = create_rw_signal(String::new());
    let saving = create_rw_signal(false);
    let photo_error = create_rw_signal(Option::<String>::None);

    let on_files = move |ev: leptos::ev::Event| {
        let input: web_sys::HtmlInputElement = event_target(&ev);
        let Some(files) = input.files().filter(|f| f.length() > 0) else { return };
        spawn_local(async move {
            let mut added = Vec::new();
            for i in 0..files.length() {
                let Some(file) = files.get(i) else { continue };
                match file_to_jpeg_base64(&file).await {
                    Ok(b64) => added.push(b64),
                    // Не роняем молча: непрочитанный снимок человек должен увидеть,
                    // иначе он будет ждать разбора того, чего мы не получили.
                    Err(e) => photo_error.set(Some(e)),
                }
            }
            // ДОБАВЛЯЕМ, а не заменяем: камера отдаёт по одному снимку за раз, и
            // лицевая сторона с оборотом приходят разными нажатиями.
            photos.update(|v| v.extend(added));
            input.set_value("");
        });
    };

    let add = move |_| {
        if saving.get_untracked() {
            return;
        }
        let imgs = photos.get_untracked();
        let text = description.get_untracked();
        if imgs.is_empty() && text.trim().is_empty() {
            return;
        }
        saving.set(true);
        let (date, meal) = (date.clone(), meal_label.clone());
        spawn_local(async move {
            lazy_food::create_pending(&imgs, &text, &date, meal).await;
            crate::services::sync::push_background();
            // Разбор пробуем сразу, но НЕ ждём его: запись уже в дневнике, и человек
            // волен закрыть приложение.
            lazy_food::run_queue_background();
            saving.set(false);
            on_added.call(());
        });
    };

    // Пустую запись добавлять нечего: ни снимков, ни слов.
    let can_add = move || !photos.get().is_empty() || !description.get().trim().is_empty();

    // Список для `<For>` собирается ЗДЕСЬ: турбофиш внутри макроса разметки не
    // разбирается, а без него тип не выводится.
    let photo_list = move || -> Vec<(usize, String)> {
        photos.get().into_iter().enumerate().collect()
    };

    view! {
        <div attr:data-testid="other-food-panel" style="padding: 8px 0;">
            <input type="file" accept="image/*" multiple=true
                id="other-food-photo-input" attr:data-testid="other-food-photo-input"
                style="display: none;" on:change=on_files />

            <div style="display: flex; flex-wrap: wrap; gap: 8px; margin-bottom: 12px;">
                <For
                    each=photo_list
                    key=|(i, b64)| format!("{i}-{}", b64.len())
                    children=move |(_, b64)| {
                        // Удаляем ПО СОДЕРЖИМОМУ, а не по номеру: после первого
                        // удаления номера сдвигаются, и по номеру ушёл бы не тот.
                        let mine = b64.clone();
                        view! {
                            <div style="position: relative;">
                                <img
                                    attr:data-testid="other-food-thumb"
                                    src=format!("data:image/jpeg;base64,{b64}")
                                    style="width: 72px; height: 72px; object-fit: cover; border-radius: 6px;" />
                                <button
                                    attr:data-testid="other-food-thumb-remove"
                                    class="delete is-small"
                                    style="position: absolute; top: 2px; right: 2px;"
                                    on:click=move |_| { let m = mine.clone(); photos.update(|v| v.retain(|x| x != &m)); }
                                ></button>
                            </div>
                        }
                    }
                />
                <label attr:for="other-food-photo-input"
                    attr:data-testid="other-food-add-photo"
                    class="button is-light"
                    style="width: 72px; height: 72px; display: flex; align-items: center; justify-content: center; font-size: 24px;"
                >"+"</label>
            </div>

            {move || photo_error.get().map(|e| view! {
                <p class="help is-danger" attr:data-testid="other-food-photo-error">{e}</p>
            })}

            <textarea
                attr:data-testid="other-food-description"
                class="textarea"
                rows="3"
                placeholder=move || t("other_food.description_placeholder")
                prop:value=move || description.get()
                on:input=move |ev| description.set(event_target_value(&ev))
            ></textarea>

            <div style="display: flex; gap: 8px; margin-top: 12px;">
                <button
                    attr:data-testid="other-food-add"
                    class="button is-primary is-fullwidth"
                    disabled=move || !can_add() || saving.get()
                    on:click=add
                >{move || t("other_food.add")}</button>
                <button
                    attr:data-testid="other-food-cancel"
                    class="button is-light"
                    on:click=move |_| on_cancel.call(())
                >{move || t("common.cancel")}</button>
            </div>

            <p class="help" style="margin-top: 8px;">{move || t("other_food.hint")}</p>
        </div>
    }
}

/// Миниатюры записи по хэшам — дневник показывает их, пока запись не распознана.
///
/// Картинка достаётся из провайдера по хэшу: сама запись несёт короткие строки, а не
/// мегабайты, и одна фотография в двух записях хранится один раз.
#[component]
pub fn EntryThumbnails(hashes: Vec<String>) -> impl IntoView {
    let srcs = create_rw_signal(Vec::<String>::new());
    spawn_local(async move {
        let mut out = Vec::new();
        for h in &hashes {
            if let Some(url) = images::data_url(h).await {
                out.push(url);
            }
        }
        srcs.set(out);
    });
    view! {
        <div style="display: flex; gap: 4px;">
            <For each=move || srcs.get() key=|s| s.clone() children=move |src| view! {
                <img attr:data-testid="entry-thumb" src=src
                    style="width: 32px; height: 32px; object-fit: cover; border-radius: 4px;" />
            } />
        </div>
    }
}
