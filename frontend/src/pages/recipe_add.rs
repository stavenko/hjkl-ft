use leptos::*;
use leptos_router::*;
use api_types::*;

use api_types::NutrientSpec;

use crate::components::food_picker::FoodPicker;
use crate::services::i18n::t;
use crate::services::{local, sync};

/// Full-page "add ingredient to recipe" flow (route `/recipes/:id/add`) — the
/// page-with-navigation counterpart of the old in-recipe modal, mirroring
/// `/diary/add`. Renders the shared [`FoodPicker`]; on pick it appends the
/// ingredient to the recipe and navigates back to the recipe page.
const PAGE_BG: &str = "background: var(--bulma-background); min-height: 100vh; padding: 0; margin: -0.75rem;";

#[component]
pub fn RecipeAddPage() -> impl IntoView {
    let params = use_params_map();
    let navigate = use_navigate();
    let recipe_id = move || params.get().get("id").cloned().unwrap_or_default();

    // Bump after a draft/new food is created → resources re-read.
    let version = create_rw_signal(0u32);

    let foods_res = create_resource(move || version.get(), |_| async { local::list_foods().await });
    let goals_res = create_resource(move || version.get(), |_| async { local::list_goals().await });
    let recipe_res = create_resource(
        move || (recipe_id(), version.get()),
        |(id, _)| async move { local::get_recipe(&id).await },
    );

    let foods = move || foods_res.get().unwrap_or_default();
    let goals = move || goals_res.get().unwrap_or_default();

    let custom_nutrients = move || -> Vec<NutrientSpec> {
        goals()
            .into_iter()
            .filter(|g| !matches!(g.nutrient.as_str(), "Calories" | "Protein" | "Fat" | "Carbs"))
            .map(|g| NutrientSpec { key: g.key, unit_label: g.unit.label().to_string(), name: g.nutrient })
            .collect()
    };

    // Ingredients already in the recipe → shown as a disabled checkmark.
    let disabled_ids = Signal::derive(move || {
        recipe_res
            .get()
            .flatten()
            .map(|r| r.ingredients.iter().map(|i| i.food_id.clone()).collect::<Vec<_>>())
            .unwrap_or_default()
    });

    let show_editor = create_rw_signal(false);

    let on_pick = {
        let navigate = navigate.clone();
        Callback::new(move |(food, grams, _waste, _restaurant): (Food, f64, f64, bool)| {
            let navigate = navigate.clone();
            let id = recipe_id();
            spawn_local(async move {
                let _ = local::add_ingredient_to_recipe(&food, grams, &id).await;
                sync::push_background();
                navigate(&format!("/recipes/{id}"), Default::default());
            });
        })
    };

    let on_food_created = Callback::new(move |_food: Food| {
        version.update(|v| *v += 1);
    });

    let back_label = move || {
        recipe_res
            .get()
            .flatten()
            .map(|r| r.name)
            .unwrap_or_else(|| t("common.back").to_string())
    };

    view! {
        <div style=PAGE_BG>
            <div style="position: sticky; top: 0; z-index: 1; background: var(--bulma-background); display: flex; align-items: center; padding: 12px 16px;">
                <button
                    style="appearance: none; -webkit-appearance: none; border: none; background: none; cursor: pointer; padding: 4px; font: inherit;"
                    class="is-size-5"
                    on:click={ let nav = navigate.clone(); move |_| nav(&format!("/recipes/{}", recipe_id()), Default::default()) }
                >
                    <span class="has-text-link">{move || format!("\u{2039} {}", back_label())}</span>
                </button>
            </div>

            <div style="padding: 0 16px 5rem 16px;">
                <FoodPicker
                    foods=Signal::derive(foods)
                    disabled_ids=disabled_ids
                    goals=Signal::derive(goals)
                    custom_nutrients=Signal::derive(custom_nutrients)
                    allow_waste=false
                    exclude_restaurant=true
                    on_pick=on_pick
                    on_food_created=on_food_created
                    show_editor=show_editor
                />
            </div>
        </div>
    }
}
