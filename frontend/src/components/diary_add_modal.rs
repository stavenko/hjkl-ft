use leptos::*;
use api_types::*;

use super::food_editor::FoodEditor;
use super::food_list_item::FoodListItem;
use crate::services::i18n::t;
use crate::services::{db, local, sync};

#[component]
pub fn DiaryAddModal(
    foods: Signal<Vec<Food>>,
    goals: Signal<Vec<Goal>>,
    today_entries: Signal<Vec<DiaryEntry>>,
    custom_nutrients: Signal<Vec<NutrientSpec>>,
    date: String,
    on_added: Callback<DiaryEntry>,
    on_food_created: Callback<Food>,
    on_close: Callback<()>,
) -> impl IntoView {
    let search = create_rw_signal(String::new());
    let show_editor = create_rw_signal(false);
    let weight_food = create_rw_signal(None::<Food>);
    let grams = create_rw_signal("100".to_string());
    let pending_draft_id = create_rw_signal(None::<String>);
    let drafts_ver = db::version("food_drafts");
    let drafts_res = create_resource(
        move || drafts_ver.get(),
        |_| async { local::list_drafts().await },
    );
    let diary_times_res = create_resource(
        move || db::version("diary").get(),
        |_| async { local::latest_diary_time_per_food().await },
    );

    // (Food, icon, sort_key)
    let filtered = move || {
        let q = search.get().to_lowercase();
        let diary_times = diary_times_res.get().unwrap_or_default();
        let food_ids: std::collections::HashSet<String> =
            foods.get().iter().map(|f| f.id.clone()).collect();

        let mut items: Vec<(Food, &'static str, String)> = Vec::new();

        for f in foods.get() {
            if f.archived { continue; }
            if !q.is_empty() && !f.name.to_lowercase().contains(&q) { continue; }
            let icon = if f.is_recipe { "\u{1f373}" } else { "\u{1f37d}\u{fe0f}" };
            let sort_key = diary_times.get(&f.id).cloned().unwrap_or_else(|| f.created_at.clone());
            items.push((f, icon, sort_key));
        }

        for draft in drafts_res.get().unwrap_or_default() {
            if draft.food_id.is_some() { continue; }
            if food_ids.contains(&draft.id) { continue; }
            let food = draft.to_food();
            if !q.is_empty() && !food.name.to_lowercase().contains(&q) { continue; }
            let sort_key = draft.created_at.clone();
            items.push((food, "\u{270f}\u{fe0f}", sort_key));
        }

        items.sort_by(|a, b| b.2.cmp(&a.2));
        items
    };

    let adjust = move |delta: f64| {
        let cur: f64 = grams.get().parse().unwrap_or(0.0);
        let new = (cur + delta).max(0.0);
        grams.set(format!("{new}"));
    };

    view! {
        <div class="modal is-active" style="z-index: 50;">
            <div class="modal-background" on:click=move |_| on_close.call(())></div>
            <div class="modal-card" style="max-width: 28rem; height: 80vh;">
                <header class="modal-card-head" style="display: block; padding-bottom: 0.5rem;">
                    <div style="display: flex; align-items: center; justify-content: space-between; margin-bottom: 0.5rem;">
                        <p class="modal-card-title">
                            {move || if show_editor.get() { t("diary_add.new_food") } else { t("diary_add.title") }}
                        </p>
                        <button attr:data-testid="diary-add-btn-close" class="delete" on:click=move |_| {
                            if show_editor.get() {
                                show_editor.set(false);
                            } else {
                                on_close.call(());
                            }
                        }></button>
                    </div>
                    <Show when=move || !show_editor.get()>
                        <div style="display: flex; gap: 6px; align-items: center;">
                            <input
                                attr:data-testid="diary-add-input-search"
                                type="text"
                                placeholder=t("diary_add.search_placeholder")
                                class="is-size-6"
                                style="flex: 1; padding: 8px 12px; border: none; border-radius: 10px; background: var(--bulma-background); color: var(--bulma-text); outline: none;"
                                prop:value=move || search.get()
                                on:input=move |ev| search.set(event_target_value(&ev))
                            />
                            <Show when=move || !search.get().is_empty()>
                                <button
                                    attr:data-testid="diary-add-btn-clear-search"
                                    style="background: none; border: none; font-size: 18px; color: var(--bulma-text-weak); cursor: pointer; padding: 4px 8px;"
                                    on:click=move |_| search.set(String::new())
                                >"\u{00d7}"</button>
                            </Show>
                        </div>
                    </Show>
                </header>
                <section class="modal-card-body" style="overflow-y: auto;">
                    <Show when=move || !show_editor.get()>
                        {move || {
                            let list = filtered();
                            if list.is_empty() {
                                view! {
                                    <div style="text-align: center; padding: 32px 0;">
                                        <p class="is-size-6 has-text-grey-light" style="margin-bottom: 16px;">
                                            {move || t("diary_add.nothing_found")}
                                        </p>
                                        <button
                                            attr:data-testid="diary-add-btn-new-food"
                                            class="is-size-6 has-text-link has-text-weight-medium"
                                            style="background: none; border: none; cursor: pointer;"
                                            on:click=move |_| show_editor.set(true)
                                        >{move || t("diary_add.add_new_food")}</button>
                                    </div>
                                }.into_view()
                            } else {
                                view! {
                                    <div>
                                        <For
                                            each=filtered
                                            key=|(f, _, _)| f.id.clone()
                                            children=move |(food, icon, _sort)| {
                                                let food_id = food.id.clone();
                                                let f = food.clone();
                                                let already_added = move || {
                                                    today_entries.get().iter().any(|e| e.food_id == food_id)
                                                };
                                                view! {
                                                    <FoodListItem food=food goals=goals icon=icon>
                                                        <button
                                                            attr:data-testid="diary-add-btn-pick-food"
                                                            class="button is-success has-text-weight-bold"
                                                            style="width: 2.75rem; height: 2.75rem; border-radius: 50%; border: none; font-size: 1.4rem; cursor: pointer;"
                                                            disabled=move || already_added()
                                                            on:click={
                                                                let f = f.clone();
                                                                move |_| {
                                                                    weight_food.set(Some(f.clone()));
                                                                    grams.set("100".into());
                                                                }
                                                            }
                                                        >"+"</button>
                                                    </FoodListItem>
                                                }
                                            }
                                        />
                                        <div style="text-align: center; padding: 16px 0;">
                                            <button
                                                attr:data-testid="diary-add-btn-new-food"
                                                class="is-size-6 has-text-link has-text-weight-medium"
                                            style="background: none; border: none; cursor: pointer;"
                                                on:click=move |_| show_editor.set(true)
                                            >{move || t("diary_add.new_food")}</button>
                                        </div>
                                    </div>
                                }.into_view()
                            }
                        }}
                    </Show>

                    // Food editor form
                    <Show when=move || show_editor.get()>
                        <FoodEditor
                            custom_nutrients=custom_nutrients
                            on_draft=Callback::new(move |(food, d_id): (Food, Option<String>)| {
                                pending_draft_id.set(d_id);
                                on_food_created.call(food.clone());
                                weight_food.set(Some(food));
                                grams.set("100".into());
                                show_editor.set(false);
                            })
                        />
                    </Show>
                </section>
                <footer class="modal-card-foot" style="justify-content: flex-end;">
                    <button attr:data-testid="diary-add-btn-done" class="button is-link" on:click=move |_| on_close.call(())>{move || t("diary_add.done")}</button>
                </footer>
            </div>
        </div>

        // Weight input modal
        {move || {
            weight_food.get().map(|food| {
                let food_name = food.name.clone();
                let food_c = food.clone();
                let pkg = food.package_weight.filter(|w| *w > 0.0);
                view! {
                    <div class="modal is-active" style="z-index: 60;">
                        <div class="modal-background" on:click=move |_| weight_food.set(None)></div>
                        <div class="modal-card" style="max-width: 22rem;">
                            <header class="modal-card-head">
                                <p class="modal-card-title is-size-6">{move || t("diary_add.how_much")}</p>
                                <button attr:data-testid="diary-add-weight-btn-close" class="delete" on:click=move |_| weight_food.set(None)></button>
                            </header>
                            <section class="modal-card-body">
                                <p class="is-size-7 has-text-grey has-text-weight-semibold mb-3"
                                    style="text-transform: uppercase;"
                                >{food_name}</p>

                                <div class="field has-addons has-addons-centered mb-3">
                                    <div class="control is-expanded">
                                        <input
                                            attr:data-testid="diary-add-weight-input-grams"
                                            type="text"
                                            inputmode="decimal"
                                            class="input has-text-centered"
                                            prop:value=move || grams.get()
                                            on:input=move |ev| grams.set(event_target_value(&ev))
                                        />
                                    </div>
                                    <div class="control">
                                        <a class="button is-static">{move || t("common.unit.g")}</a>
                                    </div>
                                </div>

                                <div class="buttons is-centered mb-3">
                                    <button attr:data-testid="diary-add-weight-btn-minus100" type="button" class="button is-small" on:click=move |_| adjust(-100.0)>"-100"</button>
                                    <button attr:data-testid="diary-add-weight-btn-minus10" type="button" class="button is-small" on:click=move |_| adjust(-10.0)>"-10"</button>
                                    <button attr:data-testid="diary-add-weight-btn-plus10" type="button" class="button is-small" on:click=move |_| adjust(10.0)>"+10"</button>
                                    <button attr:data-testid="diary-add-weight-btn-plus100" type="button" class="button is-small" on:click=move |_| adjust(100.0)>"+100"</button>
                                </div>

                                {pkg.map(|pw| {
                                    view! {
                                        <div class="buttons is-centered mb-3">
                                            <button attr:data-testid="diary-add-weight-btn-pkg-minus" type="button" class="button is-small" on:click=move |_| adjust(-pw)>
                                                {format!("-{:.0}g", pw)}
                                            </button>
                                            <button attr:data-testid="diary-add-weight-btn-pkg-exact" type="button" class="button is-small" on:click=move |_| grams.set(format!("{pw}"))>
                                                {format!("={:.0}g", pw)}
                                            </button>
                                            <button attr:data-testid="diary-add-weight-btn-pkg-plus" type="button" class="button is-small" on:click=move |_| adjust(pw)>
                                                {format!("+{:.0}g", pw)}
                                            </button>
                                        </div>
                                    }
                                })}
                            </section>
                            <footer class="modal-card-foot" style="justify-content: flex-end;">
                                <button attr:data-testid="diary-add-weight-btn-cancel" class="button" on:click=move |_| weight_food.set(None)>{move || t("diary_add.cancel")}</button>
                                <button attr:data-testid="diary-add-weight-btn-confirm" class="button is-link"
                                    on:click={
                                        let food = food_c.clone();
                                        move |_| {
                                            let g: f64 = grams.get_untracked().parse().unwrap_or(0.0);
                                            if g <= 0.0 { return; }
                                            let food = food.clone();
                                            weight_food.set(None);
                                            let d_id = pending_draft_id.get_untracked();
                                            spawn_local(async move {
                                                let entry = local::save_food_to_diary(&food, g).await;
                                                if let Some(d_id) = d_id {
                                                    local::set_draft_food_id(&d_id, &entry.food_id).await;
                                                }
                                                sync::push_background();
                                                on_added.call(entry);
                                            });
                                        }
                                    }
                                >{move || t("diary_add.add")}</button>
                            </footer>
                        </div>
                    </div>
                }
            })
        }}

    }
}
