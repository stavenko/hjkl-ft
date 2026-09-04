//! Две зоны — снимок и описание. ОДИН кусок на добавление и на правку.
//!
//! Отдельным компонентом не ради красоты. Экраны эти уже расходились: добавление
//! переделали (зоны, подписи, значок камеры, ссылка «как это сделать», просмотр по
//! нажатию), а правка осталась на прежней разметке — голый «+», крестики внахлёст,
//! описание без подсказки. Человек в обоих местах делает одно и то же, и видеть
//! должен одно и то же; порознь эти экраны снова разъедутся при первой же правке.
//!
//! Снимки — ВСЕГДА base64. Правка держит их в базе хэшами, поэтому перед показом
//! распаковывает; обратно они кладутся тем же `images::put`, а он адресует по
//! содержимому, так что нетронутый снимок получает прежний хэш и второй копии не
//! возникает.

use leptos::*;

use crate::components::food_editor::file_to_jpeg_base64;
use crate::services::i18n::t;

/// Значок камеры — тот же, что на кнопке снимка в форме продукта. Взят оттуда
/// намеренно: одно и то же действие в приложении должно выглядеть одинаково.
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

#[component]
pub fn PhotoAndDescription(
    /// Снимки как base64 JPEG без префикса. Компонент их и добавляет, и меняет
    /// после обрезки, и удаляет.
    photos: RwSignal<Vec<String>>,
    /// Описание человека.
    description: RwSignal<String>,
    /// Свой идентификатор для `<input type=file>`: на одной странице этих форм
    /// может оказаться две, а `<label for>` найдёт первую попавшуюся.
    input_id: &'static str,
    /// Заперты ли зоны. У РАЗОБРАННОЙ записи снимки и описание показываются, но не
    /// правятся: правка любого из них отменяет разбор и отправляет запись в очередь
    /// заново. Человек должен решиться на это сам, нажав «Изменить», а не задеть
    /// случайно, потянувшись поправить граммы.
    ///
    /// Замки РАЗНЫЕ на зону: поправить описание, не трогая снимков, — обычное дело,
    /// и заставлять открывать обе зоны разом незачем.
    ///
    /// Не передали — зоны открыты (так на добавлении: терять там нечего).
    #[prop(optional)]
    photos_locked: Option<RwSignal<bool>>,
    #[prop(optional)]
    description_locked: Option<RwSignal<bool>>,
) -> impl IntoView {
    let photos_locked = photos_locked.unwrap_or_else(|| create_rw_signal(false));
    let description_locked = description_locked.unwrap_or_else(|| create_rw_signal(false));
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

    // Список для `<For>` собирается ЗДЕСЬ: турбофиш внутри макроса разметки не
    // разбирается, а без него тип не выводится.
    let photo_list = move || -> Vec<(usize, String)> { photos.get().into_iter().enumerate().collect() };

    view! {
        <div style="display: flex; flex-direction: column; gap: 20px;">
            <input type="file" accept="image/*" multiple=true
                id=input_id attr:data-testid="other-food-photo-input"
                style="display: none;" on:change=on_files />

            // ── Зона 1: снимок ──────────────────────────────────────────────
            <div style="display: flex; flex-direction: column; gap: 8px;">
                <div style="display: flex; align-items: baseline; justify-content: space-between; gap: 12px;">
                    <span class="is-size-6 has-text-weight-semibold">{move || t("other_food.photo_title")}</span>
                    {move || if photos_locked.get() {
                        view! {
                            <button type="button"
                                attr:data-testid="other-food-photo-edit"
                                class="is-size-7 has-text-link"
                                style="flex: none; white-space: nowrap; background: none; border: none; cursor: pointer; padding: 0; font: inherit;"
                                on:click=move |_| photos_locked.set(false)
                            >{move || t("common.edit")}</button>
                        }.into_view()
                    } else {
                        // Ссылка ведёт в статью «Фото и распознавание» — она уже есть,
                        // и её мы перепишем под этот путь отдельно.
                        view! {
                            <a attr:data-testid="other-food-photo-how"
                                href="/help/food-photo"
                                class="is-size-7 has-text-link"
                                style="flex: none; white-space: nowrap;"
                            >{move || t("other_food.photo_how")}</a>
                        }.into_view()
                    }}
                </div>

                // Подсказка — только когда зона открыта: запертой зоне объяснять,
                // что снимать, незачем, снимок уже сделан.
                {move || (!photos_locked.get()).then(|| view! {
                    <p class="help" style="margin-top: 0;">{move || t("other_food.photo_hint")}</p>
                })}

                {move || {
                    if photos_locked.get() {
                        // Заперто: кадры показываем, трогать не даём.
                        return view! {
                            <div style="display: flex; flex-wrap: wrap; gap: 8px; align-items: flex-start;">
                                {photo_list().into_iter().map(|(_, b64)| view! {
                                    <img attr:data-testid="other-food-thumb-locked"
                                        src=format!("data:image/jpeg;base64,{b64}")
                                        style="width: 56px; height: 56px; object-fit: cover; border-radius: 8px; border: 1px solid var(--bulma-border-weak);" />
                                }).collect_view()}
                            </div>
                        }.into_view();
                    }
                    if photo_list().is_empty() {
                        // Снимков нет — кнопка занимает всю ширину и НАЗЫВАЕТ себя.
                        // Одинокая плитка 56×56 со значком не читается: человек,
                        // впервые открывший экран, не знает, что она делает.
                        view! {
                            <label attr:for=input_id
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
                                        // Нажатие ОТКРЫВАЕТ снимок, а не удаляет его:
                                        // удалять вслепую по ноготку неправильно, и
                                        // крестик вдобавок налезал на соседний кадр.
                                        let mine = b64.clone();
                                        view! {
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
                                <label attr:for=input_id
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
                <div style="display: flex; align-items: baseline; justify-content: space-between; gap: 12px;">
                    <span class="is-size-6 has-text-weight-semibold">{move || t("other_food.description_title")}</span>
                    {move || description_locked.get().then(|| view! {
                        <button type="button"
                            attr:data-testid="other-food-description-edit"
                            class="is-size-7 has-text-link"
                            style="flex: none; white-space: nowrap; background: none; border: none; cursor: pointer; padding: 0; font: inherit;"
                            on:click=move |_| description_locked.set(false)
                        >{move || t("common.edit")}</button>
                    })}
                </div>

                {move || if description_locked.get() {
                    // Заперто: показываем написанное как текст. Пустое описание —
                    // говорим об этом прямо, иначе зона выглядит сломанной.
                    let text = description.get();
                    view! {
                        <p attr:data-testid="other-food-description-locked"
                            class=if text.trim().is_empty() { "is-size-6 has-text-grey-light" } else { "is-size-6" }
                            style="margin: 0; white-space: pre-wrap; overflow-wrap: break-word;"
                        >{if text.trim().is_empty() { t("other_food.description_empty").to_string() } else { text }}</p>
                    }.into_view()
                } else {
                    view! {
                        // Подсказка стоит НАД полем, а не внутри: она длинная, а
                        // placeholder исчезает от первой буквы — ровно когда человек
                        // ещё вспоминает, что писать.
                        <p class="help" style="margin-top: 0;">{move || t("other_food.description_hint")}</p>
                        <textarea
                            attr:data-testid="other-food-description"
                            class="textarea"
                            rows="3"
                            placeholder=move || t("other_food.description_placeholder")
                            prop:value=move || description.get()
                            on:input=move |ev| description.set(event_target_value(&ev))
                        ></textarea>
                    }.into_view()
                }}
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
        </div>
    }
}
