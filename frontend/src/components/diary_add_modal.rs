use leptos::*;
use api_types::*;

use super::food_list_item::FoodListItem;
use super::new_food_panel::NewFoodPanel;
use crate::services::i18n::t;
use crate::services::{local, sync};

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
    let mode = create_rw_signal("search".to_string());
    let weight_food = create_rw_signal(None::<Food>);
    let grams = create_rw_signal("100".to_string());

    let filtered = move || {
        let q = search.get().to_lowercase();
        foods
            .get()
            .into_iter()
            .filter(|f| !f.archived && (q.is_empty() || f.name.to_lowercase().contains(&q)))
            .collect::<Vec<_>>()
    };

    let adjust = move |delta: f64| {
        let cur: f64 = grams.get().parse().unwrap_or(0.0);
        let new = (cur + delta).max(0.0);
        grams.set(format!("{new}"));
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
                        <p class="modal-card-title">{t("diary_add.title")}</p>
                        <button class="delete" on:click=move |_| on_close.call(())></button>
                    </div>
                    <div class="buttons has-addons mb-2">
                        <button type="button" class=move || mode_cls("search")
                            on:click=move |_| mode.set("search".into())>{t("diary_add.search")}</button>
                        <button type="button" class=move || mode_cls("new")
                            on:click=move |_| mode.set("new".into())>{t("diary_add.new")}</button>
                    </div>
                    <Show when=move || mode.get() == "search">
                        <div class="field has-addons" style="margin-bottom: 0;">
                            <div class="control is-expanded">
                                <input
                                    type="text"
                                    placeholder=t("diary_add.search_placeholder")
                                    class="input is-small"
                                    prop:value=move || search.get()
                                    on:input=move |ev| search.set(event_target_value(&ev))
                                />
                            </div>
                            <div class="control">
                                <button
                                    class="button is-small is-light"
                                    on:click=move |_| search.set(String::new())
                                >"\u{00d7}"</button>
                            </div>
                        </div>
                    </Show>
                </header>
                <section class="modal-card-body" style="overflow-y: auto;">
                    // Search mode
                    <Show when=move || mode.get() == "search">
                        <div>
                            <For
                                each=filtered
                                key=|f| f.id.clone()
                                children=move |food| {
                                    let food_id = food.id.clone();
                                    let f = food.clone();
                                    let already_added = move || {
                                        today_entries.get().iter().any(|e| e.food_id == food_id)
                                    };
                                    view! {
                                        <FoodListItem food=food goals=goals>
                                            <button
                                                class="button is-success is-rounded"
                                                style="height: 2.75rem; width: 2.75rem; padding: 0; font-size: 1.4rem; font-weight: 700;"
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
                        </div>
                    </Show>

                    // New mode: form + draft history
                    <Show when=move || mode.get() == "new">
                        <NewFoodPanel
                            custom_nutrients=custom_nutrients
                            goals=goals
                            today_entries=today_entries
                            on_select=Callback::new(move |food: Food| {
                                on_food_created.call(food.clone());
                                weight_food.set(Some(food));
                                grams.set("100".into());
                            })

                        />
                    </Show>
                </section>
                <footer class="modal-card-foot" style="justify-content: flex-end;">
                    <button class="button is-link" on:click=move |_| on_close.call(())>{t("diary_add.done")}</button>
                </footer>
            </div>
        </div>

        // Weight input modal — on top of search
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
                                <p class="modal-card-title is-size-6">{t("diary_add.how_much")}</p>
                                <button class="delete" on:click=move |_| weight_food.set(None)></button>
                            </header>
                            <section class="modal-card-body">
                                <p class="is-size-7 has-text-grey has-text-weight-semibold mb-3"
                                    style="text-transform: uppercase;"
                                >{food_name}</p>

                                <div class="field has-addons has-addons-centered mb-3">
                                    <div class="control is-expanded">
                                        <input
                                            type="text"
                                            inputmode="decimal"
                                            class="input has-text-centered"
                                            prop:value=move || grams.get()
                                            on:input=move |ev| grams.set(event_target_value(&ev))
                                        />
                                    </div>
                                    <div class="control">
                                        <a class="button is-static">{t("common.unit.g")}</a>
                                    </div>
                                </div>

                                <div class="buttons is-centered mb-3">
                                    <button type="button" class="button is-small is-light" on:click=move |_| adjust(-100.0)>"-100"</button>
                                    <button type="button" class="button is-small is-light" on:click=move |_| adjust(-10.0)>"-10"</button>
                                    <button type="button" class="button is-small is-light" on:click=move |_| adjust(10.0)>"+10"</button>
                                    <button type="button" class="button is-small is-light" on:click=move |_| adjust(100.0)>"+100"</button>
                                </div>

                                {pkg.map(|pw| {
                                    view! {
                                        <div class="buttons is-centered mb-3">
                                            <button type="button" class="button is-small is-light" on:click=move |_| adjust(-pw)>
                                                {format!("-{:.0}g", pw)}
                                            </button>
                                            <button type="button" class="button is-small is-light" on:click=move |_| grams.set(format!("{pw}"))>
                                                {format!("={:.0}g", pw)}
                                            </button>
                                            <button type="button" class="button is-small is-light" on:click=move |_| adjust(pw)>
                                                {format!("+{:.0}g", pw)}
                                            </button>
                                        </div>
                                    }
                                })}
                            </section>
                            <footer class="modal-card-foot" style="justify-content: flex-end;">
                                <button class="button" on:click=move |_| weight_food.set(None)>{t("diary_add.cancel")}</button>
                                <button class="button is-link"
                                    on:click={
                                        let food = food_c.clone();
                                        move |_| {
                                            let g: f64 = grams.get_untracked().parse().unwrap_or(0.0);
                                            if g <= 0.0 { return; }
                                            let food = food.clone();
                                            weight_food.set(None);
                                            spawn_local(async move {
                                                let entry = local::save_food_to_diary(&food, g).await;
                                                sync::push_background();
                                                on_added.call(entry);
                                            });
                                        }
                                    }
                                >{t("diary_add.add")}</button>
                            </footer>
                        </div>
                    </div>
                }
            })
        }}

    }
}
