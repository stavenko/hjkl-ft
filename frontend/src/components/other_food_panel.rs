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
//!
//! Экран разделён на ДВЕ названные зоны — снимок и описание, — и каждая говорит, что
//! от человека нужно. Без этого он не догадывается: снимает стол целиком вместо
//! тарелки, а в описании пишет одно слово. Разделитель между зонами — волосяная
//! линия, а не две карточки: у области снимка уже есть своя пунктирная рамка, и
//! рамка вокруг рамки читается как вложенность, которой здесь нет.
//!
//! Строка поиска сюда не относится и на этом экране скрыта: искать по базе — шаг
//! ДО, и он остался прежним. Поэтому `show_other` живёт у страницы (как и
//! `show_editor`): спрятать свою шапку она может только зная, что открыто.

use leptos::*;

use crate::components::food_editor::file_to_jpeg_base64;
use crate::services::{i18n::t, images, lazy_food};

/// Тот же линейный значок камеры, что на кнопке снимка в форме продукта. Взят
/// оттуда намеренно: «добавить снимок» в приложении выглядит одинаково везде, иначе
/// человек не узнаёт действие, которое уже делал.
fn camera_icon(size: u32) -> impl IntoView {
    view! {
        <svg xmlns="http://www.w3.org/2000/svg" width=size height=size
            viewBox="0 0 24 24" fill="none" stroke="currentColor"
            stroke-width="2" stroke-linecap="round" stroke-linejoin="round" style="flex: none;">
            <path d="M14.5 4h-5L7 7H4a2 2 0 0 0-2 2v9a2 2 0 0 0 2 2h16a2 2 0 0 0 2-2V9a2 2 0 0 0-2-2h-3l-2.5-3z"/>
            <circle cx="12" cy="13" r="3"/>
        </svg>
    }
}

/// Заголовок зоны. Одна строка, один предмет — и место справа под ссылку, если она
/// у зоны есть.
fn zone_title(title: &'static str, aside: Option<View>) -> impl IntoView {
    view! {
        <div style="display: flex; align-items: baseline; justify-content: space-between; gap: 12px;">
            <span class="is-size-6 has-text-weight-semibold">{move || t(title)}</span>
            {aside}
        </div>
    }
}

