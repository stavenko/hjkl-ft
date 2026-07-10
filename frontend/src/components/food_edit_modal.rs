use std::collections::BTreeMap;

use leptos::*;
use api_types::Food;

use crate::services::i18n::{t, nutrient_name, unit_label};
use crate::services::local;

/// Edit the product (name + КБЖУ + existing custom nutrients) behind a diary
/// entry. Save applies copy-on-write via `local::edit_food_for_entry` (edits in
/// place if the product is used only by this entry, else clones it). Opened from
/// the diary row long-press "Изменить".
#[component]
pub fn FoodEditModal(
    food: Food,
    entry_id: String,
    on_saved: Callback<()>,
    on_close: Callback<()>,
) -> impl IntoView {
    let fmt = |v: f64| if v == 0.0 { String::new() } else { format!("{v}") };

    let name = create_rw_signal(food.name.clone());
    let kcal = create_rw_signal(fmt(food.kcal));
    let protein = create_rw_signal(fmt(food.protein));
    let fat = create_rw_signal(fmt(food.fat));
    let carbs = create_rw_signal(fmt(food.carbs));
    // Existing custom nutrients: (name, value-string signal).
    let custom: Vec<(String, RwSignal<String>)> = food
        .nutrients
        .iter()
        .map(|(k, v)| (k.clone(), create_rw_signal(fmt(*v))))
        .collect();
    let custom_save = custom.clone();

    let save = move |_| {
        let parse = |s: String| -> f64 { s.replace(',', ".").parse().unwrap_or(0.0) };
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
        let eid = entry_id.clone();
        spawn_local(async move {
            local::edit_food_for_entry(&eid, name_v, kc, pr, ft, cb, nutrients).await;
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
                    {custom.into_iter().map(|(k, sig)| {
                        let unit = crate::services::enrich::nutrient_unit(&k).to_string();
                        macro_row(k, unit, sig)
                    }).collect_view()}
                </section>
                <footer class="modal-card-foot" style="justify-content: flex-end;">
                    <button class="button" on:click=move |_| on_close.call(())>{move || t("weight.cancel")}</button>
                    <button class="button is-link" on:click=save>{move || t("weight.save")}</button>
                </footer>
            </div>
        </div>
    }
}
