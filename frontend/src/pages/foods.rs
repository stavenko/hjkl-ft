use leptos::*;
use api_types::*;

use crate::components::food_modal::FoodModal;
use crate::services::{local, sync};
use crate::services::i18n::t;
use api_types::NutrientSpec;

#[component]
pub fn FoodsPage() -> impl IntoView {
    let foods = create_rw_signal(Vec::<Food>::new());
    let show_modal = create_rw_signal(false);
    let custom_nutrients = create_rw_signal(Vec::<NutrientSpec>::new());

    create_effect(move |_| {
        spawn_local(async move {
            foods.set(local::list_foods().await);
            let specs: Vec<NutrientSpec> = local::list_goals().await
                .into_iter()
                .filter(|g| !matches!(g.nutrient.as_str(), "Calories" | "Protein" | "Fat" | "Carbs"))
                .map(|g| NutrientSpec { key: g.key, unit_label: g.unit.label().to_string(), name: g.nutrient })
                .collect();
            custom_nutrients.set(specs);
        });
    });

    let toggle_archive = move |id: String, archived: bool| {
        spawn_local(async move {
            if let Some(updated) = local::archive_food(&id, archived).await {
                foods.update(|f| {
                    if let Some(food) = f.iter_mut().find(|x| x.id == id) {
                        food.archived = updated.archived;
                    }
                });
                sync::push_background();
            }
        });
    };

    let on_created = Callback::new(move |food: Food| {
        foods.update(|f| f.push(food));
        show_modal.set(false);
    });

    let on_close = Callback::new(move |_| show_modal.set(false));

    let visible_foods = move || {
        foods.get().into_iter().filter(|f| !f.archived).collect::<Vec<_>>()
    };

    view! {
        <div>
            <div class="level mb-4">
                <div class="level-left">
                    <h1 class="title is-4">{t("foods.title")}</h1>
                </div>
                <div class="level-right">
                    <button
                        class="button is-link"
                        on:click=move |_| show_modal.set(true)
                    >
                        {t("foods.add")}
                    </button>
                </div>
            </div>

            <div>
                <For
                    each=visible_foods
                    key=|f| f.id.clone()
                    children=move |food| {
                        let id = food.id.clone();
                        view! {
                            <div class="box py-3 px-4 mb-2">
                                <div class="level is-mobile">
                                    <div class="level-left">
                                        <span class="has-text-weight-medium">{&food.name}</span>
                                    </div>
                                    <div class="level-right">
                                        <span class="is-size-7 has-text-grey mr-4">
                                            {format!("{:.0} {}", food.kcal, t("common.unit.kcal"))}
                                        </span>
                                        <span class="is-size-7 has-text-grey mr-4">
                                            {format!("P {:.1}", food.protein)}
                                        </span>
                                        <span class="is-size-7 has-text-grey mr-4">
                                            {format!("F {:.1}", food.fat)}
                                        </span>
                                        <span class="is-size-7 has-text-grey mr-4">
                                            {format!("C {:.1}", food.carbs)}
                                        </span>
                                        <button
                                            class="button is-small is-ghost has-text-grey-light"
                                            title=t("foods.archive")
                                            on:click={
                                                let id = id.clone();
                                                move |_| toggle_archive(id.clone(), true)
                                            }
                                        >
                                            <span class="icon is-small">
                                                <svg xmlns="http://www.w3.org/2000/svg" width="16" height="16" viewBox="0 0 20 20" fill="currentColor">
                                                    <path fill-rule="evenodd" d="M9 2a1 1 0 00-.894.553L7.382 4H4a1 1 0 000 2v10a2 2 0 002 2h8a2 2 0 002-2V6a1 1 0 100-2h-3.382l-.724-1.447A1 1 0 0011 2H9zM7 8a1 1 0 012 0v6a1 1 0 11-2 0V8zm5-1a1 1 0 00-1 1v6a1 1 0 102 0V8a1 1 0 00-1-1z" clip-rule="evenodd" />
                                                </svg>
                                            </span>
                                        </button>
                                    </div>
                                </div>
                            </div>
                        }
                    }
                />
            </div>

            <Show when=move || show_modal.get()>
                <FoodModal
                    custom_nutrients=custom_nutrients.into()
                    on_created=on_created
                    on_close=on_close
                />
            </Show>
        </div>
    }
}
