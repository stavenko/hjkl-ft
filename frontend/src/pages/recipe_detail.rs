use leptos::*;
use leptos_router::*;
use api_types::*;

use crate::components::add_ingredient_modal::AddIngredientModal;
use crate::components::food_list_item::FoodListItem;
use crate::components::weight_modal::WeightModal;
use crate::services::{local, sync};
use crate::services::i18n::t;

#[component]
pub fn RecipeDetailPage() -> impl IntoView {
    let params = use_params_map();
    let recipe = create_rw_signal(None::<Recipe>);
    let foods = create_rw_signal(Vec::<Food>::new());
    let recipe_name = create_rw_signal(String::new());

    let goals = create_rw_signal(Vec::<Goal>::new());
    let custom_nutrients = create_rw_signal(Vec::<api_types::NutrientSpec>::new());
    let show_add_modal = create_rw_signal(false);
    let show_finalize = create_rw_signal(false);
    let final_weight = create_rw_signal(String::new());

    let weight_modal = create_rw_signal(None::<(String, String, f64, Option<f64>)>);
    let added_food_ids = create_rw_signal(Vec::<String>::new());

    create_effect(move |_| {
        let id = params.get().get("id").cloned().unwrap_or_default();
        if id.is_empty() { return; }
        spawn_local(async move {
            if let Some(r) = local::get_recipe(&id).await {
                recipe_name.set(r.name.clone());
                let ids: Vec<String> = r.ingredients.iter().map(|i| i.food_id.clone()).collect();
                added_food_ids.set(ids);
                recipe.set(Some(r));
            }
            foods.set(local::list_foods().await);
            let all_goals = local::list_goals().await;
            goals.set(all_goals.clone());
            let specs: Vec<api_types::NutrientSpec> = all_goals.into_iter()
                .filter(|g| !matches!(g.nutrient.as_str(), "Calories" | "Protein" | "Fat" | "Carbs"))
                .map(|g| api_types::NutrientSpec { key: g.key, unit_label: g.unit.label().to_string(), name: g.nutrient })
                .collect();
            custom_nutrients.set(specs);
        });
    });

    let save_name = move |_| {
        let r = recipe.get_untracked();
        let r = match r { Some(r) => r, None => return };
        let name = recipe_name.get_untracked();
        if name.is_empty() || name == r.name { return; }
        let id = r.id.clone();
        let notes = r.notes.clone();
        spawn_local(async move {
            if let Some(updated) = local::change_recipe_name(&id, &name).await {
                recipe.set(Some(updated));
                sync::push_background();
            }
        });
    };

    // Add a food (existing or just-created) as an ingredient with the chosen grams.
    let add_ingredient = move |food: Food, grams: f64| {
        let r = recipe.get_untracked();
        let r = match r { Some(r) => r, None => return };
        let recipe_id = r.id.clone();
        added_food_ids.update(|ids| if !ids.contains(&food.id) { ids.push(food.id.clone()); });
        spawn_local(async move {
            let ing = local::add_ingredient_to_recipe(&food, grams, &recipe_id).await;
            recipe.update(|r| {
                if let Some(r) = r { r.ingredients.push(ing); }
            });
            foods.set(local::list_foods().await);
            sync::push_background();
        });
    };

    let refresh_foods = move || {
        spawn_local(async move { foods.set(local::list_foods().await); });
    };

    let update_grams = move |ing_id: String, new_val: String| {
        let grams: f64 = match new_val.parse() {
            Ok(v) => v,
            Err(_) => return,
        };
        spawn_local(async move {
            if let Some(updated) = local::update_ingredient(&ing_id, grams).await {
                recipe.update(|r| {
                    if let Some(r) = r {
                        if let Some(ing) = r.ingredients.iter_mut().find(|i| i.id == ing_id) {
                            ing.grams = updated.grams;
                        }
                    }
                });
                sync::push_background();
            }
        });
    };

    let remove_ingredient = move |ing_id: String, food_id: String| {
        spawn_local(async move {
            local::remove_ingredient(&ing_id).await;
            recipe.update(|r| {
                if let Some(r) = r {
                    r.ingredients.retain(|i| i.id != ing_id);
                }
            });
            added_food_ids.update(|ids| ids.retain(|id| id != &food_id));
            sync::push_background();
        });
    };

    let on_finalize = move |ev: leptos::ev::SubmitEvent| {
        ev.prevent_default();
        let r = recipe.get_untracked();
        let r = match r { Some(r) => r, None => return };
        let total_grams: f64 = match final_weight.get_untracked().parse() {
            Ok(v) if v > 0.0 => v,
            _ => return,
        };
        let recipe_id = r.id.clone();
        spawn_local(async move {
            if let Some(_food) = local::finish_recipe(&recipe_id, total_grams).await {
                if let Some(updated) = local::get_recipe(&recipe_id).await {
                    recipe.set(Some(updated));
                }
                show_finalize.set(false);
                sync::push_background();
            }
        });
    };

    let food_name = move |food_id: &str| -> String {
        foods.get().iter().find(|f| f.id == food_id).map(|f| f.name.clone()).unwrap_or_else(|| food_id.to_string())
    };

    let total_ingredient_weight = move || -> f64 {
        recipe.get().map(|r| r.ingredients.iter().map(|i| i.grams).sum()).unwrap_or(0.0)
    };

    view! {
        <Show
            when=move || recipe.get().is_some()
            fallback=|| view! { <p class="has-text-grey">{move || t("recipe.loading")}</p> }
        >
            {move || {
                let r = recipe.get().unwrap();
                let is_finalized = r.finalized;
                view! {
                    <div>
                        // Header
                        <div class="mb-5">
                            <a attr:data-testid="recipe-detail-link-back" href="/recipes" class="is-size-7 has-text-grey">{move || t("recipe.back")}</a>
                            {if is_finalized {
                                view! {
                                    <h1 class="title is-4" style="margin-top: 0.5rem;">{&r.name}</h1>
                                }.into_view()
                            } else {
                                view! {
                                    <input
                                        attr:data-testid="recipe-detail-input-name"
                                        type="text"
                                        class="input is-medium"
                                        style="border: none; border-bottom: 1px solid transparent; box-shadow: none; background: transparent; font-weight: bold; width: 100%; padding-left: 0; margin-top: 0.5rem;"
                                        prop:value=move || recipe_name.get()
                                        on:input=move |ev| recipe_name.set(event_target_value(&ev))
                                        on:blur=save_name
                                    />
                                }.into_view()
                            }}
                        </div>

                        // Ingredients list
                        <div class="mb-4">
                            {move || {
                                let r = recipe.get().unwrap();
                                let fs = foods.get();
                                r.ingredients.iter().map(|ing| {
                                    let food = fs.iter().find(|f| f.id == ing.food_id).cloned();
                                    let ing_id = ing.id.clone();
                                    let ing_id2 = ing.id.clone();
                                    let fid = ing.food_id.clone();
                                    let fid2 = ing.food_id.clone();
                                    let g = ing.grams;
                                    if let Some(food) = food {
                                        view! {
                                            <FoodListItem food=food goals=goals.into() grams=g>
                                                {if !is_finalized {
                                                    view! {
                                                        <button
                                                            attr:data-testid="recipe-detail-btn-edit-weight"
                                                            class="button is-ghost is-small has-text-link"
                                                            style="height: auto; text-decoration: none;"
                                                            on:click={
                                                                let id = ing_id.clone();
                                                                let fid = fid.clone();
                                                                move |_| {
                                                                    let fname = food_name(&fid);
                                                                    let pkg_weight = foods.get_untracked().iter()
                                                                        .find(|f| f.id == fid)
                                                                        .and_then(|f| f.package_weight)
                                                                        .filter(|w| *w > 0.0);
                                                                    weight_modal.set(Some((id.clone(), fname, g, pkg_weight)));
                                                                }
                                                            }
                                                        >
                                                            <span class="is-size-7">{move || format!("{g:.0}{}", t("common.unit.g"))}</span>
                                                        </button>
                                                        <button
                                                            attr:data-testid="recipe-detail-btn-remove-ingredient"
                                                            class="button is-ghost has-text-grey-light"
                                                            style="height: 2.5rem; width: 2.5rem; padding: 0; text-decoration: none;"
                                                            on:click={
                                                                let id = ing_id2.clone();
                                                                let fid = fid2.clone();
                                                                move |_| remove_ingredient(id.clone(), fid.clone())
                                                            }
                                                        >
                                                            <svg xmlns="http://www.w3.org/2000/svg" width="20" height="20" viewBox="0 0 20 20" fill="currentColor">
                                                                <path fill-rule="evenodd" d="M9 2a1 1 0 00-.894.553L7.382 4H4a1 1 0 000 2v10a2 2 0 002 2h8a2 2 0 002-2V6a1 1 0 100-2h-3.382l-.724-1.447A1 1 0 0011 2H9zM7 8a1 1 0 012 0v6a1 1 0 11-2 0V8zm5-1a1 1 0 00-1 1v6a1 1 0 102 0V8a1 1 0 00-1-1z" clip-rule="evenodd" />
                                                            </svg>
                                                        </button>
                                                    }.into_view()
                                                } else {
                                                    view! {
                                                        <span class="is-size-7 has-text-grey">{move || format!("{g:.0}{}", t("common.unit.g"))}</span>
                                                    }.into_view()
                                                }}
                                            </FoodListItem>
                                        }.into_view()
                                    } else {
                                        view! {
                                            <div style="padding: 0.5rem 0; border-bottom: 1px solid var(--bulma-border-weak);">
                                                <span class="is-size-7 has-text-grey">{move || t("recipe.unknown_food")}</span>
                                            </div>
                                        }.into_view()
                                    }
                                }).collect::<Vec<_>>()
                            }}
                        </div>

                        // Nutrition summary
                        {move || {
                            let r = recipe.get().unwrap();
                            let fs = foods.get();
                            let gs = goals.get();
                            let mut total_kcal = 0.0_f64;
                            let mut total_protein = 0.0_f64;
                            let mut total_fat = 0.0_f64;
                            let mut total_carbs = 0.0_f64;
                            let mut custom_totals = std::collections::BTreeMap::<String, f64>::new();
                            for ing in &r.ingredients {
                                if let Some(f) = fs.iter().find(|f| f.id == ing.food_id) {
                                    let factor = ing.grams / 100.0;
                                    total_kcal += f.kcal * factor;
                                    total_protein += f.protein * factor;
                                    total_fat += f.fat * factor;
                                    total_carbs += f.carbs * factor;
                                    for (k, v) in &f.nutrients {
                                        *custom_totals.entry(k.clone()).or_default() += v * factor;
                                    }
                                }
                            }
                            let has_per100 = r.total_grams.filter(|g| *g > 0.0).is_some();
                            let scale = r.total_grams.filter(|g| *g > 0.0).map(|g| 100.0 / g).unwrap_or(0.0);
                            let custom_goals: Vec<_> = gs.iter()
                                .filter(|g| !matches!(g.nutrient.as_str(), "Calories" | "Protein" | "Fat" | "Carbs"))
                                .collect();
                            let grid_style = if has_per100 {
                                "display: grid; grid-template-columns: auto 1fr 1fr; gap: 0.15rem 1rem;"
                            } else {
                                "display: grid; grid-template-columns: auto 1fr; gap: 0.15rem 1rem;"
                            };
                            let header_style = if has_per100 {
                                "display: grid; grid-template-columns: auto 1fr 1fr; gap: 0.15rem 1rem; margin-bottom: 0.25rem;"
                            } else {
                                ""
                            };
                            view! {
                                <div class="box py-3 px-4 mb-4">
                                    <p class="is-size-7 has-text-weight-semibold mb-3">
                                        {move || format!("{} ({:.0}{})", t("recipe.nutrients_whole"), total_ingredient_weight(), t("common.unit.g"))}
                                    </p>
                                    <div style=grid_style>
                                        {has_per100.then(|| view! {
                                            <span class="is-size-7"></span>
                                            <span class="is-size-7 has-text-grey has-text-weight-semibold">{move || t("recipe.whole_dish")}</span>
                                            <span class="is-size-7 has-text-grey has-text-weight-semibold">{move || t("recipe.per_100g")}</span>
                                        })}
                                        {gs.iter().map(|goal| {
                                            let (val, raw_unit) = match goal.nutrient.as_str() {
                                                "Calories" => (total_kcal, "kcal"),
                                                "Protein" => (total_protein, "g"),
                                                "Fat" => (total_fat, "g"),
                                                "Carbs" => (total_carbs, "g"),
                                                custom => (custom_totals.get(custom).copied().unwrap_or(0.0), goal.unit.label()),
                                            };
                                            let unit = crate::services::i18n::unit_label(raw_unit);
                                            let name = if matches!(goal.nutrient.as_str(), "Calories" | "Protein" | "Fat" | "Carbs") {
                                                crate::services::i18n::nutrient_name(&goal.nutrient).to_string()
                                            } else {
                                                goal.nutrient.clone()
                                            };
                                            view! {
                                                <span class="is-size-7 has-text-grey">{name}</span>
                                                <span class="is-size-7 has-text-weight-medium">{format!("{:.2} {}", val, unit)}</span>
                                                {has_per100.then(|| view! {
                                                    <span class="is-size-7 has-text-weight-medium">{format!("{:.2} {}", val * scale, unit)}</span>
                                                })}
                                            }
                                        }).collect::<Vec<_>>()}
                                    </div>
                                    <p class="is-size-7 has-text-grey mt-3">
                                        {move || t("recipe.other_nutrients_hint")} " "
                                        <a attr:data-testid="recipe-detail-link-settings" href="/settings">{move || t("recipe.settings_link")}</a>
                                    </p>
                                </div>
                            }
                        }}

                        // Buttons
                        <Show when=move || !is_finalized>
                            <div class="buttons">
                                <button
                                    attr:data-testid="recipe-detail-btn-add-ingredient"
                                    class="button is-link"
                                    on:click=move |_| show_add_modal.set(true)
                                >{move || t("recipe.add_ingredient")}</button>
                                <button
                                    attr:data-testid="recipe-detail-btn-finalize"
                                    class="button is-success"
                                    on:click=move |_| show_finalize.set(true)
                                >{move || t("recipe.finalize")}</button>
                            </div>
                        </Show>

                        // Weight modal
                        {move || {
                            weight_modal.get().map(|(ing_id, fname, grams, pkg_w)| {
                                view! {
                                    <WeightModal
                                        food_name=fname
                                        current_grams=grams
                                        package_weight=pkg_w
                                        on_save=Callback::new({
                                            let id = ing_id.clone();
                                            move |new_grams: f64| {
                                                update_grams(id.clone(), format!("{new_grams}"));
                                                weight_modal.set(None);
                                            }
                                        })
                                        on_close=Callback::new(move |_| weight_modal.set(None))
                                    />
                                }
                            })
                        }}

                        // Add ingredient modal
                        <Show when=move || show_add_modal.get()>
                            <AddIngredientModal
                                foods=foods.into()
                                goals=goals.into()
                                custom_nutrients=custom_nutrients.into()
                                added_food_ids=added_food_ids
                                on_add=Callback::new(move |(food, grams): (Food, f64)| {
                                    add_ingredient(food, grams);
                                })
                                on_food_created=Callback::new(move |_food: Food| {
                                    refresh_foods();
                                })
                                on_close=Callback::new(move |_| show_add_modal.set(false))
                            />
                        </Show>

                        // Finalize modal
                        <Show when=move || show_finalize.get()>
                            <div class="modal is-active">
                                <div class="modal-background" on:click=move |_| show_finalize.set(false)></div>
                                <div class="modal-card" style="max-width: 24rem;">
                                    <header class="modal-card-head">
                                        <p class="modal-card-title">{move || t("recipe.finalize_title")}</p>
                                        <button attr:data-testid="recipe-detail-btn-finalize-close" class="delete" on:click=move |_| show_finalize.set(false)></button>
                                    </header>
                                    <form on:submit=on_finalize>
                                        <section class="modal-card-body">
                                            <p class="is-size-7 has-text-grey mb-3">
                                                {move || t("recipe.total_weight")} " "
                                                <span class="has-text-weight-semibold">{move || format!("{:.0}{}", total_ingredient_weight(), t("common.unit.g"))}</span>
                                            </p>
                                            <div class="field has-addons">
                                                <div class="control is-expanded">
                                                    <input
                                                        attr:data-testid="recipe-detail-input-final-weight"
                                                        type="text"
                                                        inputmode="decimal"
                                                        placeholder={move || format!("{:.0}", total_ingredient_weight())}
                                                        class="input"
                                                        prop:value=move || final_weight.get()
                                                        on:input=move |ev| final_weight.set(event_target_value(&ev))
                                                    />
                                                </div>
                                                <div class="control">
                                                    <a class="button is-static">{move || t("common.unit.g")}</a>
                                                </div>
                                            </div>
                                        </section>
                                        <footer class="modal-card-foot">
                                            <div class="buttons">
                                                <button attr:data-testid="recipe-detail-btn-finalize-submit" type="submit" class="button is-success">{move || t("recipe.finalize")}</button>
                                                <button attr:data-testid="recipe-detail-btn-finalize-cancel" type="button" class="button" on:click=move |_| show_finalize.set(false)>{move || t("common.cancel")}</button>
                                            </div>
                                        </footer>
                                    </form>
                                </div>
                            </div>
                        </Show>
                    </div>
                }
            }}
        </Show>
    }
}
