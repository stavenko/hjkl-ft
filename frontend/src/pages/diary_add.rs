use leptos::*;
use leptos_router::*;
use api_types::*;

use api_types::NutrientSpec;

use crate::components::food_picker::FoodPicker;
use crate::services::{local, sync};
use crate::services::i18n::t;

/// Full-page diary-add flow (route `/diary/add`). Document-scroll page with a
/// "< Дневник" back header, rendering the shared [`FoodPicker`]. On pick it
/// writes the entry (which fires the story "first food" hook internally), pushes
/// sync in the background, and navigates back to the diary. The diary page
/// remounts on return so its resources re-read from IndexedDB.
const PAGE_BG: &str = "background: var(--bulma-background); min-height: 100vh; padding: 0; margin: -0.75rem;";

#[component]
pub fn DiaryAddPage() -> impl IntoView {
    let navigate = use_navigate();

    // Version counter: bump after a draft is created → resources re-read.
    let version = create_rw_signal(0u32);

    let foods_res = create_resource(
        move || version.get(),
        |_| async { local::list_foods().await },
    );
    let goals_res = create_resource(
        move || version.get(),
        |_| async { local::list_goals().await },
    );
    let today_entries_res = create_resource(
        move || version.get(),
        |_| async {
            let today = chrono::Local::now().format("%Y-%m-%d").to_string();
            local::list_diary(&today).await
        },
    );

    let foods = move || foods_res.get().unwrap_or_default();
    let goals = move || goals_res.get().unwrap_or_default();
    let _today_entries = move || today_entries_res.get().unwrap_or_default();

    let custom_nutrients = move || -> Vec<NutrientSpec> {
        goals()
            .into_iter()
            .filter(|g| !matches!(g.nutrient.as_str(), "Calories" | "Protein" | "Fat" | "Carbs"))
            .map(|g| NutrientSpec {
                key: g.key,
                unit_label: g.unit.label().to_string(),
                name: g.nutrient,
            })
            .collect()
    };

    // No diary-wide blocking: a product may already be in today's diary and
    // still be addable. The picker blocks only what was added in THIS session.
    let disabled_ids = Signal::derive(Vec::<String>::new);

    let show_editor = create_rw_signal(false);

    let on_pick = {
        let navigate = navigate.clone();
        Callback::new(move |(food, grams, waste, restaurant): (Food, f64, f64, bool)| {
            let navigate = navigate.clone();
            spawn_local(async move {
                let _entry = local::save_food_to_diary(&food, grams, waste, restaurant).await;
                sync::push_background();
                navigate("/diary", Default::default());
            });
        })
    };

    let on_food_created = Callback::new(move |_food: Food| {
        version.update(|v| *v += 1);
    });

    view! {
        <div style=PAGE_BG>
            <div style="position: sticky; top: 0; z-index: 1; background: var(--bulma-background); display: flex; align-items: center; padding: 12px 16px;">
                <button
                    style="appearance: none; -webkit-appearance: none; border: none; background: none; cursor: pointer; padding: 4px; font: inherit;"
                    class="is-size-5"
                    on:click={ let nav = navigate.clone(); move |_| nav("/diary", Default::default()) }
                >
                    <span class="has-text-link">{move || format!("\u{2039} {}", t("diary_add.back"))}</span>
                </button>
            </div>

            <div style="padding: 0 16px 5rem 16px;">
                <FoodPicker
                    foods=Signal::derive(foods)
                    disabled_ids=disabled_ids
                    goals=Signal::derive(goals)
                    custom_nutrients=Signal::derive(custom_nutrients)
                    allow_waste=true
                    exclude_restaurant=false
                    on_pick=on_pick
                    on_food_created=on_food_created
                    show_editor=show_editor
                />
            </div>
        </div>
    }
}
