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

use crate::components::food_badges::{nutrient_badges, BADGE_ROW};
use crate::components::food_weight_modal::FoodWeightModal;
use crate::components::kebab::{kebab_icon, ITEM_CLASS, ITEM_STYLE, KEBAB_CLASS, KEBAB_STYLE, MENU_STYLE};
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

    // У РАЗОБРАННОЙ записи верх заперт. Любая правка снимков или описания отменяет
    // разбор и отправляет запись в очередь заново — на это надо решиться, нажав
    // «Изменить», а не задеть случайно, потянувшись поправить граммы.
    //
    // У нераспознанной терять нечего: разбора ещё не было, и зоны открыты сразу.
    let recognised = entry.kind == DiaryEntryKind::Aggregate;
    // Замок ОДИН на обе зоны, хотя кнопка «Изменить» стоит на каждой. Так задумано:
    // цена у правки общая (разбор отменяется целиком), и человек, взявшийся править
    // описание, тут же может поправить и кадр — второй раз решаться незачем.
    let top_locked = create_rw_signal(recognised);
    /// Открыта ли правка верха. Пока она открыта, разобранные позиции НЕ
    /// показываются: человек занят снимками и словами, а список продуктов, который
    /// вот-вот исчезнет, только отвлекал бы.
    let editing_top = move || recognised && !top_locked.get();

    // Что было до правки — чтобы «Отмена» вернула и снимки, и описание. Правка
    // верха НИЧЕГО не пишет в базу до «Сохранить»: ни состав снимков, ни обрезка,
    // ни описание. Снимок берётся в тот миг, когда зоны отпирают.
    let snapshot = store_value(None::<(Vec<String>, String)>);

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
    create_effect(move |was: Option<bool>| {
        let now = top_locked.get();
        if was == Some(true) && !now {
            snapshot.set_value(Some((photos.get_untracked(), description.get_untracked())));
        }
        now
    });

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

    // «Отмена» в режиме правки верха. Ничего в базу не писалось, поэтому вернуть
    // надо только то, что человек менял в памяти: снимки (состав, порядок, обрезку)
    // и описание. Позиции при этом возвращаются сами — они и не трогались, просто
    // были спрятаны.
    let cancel_top = move |_| {
        if let Some((p, d)) = snapshot.get_value() {
            photos.set(p);
            description.set(d);
        }
        photos_touched.set(false);
        top_locked.set(true);
    };

    // Какая позиция правит вес (её номер) и у какой открыто меню. По одной за раз:
    // это список, а не таблица, и два открытых меню друг другу мешают.
    let weighing = create_rw_signal(None::<usize>);
    let menu_for = create_rw_signal(None::<usize>);

    let rows = move || -> Vec<(usize, DiaryFoodItem)> { items.get().into_iter().enumerate().collect() };

    view! {
        <div attr:data-testid="lazy-food-edit" style="padding: 8px 0;">
            // Верхняя половина — тот же кусок, что и на добавлении
            // (`PhotoAndDescription`). Одинаковое дело должно выглядеть одинаково;
            // раньше здесь была своя разметка, и она отстала от добавления.
            <PhotoAndDescription photos=photos description=description
                input_id="lazy-edit-photo-input"
                photos_locked=top_locked description_locked=top_locked />

            // Предупреждение появляется, только когда верх ДЕЙСТВИТЕЛЬНО изменён:
            // висеть постоянно оно значило бы пугать человека, который зашёл
            // поправить граммы.
            {move || top_changed().then(|| view! {
                <p attr:data-testid="lazy-edit-reset-warning" class="help is-warning" style="margin-top: 4px;">
                    {move || t("lazy_edit.will_reset")}
                </p>
            })}

            // ── нижняя половина ──
            //
            // На время правки верха её не показываем вовсе: список продуктов при
            // сохранении исчезнет, и держать его перед глазами значит предлагать
            // править то, чего сейчас не станет.
            {move || (!editing_top()).then(|| view! {
                <hr style="margin: 16px 0;" />
                <p class="is-size-7 has-text-weight-semibold">{move || t("lazy_edit.bottom_title")}</p>

                {move || items.get().is_empty().then(|| view! {
                    <p attr:data-testid="lazy-edit-nothing-yet" class="help" style="margin: 8px 0;">
                        {move || t("lazy_edit.nothing_yet")}
                    </p>
                })}

                // Строка позиции — как в дневнике: название, пилюли КБЖУ, граммы
                // ссылкой справа и кебаб. Это одна и та же еда, и человек не должен
                // заново разбираться, что здесь на что похоже.
                <For each=rows key=|(_, it)| it.food_id.clone() children=move |(idx, it)| {
                    let fid = it.food_id.clone();
                    let fid_name = fid.clone();
                    let fid_style = fid.clone();
                    let fid_badges = fid.clone();
                    let fid_del = fid.clone();
                    let fid_weigh = fid.clone();
                    let food_of = move |id: &str| foods.get().into_iter().find(|f| f.id == id);
                    let grams_of = move || items.get().get(idx).map(|i| i.grams).unwrap_or(0.0);
                    view! {
                        <div attr:data-testid="lazy-edit-item"
                            style="display: flex; align-items: center; padding: 0.5rem 0; border-bottom: 1px solid var(--bulma-border-weak);">
                            <div style="flex: 1; min-width: 0; overflow-wrap: break-word;">
                                <span class="is-size-6 has-text-weight-medium"
                                    style=move || if food_of(&fid_style).is_some_and(|f| f.is_restaurant) {
                                        crate::components::food_list_item::RESTAURANT_NAME_STYLE
                                    } else { "" }
                                >
                                    {move || food_of(&fid_name).map(|f| f.name)
                                        .unwrap_or_else(|| t("lazy_edit.unknown_food").to_string())}
                                </span>
                                <div style=BADGE_ROW>
                                    {move || food_of(&fid_badges).map(|f| nutrient_badges(&f, grams_of() / 100.0))}
                                </div>
                            </div>

                            <div style="flex-shrink: 0; margin-left: 1rem; display: flex; align-items: center; gap: 0.75rem;">
                                // Граммы — та же ссылка, что в дневнике, и открывает то
                                // же окно веса.
                                <button attr:data-testid="lazy-edit-grams"
                                    class="button is-ghost is-small has-text-link"
                                    style="height: auto; text-decoration: none;"
                                    on:click=move |_| weighing.set(Some(idx))
                                >
                                    <span class="is-size-7">{move || format!("{:.0}{}", grams_of(), t("common.unit.g"))}</span>
                                </button>

                                <div style="position: relative;">
                                    <button attr:data-testid="lazy-edit-item-menu"
                                        class=KEBAB_CLASS style=KEBAB_STYLE
                                        on:click=move |_| menu_for.update(|m| {
                                            if *m == Some(idx) { *m = None } else { *m = Some(idx) }
                                        })
                                    >{kebab_icon()}</button>
                                    {move || (menu_for.get() == Some(idx)).then(|| {
                                        let f = fid_del.clone();
                                        view! {
                                            <div style=MENU_STYLE>
                                                <button attr:data-testid="lazy-edit-item-remove"
                                                    class=format!("{ITEM_CLASS} has-text-danger") style=ITEM_STYLE
                                                    on:click=move |_| {
                                                        let f = f.clone();
                                                        menu_for.set(None);
                                                        items.update(|l| l.retain(|i| i.food_id != f));
                                                    }
                                                >{move || t("diary.delete")}</button>
                                            </div>
                                        }
                                    })}
                                </div>
                            </div>
                        </div>

                        // Окно веса — то же самое, что правит граммы в дневнике. Полей
                        // «несъеденное» и «ресторанная еда» здесь нет: позиция хранит
                        // только еду и граммы, и предлагать остальное значило бы врать.
                        {move || (weighing.get() == Some(idx))
                            .then(|| food_of(&fid_weigh))
                            .flatten()
                            .map(|food| view! {
                                <FoodWeightModal
                                    food=food
                                    initial_grams=grams_of()
                                    submit_label=t("weight.save")
                                    on_save=Callback::new(move |(g, _waste, _rest): (f64, f64, bool)| {
                                        items.update(|l| if let Some(i) = l.get_mut(idx) { i.grams = g.max(0.0); });
                                        weighing.set(None);
                                    })
                                    on_close=Callback::new(move |_| weighing.set(None))
                                />
                            })}
                    }
                } />
            })}

            <div style="display: flex; gap: 8px; margin-top: 16px;">
                // Пока снимки достаются из базы, сохранять НЕЛЬЗЯ: список кадров
                // ещё пуст, и сохранение стёрло бы их все. Ждать недолго — это
                // чтение из локальной базы, — а цена ошибки чужие фотографии.
                <button attr:data-testid="lazy-edit-save" class="button is-primary is-fullwidth"
                    disabled=move || saving.get() || loading_photos.get() on:click=save
                >{move || t("common.save")}</button>
                // «Отмена» значит разное в двух положениях, и это не двусмысленность,
                // а точность: в правке верха отменять нечего, кроме самой правки, —
                // форма остаётся открытой и позиции возвращаются на место. В обычном
                // положении отменять нечего вовсе, и она просто закрывает форму.
                {move || if editing_top() {
                    view! {
                        <button attr:data-testid="lazy-edit-cancel-top" class="button is-light"
                            on:click=cancel_top
                        >{move || t("common.cancel")}</button>
                    }.into_view()
                } else {
                    view! {
                        <button attr:data-testid="lazy-edit-cancel" class="button is-light"
                            on:click=move |_| on_cancel.call(())
                        >{move || t("common.cancel")}</button>
                    }.into_view()
                }}
            </div>
        </div>
    }
}
