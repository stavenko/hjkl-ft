//! Строка дневника для ленивых записей — нераспознанной и разобранной.
//!
//! Отдельным компонентом, а не ветвлением внутри обычной строки: обычная строка
//! знает про еду, граммы, отходы, повтор и копирование, и ни одно из этих понятий к
//! нераспознанной записи не применимо. Вплести её туда значило бы обвешать каждую
//! ветку проверками «а есть ли у нас вообще еда».
//!
//! Меню ОДИНАКОВОЕ у обеих форм и совпадает с обычной строкой дневника:
//! дублировать, перенести, изменить, удалить. Раньше разобранную нельзя было ни
//! удалить, ни скопировать — это оказалось неверно: съеденное удаляют целиком, а не
//! выскабливают по позиции, и повторить вчерашний обед хочется так же, как повторяют
//! обычную еду.

use api_types::{DiaryEntry, DiaryEntryKind, Food};
use leptos::*;

use crate::components::kebab::{kebab_icon, ITEM_CLASS, ITEM_STYLE, KEBAB_CLASS, KEBAB_STYLE, MENU_STYLE};
use crate::components::other_food_panel::EntryThumbnails;
use crate::services::{i18n::t, local};

#[component]
pub fn LazyDiaryRow(
    entry: DiaryEntry,
    /// Вся еда — по ней считаются КБЖУ разобранной записи.
    foods: Signal<Vec<Food>>,
    /// Открыть форму правки этой записи.
    on_edit: Callback<DiaryEntry>,
    /// Удалить запись целиком.
    on_delete: Callback<DiaryEntry>,
    /// Скопировать запись в выбранный приём — тем же окном, что и у обычной строки.
    on_duplicate: Callback<DiaryEntry>,
    /// Перенести запись в другой приём пищи — тем же окном выбора, что и у обычной
    /// строки. Держит его страница: окно одно на весь дневник.
    on_move: Callback<DiaryEntry>,
    /// Скрыть разделитель у последней строки.
    is_last: bool,
) -> impl IntoView {
    let menu_open = create_rw_signal(false);
    let pending = entry.kind == DiaryEntryKind::Pending;
    let e_edit = entry.clone();
    let e_delete = entry.clone();
    let e_move = entry.clone();
    let e_dup = entry.clone();
    let e_badges = entry.clone();

    // Нераспознанная показывает СЕБЯ: снимки и обрывок описания. Надписи «Ещё не
    // распознано» здесь больше нет — она одинакова у всех таких записей и потому
    // не помогает найти свою, а место занимает первым и самым крупным. Человек
    // узнаёт запись по кадру и по собственным словам, а не по нашему сообщению
    // о состоянии.
    let title = (!pending)
        .then(|| entry.label.clone())
        .flatten()
        .unwrap_or_default();
    // Описание показываем всегда, когда оно есть: у нераспознанной оно вместо
    // названия, у разобранной — под ним, как исходник, из которого её собрали.
    let note = entry.description.clone().unwrap_or_default().trim().to_string();
    let has_note = !note.is_empty();
    // Ни кадров, ни слов — сказать про запись нечего, и молчать нельзя.
    let mute = pending && !has_note && entry.images.is_empty();
    // Разбор не удался — говорим об этом прямо (§6.6). Пока попытки не кончились,
    // причина всё равно показывается: человек вправе знать, что происходит, и
    // поправить снимки, не дожидаясь третьего провала.
    let failure = pending.then(|| entry.recognition_error.clone()).flatten();
    let gave_up = pending && entry.recognition_tries >= crate::services::lazy_food::RECOGNITION_TRIES;

    view! {
        <div
            attr:data-testid=if pending { "diary-row-pending" } else { "diary-row-aggregate" }
            style=format!(
                "display: flex; align-items: center; padding: 0.5rem 0;{}",
                if is_last { "" } else { " border-bottom: 1px solid var(--bulma-border-weak);" }
            )
        >
            <div style="flex: 1; min-width: 0; overflow-wrap: break-word;">
                {(!title.is_empty()).then(|| view! {
                    <span
                        attr:data-testid="lazy-row-title"
                        class="is-size-6 has-text-weight-medium"
                    >{title.clone()}</span>
                })}

                // Снимки и слова — рядом, одной строкой: так это и лежало на экране
                // добавления, и так человек это помнит.
                {(!entry.images.is_empty() || has_note).then(|| view! {
                    <div style=format!("display: flex; align-items: flex-start; gap: 8px; min-width: 0;{}",
                                       if title.is_empty() { "" } else { " margin-top: 4px;" })>
                        {(!entry.images.is_empty()).then(|| view! {
                            <div style="flex: none;">
                                <EntryThumbnails hashes=entry.images.clone() />
                            </div>
                        })}
                        {has_note.then(|| view! {
                            // Ровно две строки, дальше многоточие: это НАПОМИНАНИЕ,
                            // а не текст для чтения, и разрастаться на полэкрана ему
                            // незачем.
                            <p
                                attr:data-testid="lazy-row-note"
                                class="is-size-7"
                                style="margin: 0; min-width: 0; flex: 1; line-height: 1.25; color: var(--bulma-text-weak); \
                                       display: -webkit-box; -webkit-line-clamp: 2; -webkit-box-orient: vertical; overflow: hidden;"
                            >{note.clone()}</p>
                        })}
                    </div>
                })}

                // Ни кадров, ни слов не бывает почти никогда (добавить пустую запись
                // нельзя), но если такое случилось — строка не должна быть пустой.
                // Причина неудачи — под кадрами и описанием, а не вместо них: снимки
                // человек узнаёт, а сообщение читает.
                {failure.clone().map(|reason| view! {
                    <p attr:data-testid="lazy-row-error" class="is-size-7 has-text-danger"
                        style="margin: 4px 0 0; line-height: 1.25;">
                        {reason}
                        {gave_up.then(|| view! {
                            <span attr:data-testid="lazy-row-gave-up" style="display: block; margin-top: 3px;">
                                {move || t("lazy_food.err.gave_up")}
                            </span>
                        })}
                    </p>
                })}

                {mute.then(|| view! {
                    <span attr:data-testid="lazy-row-title" class="is-size-6 has-text-weight-medium"
                        style="color: var(--bulma-text-weak);"
                    >{move || t("other_food.not_recognised")}</span>
                })}

                // КБЖУ есть только у разобранной: у нераспознанной их не существует,
                // и показывать нули значило бы соврать, будто еда бескалорийна.
                {(!pending).then(move || view! {
                    <div style="display: flex; flex-wrap: nowrap; gap: 4px; margin-top: 4px; min-width: 0; overflow: hidden;">
                        {move || {
                            let fs = foods.get();
                            use crate::services::i18n;
                            [("Calories", ""), ("Protein", i18n::unit_label("g")),
                             ("Fat", i18n::unit_label("g")), ("Carbs", i18n::unit_label("g"))]
                                .iter()
                                .map(|(key, unit)| {
                                    let v = local::entry_nutrient(&e_badges, &fs, key);
                                    view! {
                                        <span class="tag is-small">
                                            {format!("{} {:.0}", i18n::nutrient_badge(key), v)}
                                            " "
                                            <span class="has-text-grey-light">{unit.to_string()}</span>
                                        </span>
                                    }
                                })
                                .collect_view()
                        }}
                    </div>
                })}
            </div>

            // Кебаб и меню — из общего места (`components::kebab`), чтобы они не
            // разошлись с обычной строкой снова: раньше здесь стоял знак «⋯» другой
            // кнопкой и другого размера.
            <div style="position: relative;">
                <button
                    attr:data-testid="lazy-row-menu"
                    class=KEBAB_CLASS
                    style=KEBAB_STYLE
                    on:click=move |_| menu_open.update(|o| *o = !*o)
                >{kebab_icon()}</button>
                {move || menu_open.get().then(|| {
                    let e1 = e_edit.clone();
                    let e2 = e_delete.clone();
                    let e3 = e_move.clone();
                    let e4 = e_dup.clone();
                    // Порядок тот же, что в обычной строке: человек ищет пункт по
                    // месту, а не перечитывает список каждый раз.
                    view! {
                        <div style=MENU_STYLE>
                            <button
                                attr:data-testid="lazy-row-duplicate"
                                class=ITEM_CLASS style=ITEM_STYLE
                                on:click=move |_| { menu_open.set(false); on_duplicate.call(e4.clone()); }
                            >{move || t("diary.duplicate")}</button>
                            <button
                                attr:data-testid="lazy-row-move"
                                class=ITEM_CLASS style=ITEM_STYLE
                                on:click=move |_| { menu_open.set(false); on_move.call(e3.clone()); }
                            >{move || t("diary.move")}</button>
                            <button
                                attr:data-testid="lazy-row-edit"
                                class=ITEM_CLASS style=ITEM_STYLE
                                on:click=move |_| { menu_open.set(false); on_edit.call(e1.clone()); }
                            >{move || t("diary.edit")}</button>
                            <button
                                attr:data-testid="lazy-row-delete"
                                class=format!("{ITEM_CLASS} has-text-danger") style=ITEM_STYLE
                                on:click=move |_| { menu_open.set(false); on_delete.call(e2.clone()); }
                            >{move || t("diary.delete")}</button>
                        </div>
                    }
                })}
            </div>
        </div>
    }
}
