use leptos::*;
use api_types::*;

use super::food_picker_modal::FoodPickerModal;

#[component]
pub fn AddIngredientModal(
    foods: Signal<Vec<Food>>,
    goals: Signal<Vec<Goal>>,
    custom_nutrients: Signal<Vec<NutrientSpec>>,
    added_food_ids: RwSignal<Vec<String>>,
    on_add: Callback<(Food, f64)>,
    on_food_created: Callback<Food>,
    on_close: Callback<()>,
) -> impl IntoView {
    let disabled_ids = Signal::derive(move || added_food_ids.get());

    view! {
        <FoodPickerModal
            title="add_ingredient.title"
            foods=foods
            disabled_ids=disabled_ids
            goals=goals
            custom_nutrients=custom_nutrients
            exclude_restaurant=true
            on_pick=Callback::new(move |(food, grams, _waste, _restaurant): (Food, f64, f64, bool)| on_add.call((food, grams)))
            on_food_created=on_food_created
            on_close=on_close
        />
    }
}
