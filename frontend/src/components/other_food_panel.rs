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

use crate::components::photo_description::PhotoAndDescription;
use crate::services::{i18n::t, images, lazy_food};

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

            <PhotoAndDescription photos=photos description=description input_id="other-food-photo-input" />

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
