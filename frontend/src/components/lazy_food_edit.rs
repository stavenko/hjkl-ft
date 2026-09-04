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

use crate::components::photo_description::PhotoAndDescription;
use crate::services::{db, i18n::t, images, lazy_food, local};

#[component]
pub fn LazyFoodEdit(
    entry: DiaryEntry,
    foods: Signal<Vec<Food>>,
    /// Запись сохранена — закрыть форму и обновить дневник.
    on_saved: Callback<()>,
    on_cancel: Callback<()>,
) -> impl IntoView {
    // Верхняя половина. Снимки держим ОДНИМ списком base64 — тем же, каким их
    // видит форма добавления, — и потому обе пользуются общим `PhotoAndDescription`.
    //
    // Раньше здесь было два списка: хэши прежних снимков и base64 новых, «чтобы не
    // перекладывать мегабайты». Стоило это дорого: прежние снимки нельзя было ни
    // открыть, ни обрезать — только удалить, — и разметка расходилась с добавлением.
    // А экономии почти нет: снимки уже сжаты до 1536 и 0.85, их единицы, и дневник
    // всё равно распаковывает их для миниатюр.
    //
    // Обратно они кладутся тем же `images::put`: он адресует по содержимому, так что
    // нетронутый снимок получит прежний хэш и второй копии не возникнет.
    let photos = create_rw_signal(Vec::<String>::new());
    let description = create_rw_signal(entry.description.clone().unwrap_or_default());
    // Пока снимки достаются из базы, форму показывать можно — их место просто пусто.
    let loading_photos = create_rw_signal(!entry.images.is_empty());
    {
        let hashes = entry.images.clone();
        spawn_local(async move {
            let mut out = Vec::new();
            for h in &hashes {
                if let Some(b64) = images::get(h).await {
                    out.push(b64);
                }
            }
            photos.set(out);
            loading_photos.set(false);
        });
    }
    // Нижняя половина.
    let items = create_rw_signal(entry.items.clone());

    // `store_value`, а не обычные переменные: замыкание `top_changed` нужно и
    // обработчику сохранения, и разметке, а замыкание, захватившее String и Vec, не
    // копируется.
    let was_description = store_value(entry.description.clone().unwrap_or_default());
    // Сравниваем по ЧИСЛУ и содержимому кадров, а не по хэшам: обрезка меняет
    // содержимое, и запись обязана уйти на разбор заново — иначе список продуктов
    // остался бы от кадра, которого больше нет.
    let was_count = store_value(entry.images.len());
    let saving = create_rw_signal(false);
    // Обрезали ли какой-нибудь кадр. Число снимков при обрезке не меняется, а
    // содержимое — да, и по одному счёту такую правку не поймать.
    let photos_touched = create_rw_signal(false);
    create_effect(move |prev: Option<Vec<String>>| {
        let now = photos.get();
        if let Some(p) = prev {
            if !p.is_empty() && p != now && !loading_photos.get_untracked() {
                photos_touched.set(true);
            }
        }
        now
    });

    // Тронул ли человек верхнюю половину. Считается СРАВНЕНИЕМ с тем, что было, а не
    // флажком «поле получало фокус»: заглянуть в описание и выйти, ничего не изменив,
    // не должно стирать распознавание.
    let top_changed = move || {
        if loading_photos.get() {
            // Снимки ещё не достали — сравнивать не с чем, и объявлять запись
            // изменённой нельзя: она бы уехала на разбор от одного открытия формы.
            return false;
        }
        description.get().trim() != was_description.get_value().trim()
            || photos.get().len() != was_count.get_value()
            || photos_touched.get()
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
            // Кладём ВСЕ кадры: `images::put` адресует по содержимому, поэтому
            // нетронутый снимок получает прежний хэш, а обрезанный — новый.
            let mut hashes = Vec::new();
            for b64 in photos.get_untracked() {
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

    let rows = move || -> Vec<(usize, DiaryFoodItem)> { items.get().into_iter().enumerate().collect() };

    view! {
        <div attr:data-testid="lazy-food-edit" style="padding: 8px 0;">
            // Верхняя половина — тот же кусок, что и на добавлении
            // (`PhotoAndDescription`). Одинаковое дело должно выглядеть одинаково;
            // раньше здесь была своя разметка, и она отстала от добавления.
            <PhotoAndDescription photos=photos description=description input_id="lazy-edit-photo-input" />

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
                // Пока снимки достаются из базы, сохранять НЕЛЬЗЯ: список кадров
                // ещё пуст, и сохранение стёрло бы их все. Ждать недолго — это
                // чтение из локальной базы, — а цена ошибки чужие фотографии.
                <button attr:data-testid="lazy-edit-save" class="button is-primary is-fullwidth"
                    disabled=move || saving.get() || loading_photos.get() on:click=save
                >{move || t("common.save")}</button>
                <button attr:data-testid="lazy-edit-cancel" class="button is-light"
                    on:click=move |_| on_cancel.call(())
                >{move || t("common.cancel")}</button>
            </div>
        </div>
    }
}
