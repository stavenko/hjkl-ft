use std::collections::BTreeMap;

use leptos::*;
use api_types::Food;

use crate::services::i18n::{t, nutrient_name, unit_label};
use crate::services::local::{self, AiFoodData};

/// Edit the product behind a diary entry: the manual part (name + КБЖУ) and, in a
/// separate framed block, everything the AI derived — nutrients, iron (amount +
/// absorbed fraction) and the category flags. The frame exists so it is never in
/// doubt which numbers a human typed and which a model guessed; the model does get
/// them wrong, and the block is editable precisely so that can be fixed.
///
/// Save applies copy-on-write via `local::edit_food_for_entry` (edits in place if
/// the product is used only by this entry, else clones it). Opened from the diary
/// row long-press "Изменить".
#[component]
pub fn FoodEditModal(
    food: Food,
    entry_id: String,
    on_saved: Callback<()>,
    on_close: Callback<()>,
) -> impl IntoView {
    let fmt = |v: f64| if v == 0.0 { String::new() } else { format!("{v}") };
    let fmt_opt = |v: Option<f64>| v.map(|x| format!("{x}")).unwrap_or_default();

    let name = create_rw_signal(food.name.clone());
    let kcal = create_rw_signal(fmt(food.kcal));
    let protein = create_rw_signal(fmt(food.protein));
    let fat = create_rw_signal(fmt(food.fat));
    let carbs = create_rw_signal(fmt(food.carbs));

    // Нутриенты перечисляются по СПИСКУ, а не по тому, что уже лежит в продукте:
    // строка обязана быть на месте и когда значения ещё нет. Пока форма рисовала
    // только имеющиеся ключи, свежий продукт (и любой после инвалидации кальция)
    // показывал форму БЕЗ кальция — и вписать его руками было негде.
    //
    // Ключи, которых нет в списке, но которые лежат в продукте от прежних сборок,
    // дописываются следом, чтобы правка ничего не теряла. Legacy-ключ «Железо»
    // скрыт: железо живёт в собственных полях ниже, два источника одного числа —
    // это два разных числа.
    let mut custom: Vec<(String, RwSignal<String>)> = crate::services::enrich::nutrient_names()
        .map(|name| {
            let v = food.nutrients.get(name).copied();
            (name.to_string(), create_rw_signal(v.map(fmt).unwrap_or_default()))
        })
        .collect();
    for (k, v) in food.nutrients.iter() {
        if crate::services::enrich::is_hidden_nutrient(k) || custom.iter().any(|(n, _)| n == k) {
            continue;
        }
        custom.push((k.clone(), create_rw_signal(fmt(*v))));
    }
    let custom_save = custom.clone();

    // Iron: amount in mg and the absorbed fraction, shown as a PERCENT (0…100) —
    // a share reads far better than 0.25.
    let iron_mg = create_rw_signal(fmt_opt(food.iron_mg));
    let iron_abs = create_rw_signal(fmt_opt(food.iron_absorption.map(|a| (a * 100.0).round())));

    // Category flags stay TRI-state: `None` means «the model hasn't judged this
    // yet», which is not the same as «no». Untouched flags keep their `None`.
    let f_veg = create_rw_signal(food.is_veg_fruit);
    let f_heme = create_rw_signal(food.is_heme);
    // Мясные признаки правятся здесь же: по ним считаются недельная шкала красного
    // мяса и дневной индикатор колбас, а до сих пор посмотреть их было негде —
    // человек видел «86 г колбасы» и не мог ни проверить, за что продукт сочли
    // колбасой, ни поправить.
    let f_red = create_rw_signal(food.is_red_meat);
    let f_processed = create_rw_signal(food.is_processed_meat);

    // Portal строит разметку повторно, значит всё внутри должно переживать
    // повторный вызов. Обычное замыкание уехало бы в on:click при первом же
    // построении — оборачиваем в Callback, он Copy.
    let save = Callback::new(move |_: web_sys::MouseEvent| {
        let parse = |s: String| -> f64 { s.replace(',', ".").parse().unwrap_or(0.0) };
        let parse_opt = |s: String| -> Option<f64> {
            let s = s.trim().replace(',', ".");
            if s.is_empty() { None } else { s.parse().ok() }
        };
        let name_v = name.get_untracked();
        if name_v.trim().is_empty() {
            return;
        }
        let kc = parse(kcal.get_untracked());
        let pr = parse(protein.get_untracked());
        let ft = parse(fat.get_untracked());
        let cb = parse(carbs.get_untracked());
        // Введённый НОЛЬ сохраняется: «в этом продукте нет клетчатки» — это ответ, а
        // не пропуск, и фоновый проход не должен спрашивать снова (раньше нули
        // выбрасывались, ключ исчезал, и продукт переспрашивался бесконечно).
        //
        // А вот ПУСТОЕ поле — не ноль. Строка теперь рисуется даже когда значения
        // ещё нет (продукт только что заведён или значение стёрла миграция), и
        // записать её нулём значило бы соврать: проход решил бы, что нутриент
        // выяснен, и никогда его не запросил.
        let mut nutrients = BTreeMap::new();
        for (k, sig) in custom_save.iter() {
            let raw = sig.get_untracked();
            if raw.trim().is_empty() {
                continue;
            }
            nutrients.insert(k.clone(), parse(raw));
        }
        // Процент усвоения возвращаем в долю и держим в 0…1: значение вне диапазона
        // молча испортило бы весь недельный счёт железа.
        let absorption = parse_opt(iron_abs.get_untracked())
            .map(|p| (p / 100.0).clamp(0.0, 1.0));
        let ai = AiFoodData {
            nutrients,
            iron_mg: parse_opt(iron_mg.get_untracked()).map(|v| v.max(0.0)),
            iron_absorption: absorption,
            is_veg_fruit: f_veg.get_untracked(),
            is_heme: f_heme.get_untracked(),
            is_red_meat: f_red.get_untracked(),
            is_processed_meat: f_processed.get_untracked(),
        };
        let eid = entry_id.clone();
        spawn_local(async move {
            local::edit_food_for_entry(&eid, name_v, kc, pr, ft, cb, ai).await;
            on_saved.call(());
            on_close.call(());
        });
    });

    let macro_row = |label: String, unit: String, sig: RwSignal<String>| {
        view! {
            <div style="display: flex; align-items: center; padding: 8px 0; border-bottom: 0.5px solid var(--bulma-border-weak);">
                <span class="is-size-6" style="min-width: 90px;">{label}</span>
                <div style="flex: 1;"></div>
                <input type="text" inputmode="decimal"
                    class="is-size-6"
                    style="width: 90px; text-align: right; padding: 4px 8px; border: none; background: var(--bulma-background); color: var(--bulma-text); border-radius: 8px; outline: none;"
                    prop:value=move || sig.get()
                    on:input=move |ev| sig.set(event_target_value(&ev))
                />
                <span class="has-text-grey-light is-size-7" style="margin-left: 6px; min-width: 30px;">{unit}</span>
            </div>
        }
    };

    // Tri-state flag row: a tap sets an explicit yes/no; a flag the model never
    // judged shows «—» and stays unset until touched.
    let flag_row = |label: &'static str, sig: RwSignal<Option<bool>>| {
        view! {
            <div style="display: flex; align-items: center; padding: 8px 0; border-bottom: 0.5px solid var(--bulma-border-weak);">
                <span class="is-size-6" style="flex: 1; min-width: 0;">{label}</span>
                <button
                    attr:data-flag=label
                    class="button is-small"
                    style="min-width: 70px;"
                    on:click=move |_| sig.set(Some(!sig.get_untracked().unwrap_or(false)))>
                    {move || match sig.get() {
                        Some(true) => "да",
                        Some(false) => "нет",
                        None => "—",
                    }}
                </button>
            </div>
        }
    };

    // Строка ВЫВЕДЕННОГО значения: её не правят, потому что править нечего —
    // величина посчитана из состава (или из профиля жира), и любая правка была бы
    // затёрта первым же пересчётом.
    let derived_row = |label: &'static str, value: String, unit: &'static str| {
        view! {
            <div style="display: flex; align-items: center; padding: 8px 0; border-bottom: 0.5px solid var(--bulma-border-weak);">
                <span class="is-size-6" style="flex: 1; min-width: 0;">{label}</span>
                <span attr:data-derived=label class="is-size-6 has-text-weight-semibold">{value}</span>
                <span class="has-text-grey-light is-size-7" style="margin-left: 6px; min-width: 30px;">{unit}</span>
            </div>
        }
    };

    // Жирные кислоты продукта: граммы в 100 г, посчитанные из профиля (доли от
    // собственного жира) — весь состав, а не одна омега-3. До этого профиль
    // собирался, но человеку не показывался вовсе, и проверить, верно ли модель
    // разобрала продукт, было негде: именно так и жил кижуч с занижёнными ПНЖК.
    let acids_own = food.fatty_acids_per_100g();
    let acid_g = move |pick: fn(&api_types::FattyAcids) -> f64| {
        acids_own
            .as_ref()
            .map(|a| format!("{:.2}", pick(a)))
            .unwrap_or_else(|| "—".to_string())
    };

    // Блюдо — функция состава, поэтому вместо флагов у него количества на 100 г.
    // Грузится отдельно: состав и конечный вес лежат в рецепте, а не в продукте.
    let recipe_id = food.recipe_id.clone().filter(|_| food.is_recipe);
    let flags = create_local_resource(
        move || recipe_id.clone(),
        |id| async move {
            match id {
                Some(id) => local::recipe_flags_for(&id).await,
                None => None,
            }
        },
    );
    let is_recipe = food.is_recipe;

    // Portal принимает только `Fn`-замыкание, поэтому строки нутриентов собираем
    // здесь, а не внутри разметки: иначе `custom` уехал бы из окружения и вью стало
    // бы одноразовым.
    let custom_rows = custom
        .into_iter()
        .map(|(k, sig)| {
            let unit = crate::services::enrich::nutrient_unit(&k).to_string();
            macro_row(k, unit, sig)
        })
        .collect_view();

    view! {
      // Portal в <body> — как у полноэкранного просмотра историй. Приложение живёт
      // внутри оболочки с `position: fixed`, а она образует собственный контекст
      // наложения: вложенный overlay не может закрасить нижнюю панель, какой бы
      // z-index ему ни дать. Монтирование в корень документа это снимает.
      <Portal>
        // Полноэкранная СТРАНИЦА, а не модалка. Модалка не годилась: блок «найдено
        // автоматически» вырос, карточка стала выше экрана, и её собственный подвал
        // с кнопками уехал под нижнюю панель навигации — форму нельзя было ни
        // сохранить, ни закрыть. Здесь шапка и подвал закреплены, прокручивается
        // только середина.
        <div attr:data-testid="food-edit-page"
            style="position: fixed; inset: 0; z-index: 90; background: var(--bulma-scheme-main); \
                   display: flex; flex-direction: column;">
            // Шапка: назад + заголовок.
            <div style="flex-shrink: 0; display: flex; align-items: center; gap: 10px; \
                    padding: calc(env(safe-area-inset-top) + 10px) 14px 10px; \
                    border-bottom: 0.5px solid var(--bulma-border-weak);">
                <button attr:data-testid="food-edit-back"
                    attr:aria-label="Назад"
                    class="button is-ghost"
                    style="width: 2.5rem; height: 2.5rem; padding: 0; text-decoration: none; flex-shrink: 0;"
                    on:click=move |_| on_close.call(())>
                    <svg xmlns="http://www.w3.org/2000/svg" width="24" height="24" viewBox="0 0 24 24"
                        fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round"
                        stroke-linejoin="round">
                        <path d="M19 12H5"/><path d="M12 19l-7-7 7-7"/>
                    </svg>
                </button>
                <span class="is-size-5 has-text-weight-semibold" style="flex: 1; min-width: 0;">
                    {move || t("diary.edit_product")}
                </span>
            </div>

            // Прокручиваемая середина.
            <div style="flex: 1; min-height: 0; overflow-y: auto; -webkit-overflow-scrolling: touch; \
                    padding: 14px;">
                <input type="text"
                    class="is-size-6"
                    style="width: 100%; padding: 10px 12px; border: none; border-radius: 10px; background: var(--bulma-background); color: var(--bulma-text); outline: none; box-sizing: border-box; margin-bottom: 12px;"
                    placeholder=t("food_editor.product_name")
                    prop:value=move || name.get()
                    on:input=move |ev| name.set(event_target_value(&ev))
                />
                {macro_row(nutrient_name("Calories").to_string(), unit_label("kcal").to_string(), kcal)}
                {macro_row(nutrient_name("Protein").to_string(), unit_label("g").to_string(), protein)}
                {macro_row(nutrient_name("Fat").to_string(), unit_label("g").to_string(), fat)}
                {macro_row(nutrient_name("Carbs").to_string(), unit_label("g").to_string(), carbs)}

                // ── Всё, что подобрал ИИ — в отдельной рамке ──
                <div attr:data-testid="ai-found-block"
                    style="margin-top: 16px; border: 1px dashed var(--bulma-border); border-radius: 12px; \
                           padding: 10px 12px; background: var(--bulma-scheme-main-bis);">
                    <span class="is-size-7 has-text-weight-semibold">"Найдено автоматически"</span>
                    <p class="is-size-7 has-text-grey" style="margin: 4px 0 6px; line-height: 1.35;">
                        "Эти данные подобрал искусственный интеллект по названию продукта. Он ошибается — если знаете точное значение, исправьте."
                    </p>
                    {custom_rows.clone()}
                    {macro_row("Железо".to_string(), "мг".to_string(), iron_mg)}
                    {macro_row("Усвоение железа".to_string(), "%".to_string(), iron_abs)}
                    // У блюда флагов нет: рагу из говядины с капустой — и мясо, и
                    // овощи разом, и вопрос в том, сколько. Вместо пометок —
                    // количества на 100 г, посчитанные из состава и конечного веса.
                    {if is_recipe {
                        view! {
                            {move || match flags.get() {
                                None => view! {
                                    <div class="is-size-7 has-text-grey" style="padding: 8px 0;">"Считаем из состава…"</div>
                                }.into_view(),
                                Some(None) => view! {
                                    <div class="is-size-7 has-text-grey" style="padding: 8px 0;">
                                        "Состав или конечный вес блюда неизвестны — считать не из чего."
                                    </div>
                                }.into_view(),
                                Some(Some(f)) => {
                                    let num = |v: Option<f64>, dec: usize| v
                                        .map(|x| format!("{x:.*}", dec))
                                        .unwrap_or_else(|| "—".to_string());
                                    // «Улучшает/ухудшает» — про то, куда блюдо тянет
                                    // недельный баланс: выше нормы тянет вверх, ниже —
                                    // вниз. Числа тут намеренно нет: важна сторона.
                                    let balance = match f.unsat_to_sat {
                                        None => "—",
                                        Some(r) if r > crate::services::fats::UNSAT_TO_SAT_MIN => "улучшает",
                                        Some(r) if r < crate::services::fats::UNSAT_TO_SAT_MIN => "ухудшает",
                                        Some(_) => "не влияет",
                                    };
                                    view! {
                                        {derived_row("Овощи и фрукты", num(f.veg_fruit_g, 0), "г")}
                                        {derived_row("Порции гемового железа", num(f.heme_portions, 2), "")}
                                        {derived_row("Баланс жирных кислот", balance.to_string(), "")}
                                        {derived_row("Омега-3 (ЭПК+ДГК)", num(f.epa_dha_g, 2), "г")}
                                    }.into_view()
                                }
                            }}
                        }.into_view()
                    } else {
                        view! {
                            {flag_row("Овощ или фрукт", f_veg)}
                            {flag_row("Источник гемового железа", f_heme)}
                            {flag_row("Красное мясо", f_red)}
                            {flag_row("Мясо глубокой переработки", f_processed)}
                            // Весь состав жира, в граммах на 100 г продукта. Правке
                            // не подлежит: это доли профиля, умноженные на жир, —
                            // менять надо профиль, а не результат.
                            {derived_row("Насыщенные (НЖК)", acid_g(|a| a.sfa_g), "г")}
                            {derived_row("Мононенасыщенные (МНЖК)", acid_g(|a| a.mufa_g), "г")}
                            {derived_row("Полиненасыщенные (ПНЖК)", acid_g(|a| a.pufa_g), "г")}
                            {derived_row("из них омега-3 (ЭПК+ДГК)", acid_g(|a| a.epa_dha_g), "г")}
                        }.into_view()
                    }}
                </div>
            </div>

            // Подвал: закреплён, поэтому «Сохранить» видно при любой длине формы.
            <div style="flex-shrink: 0; display: flex; gap: 10px; padding: 10px 14px; \
                    padding-bottom: calc(env(safe-area-inset-bottom) + 10px); \
                    border-top: 0.5px solid var(--bulma-border-weak);">
                <button class="button is-fullwidth" style="flex: 1;"
                    on:click=move |_| on_close.call(())>{move || t("weight.cancel")}</button>
                <button attr:data-testid="food-edit-save" class="button is-link is-fullwidth" style="flex: 1;"
                    on:click=move |ev| save.call(ev)>{move || t("weight.save")}</button>
            </div>
        </div>
      </Portal>
    }
}
