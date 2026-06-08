use leptos::*;
use api_types::*;

use super::food_list_item::FoodListItem;
use super::new_food_panel::NewFoodPanel;
use crate::services::i18n::t;

#[component]
pub fn AddIngredientModal(
    foods: Signal<Vec<Food>>,
    goals: Signal<Vec<Goal>>,
    today_entries: Signal<Vec<DiaryEntry>>,
    custom_nutrients: Signal<Vec<NutrientSpec>>,
    added_food_ids: RwSignal<Vec<String>>,
    on_add: Callback<String>,
    on_food_created: Callback<Food>,
    on_close: Callback<()>,
) -> impl IntoView {
    let mode = create_rw_signal("search".to_string());
    let food_search = create_rw_signal(String::new());

    let filtered_foods = move || {
        let q = food_search.get().to_lowercase();
        let added = added_food_ids.get();
        foods.get().into_iter().filter(|f| {
            (q.is_empty() || f.name.to_lowercase().contains(&q)) && !f.is_recipe && !f.archived
        }).map(move |f| {
            let is_added = added.contains(&f.id);
            (f, is_added)
        }).collect::<Vec<_>>()
    };

    let mode_cls = move |m: &str| {
        if mode.get() == m {
            "button is-small is-link is-selected"
        } else {
            "button is-small"
        }
    };

    view! {
        <div class="modal is-active">
            <div class="modal-background" on:click=move |_| on_close.call(())></div>
            <div class="modal-card" style="max-width: 28rem; max-height: 80vh;">
                <header class="modal-card-head" style="display: block; padding-bottom: 0.5rem;">
                    <div style="display: flex; align-items: center; justify-content: space-between; margin-bottom: 0.5rem;">
                        <p class="modal-card-title">{t("add_ingredient.title")}</p>
                        <button class="delete" on:click=move |_| on_close.call(())></button>
                    </div>
                    <div class="buttons has-addons mb-2">
                        <button type="button" class=move || mode_cls("search")
                            on:click=move |_| mode.set("search".into())>{t("add_ingredient.search")}</button>
                        <button type="button" class=move || mode_cls("new")
                            on:click=move |_| mode.set("new".into())>{t("add_ingredient.new")}</button>
                    </div>
                    <Show when=move || mode.get() == "search">
                        <div class="field has-addons" style="margin-bottom: 0;">
                            <div class="control is-expanded">
                                <input
                                    type="text"
                                    placeholder=t("add_ingredient.search_placeholder")
                                    class="input is-small"
                                    prop:value=move || food_search.get()
                                    on:input=move |ev| food_search.set(event_target_value(&ev))
                                />
                            </div>
                            <div class="control">
                                <button class="button is-small is-light"
                                    on:click=move |_| food_search.set(String::new())
                                >"\u{00d7}"</button>
                            </div>
                        </div>
                    </Show>
                </header>
                <section class="modal-card-body" style="overflow-y: auto;">
                    // Search mode
                    <Show when=move || mode.get() == "search">
                        <div>
                            {move || {
                                filtered_foods().into_iter().map(|(food, is_added)| {
                                    let fid = food.id.clone();
                                    view! {
                                        <FoodListItem food=food goals=goals>
                                            <button
                                                class="button is-success is-rounded"
                                                style="height: 2.75rem; width: 2.75rem; padding: 0; font-size: 1.4rem; font-weight: 700;"
                                                disabled=is_added
                                                on:click={
                                                    let id = fid.clone();
                                                    move |_| on_add.call(id.clone())
                                                }
                                            >{if is_added { "\u{2713}" } else { "+" }}</button>
                                        </FoodListItem>
                                    }
                                }).collect::<Vec<_>>()
                            }}
                        </div>
                    </Show>

                    // New mode
                    <Show when=move || mode.get() == "new">
                        <NewFoodPanel
                            custom_nutrients=custom_nutrients
                            goals=goals
                            today_entries=today_entries
                            on_select=Callback::new(move |food: Food| {
                                on_food_created.call(food);
                            })

                        />
                    </Show>
                </section>
                <footer class="modal-card-foot" style="justify-content: flex-end;">
                    <button class="button is-link" on:click=move |_| on_close.call(())>{t("add_ingredient.done")}</button>
                </footer>
            </div>
        </div>
    }
}
