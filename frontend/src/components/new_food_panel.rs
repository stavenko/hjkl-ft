use leptos::*;
use api_types::*;

use super::food_editor::FoodEditor;
use super::food_list_item::FoodListItem;
use crate::services::local;
use crate::services::i18n::t;

/// Accordion panel: "New food" form + "History" of FoodDrafts.
/// Calls `on_select` with a Food when user creates new or picks from history.
#[component]
pub fn NewFoodPanel(
    custom_nutrients: Signal<Vec<NutrientSpec>>,
    goals: Signal<Vec<Goal>>,
    today_entries: Signal<Vec<DiaryEntry>>,
    on_select: Callback<Food>,
) -> impl IntoView {
    let accordion = create_rw_signal("form".to_string());
    let drafts_version = create_rw_signal(0u32);
    let drafts_res = create_resource(
        move || drafts_version.get(),
        |_| async { local::list_drafts().await },
    );
    let drafts = move || drafts_res.get().unwrap_or_default();

    let acc_cls = move |s: &str| {
        if accordion.get() == s {
            "has-text-weight-semibold has-text-link"
        } else {
            "has-text-grey"
        }
    };

    fn draft_to_food(d: &FoodDraft) -> Food {
        Food {
            id: d.id.clone(),
            name: d.name.clone(),
            kcal: d.kcal,
            protein: d.protein,
            fat: d.fat,
            carbs: d.carbs,
            nutrients: d.nutrients.clone(),
            package_weight: d.package_weight,
            is_recipe: false,
            recipe_id: None,
            archived: false,
            created_at: d.created_at.clone(),
            updated_at: String::new(),
        }
    }

    view! {
        // Accordion headers
        <div style="display: flex; gap: 1.5rem; margin-bottom: 0.75rem; cursor: pointer;">
            <span class=move || acc_cls("form") style="cursor: pointer;"
                on:click=move |_| accordion.set("form".into())>{t("new_food.title")}</span>
            <span class=move || acc_cls("history") style="cursor: pointer;"
                on:click=move |_| accordion.set("history".into())>{t("new_food.history")}</span>
        </div>

        // Form section
        <Show when=move || accordion.get() == "form">
            <FoodEditor
                custom_nutrients=custom_nutrients
                on_draft=Callback::new(move |food: Food| {
                    let food_c = food.clone();
                    spawn_local(async move {
                        let _draft = local::save_draft(&food_c).await;
                        drafts_version.update(|v| *v += 1);
                    });
                    on_select.call(food);
                })
            />
        </Show>

        // History section
        <Show when=move || accordion.get() == "history">
            <div style="overflow-y: auto; max-height: 50vh;">
                {move || {
                    drafts().into_iter().map(|draft| {
                        let draft_food_id = draft.food_id.clone();
                        let draft_id = draft.id.clone();
                        let display_food = draft_to_food(&draft);
                        let select_food = draft_to_food(&draft);
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
                                    class="button is-success is-rounded"
                                    style="height: 2.75rem; width: 2.75rem; padding: 0; font-size: 1.4rem; font-weight: 700;"
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
    }
}
