use leptos::*;
use leptos_router::*;
use api_types::*;

use crate::components::food_list_item::FoodListItem;
use crate::services::{local, sync};
use crate::services::i18n::t;

#[component]
pub fn RecipesPage() -> impl IntoView {
    let search = create_rw_signal(String::new());
    let version = create_rw_signal(0u32);

    let recipes_res = create_resource(
        move || version.get(),
        |_| async { local::list_recipes().await },
    );
    let foods_res = create_resource(
        move || version.get(),
        |_| async { local::list_foods().await },
    );
    let goals_res = create_resource(
        move || version.get(),
        |_| async { local::list_goals().await },
    );

    let recipes = move || recipes_res.get().unwrap_or_default();
    let foods = move || foods_res.get().unwrap_or_default();
    let goals = move || goals_res.get().unwrap_or_default();

    let invalidate = move || version.update(|v| *v += 1);

    let on_create = move |_| {
        spawn_local(async move {
            let recipe = local::new_recipe("").await;
            let id = recipe.id.clone();
            invalidate();
            sync::push_background();
            let navigate = use_navigate();
            navigate(&format!("/recipes/{id}"), Default::default());
        });
    };

    let filtered = move || {
        let q = search.get().to_lowercase();
        let rs = recipes();
        // Hide finalized recipe if an in-progress clone with the same name exists
        let in_progress_names: Vec<String> = rs.iter()
            .filter(|r| !r.finalized)
            .map(|r| r.name.clone())
            .collect();
        rs.into_iter().filter(|r| {
            let matches_search = q.is_empty() || r.name.to_lowercase().contains(&q);
            let hidden = r.finalized && in_progress_names.contains(&r.name);
            matches_search && !hidden
        }).collect::<Vec<_>>()
    };

    view! {
        <div>
            <div style="display: flex; align-items: center; justify-content: space-between; margin-bottom: 1rem;">
                <h1 class="title is-4" style="margin-bottom: 0;">{move || t("recipes.title")}</h1>
                <button attr:data-testid="recipes-btn-new" class="button is-link" on:click=on_create>{move || t("recipes.new")}</button>
            </div>

            <div class="field mb-4">
                <div class="control">
                    <input
                        attr:data-testid="recipes-input-search"
                        type="text"
                        placeholder=t("recipes.search_placeholder")
                        class="input is-small"
                        prop:value=move || search.get()
                        on:input=move |ev| search.set(event_target_value(&ev))
                    />
                </div>
            </div>

            <div>
                {move || {
                    let fs = foods();
                    let gs = goals();
                    filtered().into_iter().map(|recipe| {
                        let id = recipe.id.clone();
                        let id_nav = recipe.id.clone();
                        let id_cook = recipe.id.clone();
                        let is_done = recipe.finalized;

                        if is_done {
                            // Finalized: show as FoodListItem with nutrition badges
                            let food = recipe.food_id.as_ref()
                                .and_then(|fid| fs.iter().find(|f| f.id == *fid))
                                .cloned();

                            if let Some(food) = food {
                                view! {
                                    <a href=format!("/recipes/{id}") style="text-decoration: none; color: inherit;">
                                        <FoodListItem food=food goals=Signal::derive(goals)>
                                            <button
                                                class="button is-small"
                                                style="white-space: nowrap;"
                                                on:click=move |ev| {
                                                    ev.prevent_default();
                                                    ev.stop_propagation();
                                                    let cid = id_cook.clone();
                                                    spawn_local(async move {
                                                        if let Some(new_recipe) = local::clone_recipe(&cid).await {
                                                            let new_id = new_recipe.id.clone();
                                                            invalidate();
                                                            sync::push_background();
                                                            let navigate = use_navigate();
                                                            navigate(&format!("/recipes/{new_id}"), Default::default());
                                                        }
                                                    });
                                                }
                                            >{move || t("recipes.cook_again")}</button>
                                        </FoodListItem>
                                    </a>
                                }.into_view()
                            } else {
                                view! {
                                    <a href=format!("/recipes/{id}") style="text-decoration: none; color: inherit;">
                                        <div style="padding: 0.5rem 0; border-bottom: 1px solid var(--bulma-border-weak);">
                                            <span class="is-size-6 has-text-weight-medium">{&recipe.name}</span>
                                            <span class="tag is-success is-light ml-2">{move || t("recipes.complete")}</span>
                                        </div>
                                    </a>
                                }.into_view()
                            }
                        } else {
                            // In progress: simple row with status
                            view! {
                                <a href=format!("/recipes/{id_nav}") style="text-decoration: none; color: inherit;">
                                    <div style="display: flex; align-items: center; padding: 0.5rem 0; border-bottom: 1px solid var(--bulma-border-weak);">
                                        <span class="is-size-6 has-text-weight-medium" style="flex: 1;">{&recipe.name}</span>
                                        <span class="tag is-warning is-light">{move || t("recipes.in_progress")}</span>
                                    </div>
                                </a>
                            }.into_view()
                        }
                    }).collect::<Vec<_>>()
                }}
            </div>
        </div>
    }
}