#[component]
pub fn OtherFoodPanel(
    /// День, в который пишем.
    date: String,
    /// Приём пищи, если человек добавляет внутрь него.
    #[prop(optional_no_strip)]
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
    // Какой снимок открыт на просмотр. Держим САМ снимок, а не его номер: пока
    // просмотр открыт, список может измениться, и номер указал бы не на тот кадр.
    let viewing = create_rw_signal(Option::<String>::None);

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
        <div attr:data-testid="other-food-panel"
            style="display: flex; flex-direction: column; gap: 20px; padding: 4px 0 8px;">
            <input type="file" accept="image/*" multiple=true
                id="other-food-photo-input" attr:data-testid="other-food-photo-input"
                style="display: none;" on:change=on_files />

            // ── Зона 1: снимок ──────────────────────────────────────────────
            <div style="display: flex; flex-direction: column; gap: 8px;">
                {zone_title("other_food.photo_title", Some(view! {
                    // Ссылка ведёт в статью «Фото и распознавание» — она уже есть,
                    // и её мы перепишем под этот путь отдельно.
                    <a attr:data-testid="other-food-photo-how"
                        href="/help/food-photo"
                        class="is-size-7 has-text-link"
                        style="flex: none; white-space: nowrap;"
                    >{move || t("other_food.photo_how")}</a>
                }.into_view()))}

                <p class="help" style="margin-top: 0;">{move || t("other_food.photo_hint")}</p>

                {move || {
                    let shots = photo_list();
                    if shots.is_empty() {
                        // Снимков нет — кнопка занимает всю ширину и НАЗЫВАЕТ себя.
                        // Одинокая плитка 56×56 со значком не читается: человек,
                        // впервые открывший экран, не знает, что она делает.
                        view! {
                            <label attr:for="other-food-photo-input"
                                attr:data-testid="other-food-add-photo"
                                style="display: flex; align-items: center; justify-content: center; gap: 10px; \
                                       padding: 20px 12px; border: 1px dashed var(--bulma-border); border-radius: 10px; \
                                       background: var(--bulma-scheme-main); color: var(--bulma-text-weak); cursor: pointer;"
                            >
                                {camera_icon(24)}
                                <span class="is-size-6">{move || t("other_food.add_photo")}</span>
                            </label>
                        }.into_view()
                    } else {
                        // Снимки есть — они и есть содержание зоны, а кнопка
                        // становится такой же плиткой в их ряду, как в форме
                        // продукта: одинаковые края, один размер, одно место.
                        view! {
                            <div style="display: flex; flex-wrap: wrap; gap: 8px; align-items: flex-start;">
                                <For
                                    each=photo_list
                                    key=|(i, b64)| format!("{i}-{}", b64.len())
                                    children=move |(_, b64)| {
                                        // Удаляем ПО СОДЕРЖИМОМУ, а не по номеру: после
                                        // первого удаления номера сдвигаются, и по
                                        // номеру ушёл бы не тот.
                                        let mine = b64.clone();
                                        view! {
                                            // Нажатие ОТКРЫВАЕТ снимок, а не удаляет его. Крестика
                                            // на миниатюре больше нет: он налезал на соседнюю (56 px
                                            // в ряд через 8, а сам 20 и вынесен наружу), а главное —
                                            // удалять вслепую по ноготку неправильно. Удаление
                                            // переехало в просмотр, где видно, что удаляешь.
                                            <button type="button"
                                                attr:data-testid="other-food-thumb"
                                                attr:aria-label=t("other_food.open_photo")
                                                style="width: 56px; height: 56px; padding: 0; border: 1px solid var(--bulma-border-weak); border-radius: 8px; overflow: hidden; cursor: pointer; background: none;"
                                                on:click=move |_| viewing.set(Some(mine.clone()))
                                            >
                                                <img
                                                    src=format!("data:image/jpeg;base64,{b64}")
                                                    style="width: 100%; height: 100%; object-fit: cover; display: block;" />
                                            </button>
                                        }
                                    }
                                />
                                <label attr:for="other-food-photo-input"
                                    attr:data-testid="other-food-add-photo"
                                    attr:aria-label=t("other_food.photo_more")
                                    style="width: 56px; height: 56px; flex: none; display: flex; align-items: center; justify-content: center; \
                                           border: 1px dashed var(--bulma-border); border-radius: 8px; \
                                           background: var(--bulma-scheme-main); color: var(--bulma-text-weak); cursor: pointer;"
                                >{camera_icon(24)}</label>
                            </div>
                        }.into_view()
                    }
                }}

                {move || photo_error.get().map(|e| view! {
                    <p class="help is-danger" attr:data-testid="other-food-photo-error">{e}</p>
                })}
            </div>

            <div style="border-bottom: 0.5px solid var(--bulma-border-weak);"></div>

            // ── Зона 2: описание ────────────────────────────────────────────
            <div style="display: flex; flex-direction: column; gap: 8px;">
                {zone_title("other_food.description_title", None)}
                // Подсказка стоит НАД полем, а не внутри: она длинная, а placeholder
                // исчезает от первой буквы — ровно когда человек ещё вспоминает,
                // что писать.
                <p class="help" style="margin-top: 0;">{move || t("other_food.description_hint")}</p>
                <textarea
                    attr:data-testid="other-food-description"
                    class="textarea"
                    rows="3"
                    placeholder=move || t("other_food.description_placeholder")
                    prop:value=move || description.get()
                    on:input=move |ev| description.set(event_target_value(&ev))
                ></textarea>
            </div>

            // Просмотр снимка поверх всего: обрезать или удалить.
            {move || viewing.get().map(|shot| {
                let opened = shot.clone();
                let for_done = shot.clone();
                let for_delete = shot.clone();
                view! {
                    <crate::components::photo_crop::PhotoCrop
                        src=opened
                        on_done=Callback::new(move |cut: String| {
                            // Заменяем НА МЕСТЕ, чтобы порядок снимков не менялся:
                            // человек снимал тарелку и этикетку в своём порядке.
                            let was = for_done.clone();
                            photos.update(|v| {
                                if let Some(slot) = v.iter_mut().find(|x| **x == was) {
                                    *slot = cut.clone();
                                }
                            });
                            viewing.set(None);
                        })
                        on_delete=Callback::new(move |_| {
                            let was = for_delete.clone();
                            photos.update(|v| v.retain(|x| *x != was));
                            viewing.set(None);
                        })
                        on_cancel=Callback::new(move |_| viewing.set(None))
                    />
                }
            })}

            // ── Действия ────────────────────────────────────────────────────
            <div style="display: flex; flex-direction: column; gap: 8px;">
                <div style="display: flex; gap: 8px;">
                    <button
                        attr:data-testid="other-food-add"
                        // Заливку кнопка получает, только когда ей ЕСТЬ что добавить.
                        // С постоянной заливкой она выглядит нажимаемой всегда, и
                        // человек жмёт в пустоту, не понимая, чего от него хотят.
                        class=move || if can_add() && !saving.get() {
                            "button is-primary is-fullwidth"
                        } else {
                            "button is-fullwidth"
                        }
                        disabled=move || !can_add() || saving.get()
                        on:click=add
                    >{move || t("other_food.add")}</button>
                    <button
                        attr:data-testid="other-food-cancel"
                        class="button is-light"
                        on:click=move |_| on_cancel.call(())
                    >{move || t("common.cancel")}</button>
                </div>
                <p class="help" style="margin-top: 0;">{move || t("other_food.hint")}</p>
            </div>
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
    // 44, а не 32: рядом с ними в строке дневника стоят две строчки описания той
    // же высоты, и кадр должен читаться, а не намекать на себя.
    view! {
        <div style="display: flex; gap: 4px;">
            <For each=move || srcs.get() key=|s| s.clone() children=move |src| view! {
                <img attr:data-testid="entry-thumb" src=src
                    style="width: 44px; height: 44px; object-fit: cover; border-radius: 6px; border: 1px solid var(--bulma-border-weak);" />
            } />
        </div>
    }
}
