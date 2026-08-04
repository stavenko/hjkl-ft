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

    // Existing custom nutrients: (name, value-string signal). The legacy «Железо»
    // key that old builds wrote into this map stays hidden — iron now lives in its
    // own fields below, and showing both would be two sources for one number.
    let custom: Vec<(String, RwSignal<String>)> = food
        .nutrients
        .iter()
        .filter(|(k, _)| !crate::services::enrich::is_hidden_nutrient(k))
        .map(|(k, v)| (k.clone(), create_rw_signal(fmt(*v))))
        .collect();
    let custom_save = custom.clone();

    // Iron: amount in mg and the absorbed fraction, shown as a PERCENT (0…100) —
    // a share reads far better than 0.25.
    let iron_mg = create_rw_signal(fmt_opt(food.iron_mg));
    let iron_abs = create_rw_signal(fmt_opt(food.iron_absorption.map(|a| (a * 100.0).round())));

    // Category flags stay TRI-state: `None` means «the model hasn't judged this
    // yet», which is not the same as «no». Untouched flags keep their `None`.
    let f_snack = create_rw_signal(food.is_snack);
    let f_liquid = create_rw_signal(food.is_liquid_cal);
    let f_veg = create_rw_signal(food.is_veg_fruit);
    let f_egg = create_rw_signal(food.is_egg);
    let f_meat = create_rw_signal(food.is_red_meat);

    let save = move |_| {
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
        let mut nutrients = BTreeMap::new();
        for (k, sig) in custom_save.iter() {
            let v = parse(sig.get_untracked());
            if v != 0.0 {
                nutrients.insert(k.clone(), v);
            }
        }
        // Процент усвоения возвращаем в долю и держим в 0…1: значение вне диапазона
        // молча испортило бы весь недельный счёт железа.
        let absorption = parse_opt(iron_abs.get_untracked())
            .map(|p| (p / 100.0).clamp(0.0, 1.0));
        let ai = AiFoodData {
            nutrients,
            iron_mg: parse_opt(iron_mg.get_untracked()).map(|v| v.max(0.0)),
            iron_absorption: absorption,
            is_snack: f_snack.get_untracked(),
            is_liquid_cal: f_liquid.get_untracked(),
            is_veg_fruit: f_veg.get_untracked(),
            is_egg: f_egg.get_untracked(),
            is_red_meat: f_meat.get_untracked(),
        };
        let eid = entry_id.clone();
        spawn_local(async move {
            local::edit_food_for_entry(&eid, name_v, kc, pr, ft, cb, ai).await;
            on_saved.call(());
            on_close.call(());
        });
    };

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

    view! {
        <div class="modal is-active" style="z-index: 70;">
            <div class="modal-background" on:click=move |_| on_close.call(())></div>
            <div class="modal-card" style="max-width: 26rem;">
                <header class="modal-card-head">
                    <p class="modal-card-title is-size-6">{move || t("diary.edit_product")}</p>
                </header>
                <section class="modal-card-body">
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
                        {custom.into_iter().map(|(k, sig)| {
                            let unit = crate::services::enrich::nutrient_unit(&k).to_string();
                            macro_row(k, unit, sig)
                        }).collect_view()}
                        {macro_row("Железо".to_string(), "мг".to_string(), iron_mg)}
                        {macro_row("Усвоение железа".to_string(), "%".to_string(), iron_abs)}
                        {flag_row("Низкокалорийный перекус", f_snack)}
                        {flag_row("Жидкие калории", f_liquid)}
                        {flag_row("Овощ или фрукт", f_veg)}
                        {flag_row("Яйца", f_egg)}
                        {flag_row("Красное мясо", f_meat)}
                    </div>
                </section>
                <footer class="modal-card-foot" style="justify-content: flex-end;">
                    <button class="button" on:click=move |_| on_close.call(())>{move || t("weight.cancel")}</button>
                    <button class="button is-link" on:click=save>{move || t("weight.save")}</button>
                </footer>
            </div>
        </div>
    }
}
