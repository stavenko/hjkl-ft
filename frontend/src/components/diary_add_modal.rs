use leptos::*;
use api_types::*;

use super::food_picker_modal::FoodPickerModal;
use crate::services::{local, sync};

#[component]
pub fn DiaryAddModal(
    foods: Signal<Vec<Food>>,
    goals: Signal<Vec<Goal>>,
    today_entries: Signal<Vec<DiaryEntry>>,
    custom_nutrients: Signal<Vec<NutrientSpec>>,
    date: String,
    on_added: Callback<DiaryEntry>,
    on_food_created: Callback<Food>,
    on_close: Callback<()>,
) -> impl IntoView {
    let _ = date;
    // No diary-wide blocking: a product may already be in today's diary and still
    // be addable. The picker blocks only what was added IN THIS dialog session
    // (resets on close), so `disabled_ids` is empty here.
    let _ = today_entries;
    let disabled_ids = Signal::derive(Vec::<String>::new);

    view! {
        <FoodPickerModal
            title="diary_add.title"
            foods=foods
            disabled_ids=disabled_ids
            goals=goals
            custom_nutrients=custom_nutrients
            allow_waste=true
            on_pick=Callback::new(move |(food, grams, waste, restaurant): (Food, f64, f64, bool)| {
                spawn_local(async move {
                    let entry = local::save_food_to_diary(&food, grams, waste, restaurant).await;
                    sync::push_background();
                    on_added.call(entry);
                });
            })
            on_food_created=on_food_created
            on_close=on_close
        />
    }
}
