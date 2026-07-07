use leptos::*;
use leptos_router::*;
use api_types::*;

use crate::components::food_list_item::FoodListItem;
use crate::components::weight_modal::WeightModal;
use crate::services::{local, sync};
use crate::services::i18n::t;

#[component]
pub fn RecipesPage() -> impl IntoView {
    let search = create_rw_signal(String::new());
    let version = create_rw_signal(0u32);
    // Which finalized recipe's action menu is open (its id), and the "change final
    // weight" modal state: (recipe_id, food_name, current_total_grams).
    let menu_open = create_rw_signal(None::<String>);
    let weight_modal = create_rw_signal(None::<(String, String, f64)>);

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
                        let is_done = recipe.finalized;

                        if is_done {
                            // Finalized: show as FoodListItem with nutrition badges
                            let food = recipe.food_id.as_ref()
                                .and_then(|fid| fs.iter().find(|f| f.id == *fid))
                                .cloned();

                            if let Some(food) = food {
                                // Diary-like row: badges show the WHOLE product's КБЖУ
                                // (grams = total_grams), the final weight is shown, and
                                // the actions hide behind a kebab "⋮" menu (like the diary).
                                let total = recipe.total_grams;
                                let rid_menu = recipe.id.clone();
                                let rid_open = recipe.id.clone();
                                // Copy handles for use inside the `<Show>` menu (whose
                                // content must be `Fn`, so it can't move owned Strings).
                                let sv_id = store_value(recipe.id.clone());
                                let sv_name = store_value(food.name.clone());
                                // Row navigates on tap via on:click (not <a>): a leptos_router
                                // <a> intercepts clicks even on the kebab, so a div lets the
                                // menu's stop_propagation actually suppress navigation.
                                let nid = recipe.id.clone();
                                view! {
                                    <div on:click=move |_| { use_navigate()(&format!("/recipes/{nid}"), Default::default()); }
                                         style="cursor: pointer;">
                                        <FoodListItem food=food goals=Signal::derive(goals) grams=total.unwrap_or(100.0)>
                                            {total.map(|g| view! {
                                                <span class="is-size-7 has-text-grey" style="white-space: nowrap;">
                                                    {format!("{:.0}{}", g, t("common.unit.g"))}
                                                </span>
                                            })}
                                            <div style="position: relative;">
                                                <button
                                                    class="button is-ghost has-text-grey-light"
                                                    style="height: 2.5rem; width: 2.5rem; padding: 0; text-decoration: none;"
                                                    on:click=move |ev| {
                                                        ev.prevent_default();
                                                        ev.stop_propagation();
                                                        let rid = rid_menu.clone();
                                                        menu_open.update(|m| {
                                                            if m.as_deref() == Some(&rid) { *m = None; } else { *m = Some(rid); }
                                                        });
                                                    }
                                                >
                                                    <svg xmlns="http://www.w3.org/2000/svg" width="20" height="20" viewBox="0 0 20 20" fill="currentColor">
                                                        <circle cx="10" cy="4" r="1.6"/>
                                                        <circle cx="10" cy="10" r="1.6"/>
                                                        <circle cx="10" cy="16" r="1.6"/>
                                                    </svg>
                                                </button>
                                                <Show when=move || menu_open.get().as_deref() == Some(&rid_open)>
                                                    <div style="position: absolute; right: 0; top: 100%; z-index: 10; background: var(--bulma-scheme-main); border-radius: 6px; box-shadow: 0 2px 12px rgba(0,0,0,0.15); min-width: 12rem; padding: 0.25rem 0;">
                                                        <button
                                                            class="button is-ghost is-small is-fullwidth"
                                                            style="justify-content: flex-start; text-decoration: none;"
                                                            on:click=move |ev: leptos::ev::MouseEvent| {
                                                                ev.prevent_default();
                                                                ev.stop_propagation();
                                                                menu_open.set(None);
                                                                let cid = sv_id.get_value();
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
                                                        <button
                                                            class="button is-ghost is-small is-fullwidth"
                                                            style="justify-content: flex-start; text-decoration: none;"
                                                            on:click=move |ev: leptos::ev::MouseEvent| {
                                                                ev.prevent_default();
                                                                ev.stop_propagation();
                                                                menu_open.set(None);
                                                                weight_modal.set(Some((sv_id.get_value(), sv_name.get_value(), total.unwrap_or(0.0))));
                                                            }
                                                        >{move || t("recipes.change_weight")}</button>
                                                    </div>
                                                </Show>
                                            </div>
                                        </FoodListItem>
                                    </div>
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

            // Tap-away backdrop: closes an open row menu when tapping outside it.
            {move || menu_open.get().is_some().then(|| view! {
                <div style="position: fixed; inset: 0; z-index: 9;"
                    on:pointerdown=move |_| menu_open.set(None)></div>
            })}

            // "Изменить окончательный вес": enter a new final weight → repriced.
            {move || weight_modal.get().map(|(rid, fname, cur)| {
                view! {
                    <WeightModal
                        food_name=fname
                        current_grams=cur
                        package_weight=None
                        on_save=Callback::new(move |new_w: f64| {
                            let rid = rid.clone();
                            spawn_local(async move {
                                local::change_recipe_weight(&rid, new_w).await;
                                invalidate();
                                sync::push_background();
                            });
                            weight_modal.set(None);
                        })
                        on_close=Callback::new(move |_| weight_modal.set(None))
                    />
                }
            })}
        </div>
    }
}
