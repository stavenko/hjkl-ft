use leptos::*;
use api_types::*;

use super::food_editor::FoodEditor;
use super::food_list_item::FoodListItem;
use crate::services::{db, local};
use crate::services::i18n::t;

#[component]
pub fn NewFoodPanel(
    custom_nutrients: Signal<Vec<NutrientSpec>>,
    goals: Signal<Vec<Goal>>,
    today_entries: Signal<Vec<DiaryEntry>>,
    on_select: Callback<Food>,
) -> impl IntoView {
    let accordion = create_rw_signal("form".to_string());
    let drafts_ver = db::version("food_drafts");
    let drafts_res = create_resource(
        move || drafts_ver.get(),
        |_| async { local::list_drafts().await },
    );
    let drafts = move || drafts_res.get().unwrap_or_default();


    view! {
        // Segmented control tabs
        <div style="display: flex; background: var(--bulma-background); border-radius: 8px; padding: 2px; margin-bottom: 12px;">
            <button
                class="is-size-7 has-text-weight-medium"
                style=move || format!(
                    "flex: 1; padding: 6px 0; border: none; border-radius: 7px; cursor: pointer; {}",
                    if accordion.get() == "form" {
                        "background: var(--bulma-scheme-main); color: var(--bulma-text-strong); box-shadow: 0 1px 3px rgba(0,0,0,0.1);"
                    } else {
                        "background: transparent; color: var(--bulma-text-weak);"
                    }
                )
                on:click=move |_| accordion.set("form".into())
            >{move || t("new_food.title")}</button>
            <button
                class="is-size-7 has-text-weight-medium"
                style=move || format!(
                    "flex: 1; padding: 6px 0; border: none; border-radius: 7px; cursor: pointer; {}",
                    if accordion.get() == "history" {
                        "background: var(--bulma-scheme-main); color: var(--bulma-text-strong); box-shadow: 0 1px 3px rgba(0,0,0,0.1);"
                    } else {
                        "background: transparent; color: var(--bulma-text-weak);"
                    }
                )
                on:click=move |_| accordion.set("history".into())
            >{move || t("new_food.history")}</button>
        </div>

        // Fixed-height content area
        <div style="min-height: 320px;">
            <Show when=move || accordion.get() == "form">
                <FoodEditor
                    custom_nutrients=custom_nutrients
                    on_draft=Callback::new(move |(food, _d_id): (Food, Option<String>)| {
                        on_select.call(food);
                    })
                />
            </Show>

            <Show when=move || accordion.get() == "history">
                <div style="overflow-y: auto; max-height: 50vh;">
                    {move || {
                        drafts().into_iter().map(|draft| {
                            let draft_food_id = draft.food_id.clone();
                            let draft_id = draft.id.clone();
                            let display_food = draft.to_food();
                            let select_food = draft.to_food();
                            let already_added = move || {
                                if let Some(ref fid) = draft_food_id {
                                    today_entries.get().iter().any(|e| e.food_id == *fid)
                                } else {
                                    false
                                }
                            };
                            view! {
                                <FoodListItem food=display_food goals=goals>
                                    <button
                                        class="button is-success has-text-weight-bold"
                                        style="width: 2.75rem; height: 2.75rem; border-radius: 50%; border: none; font-size: 1.4rem; cursor: pointer;"
                                        disabled=move || already_added()
                                        on:click={
                                            let f = select_food.clone();
                                            move |_| on_select.call(f.clone())
                                        }
                                    >"+"</button>
                                </FoodListItem>
                            }
                        }).collect::<Vec<_>>()
                    }}
                </div>
            </Show>
        </div>
    }
}
