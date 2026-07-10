use leptos::*;
use api_types::*;

use super::food_editor::FoodEditor;
use super::food_list_item::FoodListItem;
use super::waste_field::WasteField;
use super::restaurant_field::RestaurantField;
use crate::services::i18n::t;
use crate::services::{db, local};

/// Shared, presentational food picker: search input, filtered+paginated food
/// list with a "+" pick action, "new food" via the editor, and the grams /
/// weight sub-step (a small in-component overlay). Knows nothing about modal
/// chrome or page chrome — the caller supplies the list, the disabled set and
/// what happens when a (food, grams, waste, restaurant) is confirmed.
///
/// `show_editor` is owned by the caller so the surrounding chrome (e.g. a modal
/// title) can react to whether the new-food editor is open.
#[component]
pub fn FoodPicker(
    foods: Signal<Vec<Food>>,
    /// Food ids that are already added — shown as a disabled checkmark.
    disabled_ids: Signal<Vec<String>>,
    goals: Signal<Vec<Goal>>,
    custom_nutrients: Signal<Vec<NutrientSpec>>,
    /// Show the "didn't eat it whole" waste field and the "restaurant food"
    /// checkbox in the grams step (diary only).
    #[prop(default = false)]
    allow_waste: bool,
    /// Hide restaurant-flagged foods from the list (recipe ingredients only).
    #[prop(default = false)]
    exclude_restaurant: bool,
    on_pick: Callback<(Food, f64, f64, bool)>,
    on_food_created: Callback<Food>,
    /// Whether the new-food editor is shown. Owned by the caller so chrome can
    /// react (title swap, hide the search row, redirect tap-outside).
    show_editor: RwSignal<bool>,
) -> impl IntoView {
    // Foods picked during THIS picker session. A food is shown as added (and its
    // "+" disabled) only if it's in `disabled_ids` (e.g. recipe ingredients) OR
    // it was picked in this session. This signal lives with the picker instance,
    // so it resets automatically when the picker is unmounted and recreated.
    let picked = create_rw_signal(std::collections::HashSet::<String>::new());

    let search = create_rw_signal(String::new());
    let drafts_ver = db::version("food_drafts");
    let drafts_res = create_resource(move || drafts_ver.get(), |_| async { local::list_drafts().await });
    // Recipes that were cooked again (superseded_by set) — their finished food is
    // hidden from the picker, so only the newest version of a re-cooked dish shows.
    let recipes_ver = db::version("recipes");
    let recipes_res = create_resource(move || recipes_ver.get(), |_| async { local::list_recipes().await });
    let diary_times_res = create_resource(
        move || db::version("diary").get(),
        |_| async { local::latest_diary_time_per_food().await },
    );

    // foods (incl. recipes) + uncommitted drafts, most-recently-used first.
    let items = Signal::derive(move || {
        let q = search.get().to_lowercase();
        let diary_times = diary_times_res.get().unwrap_or_default();
        let food_ids: std::collections::HashSet<String> =
            foods.get().iter().map(|f| f.id.clone()).collect();

        // Finished foods of recipes that were cooked again (have a successor) — the
        // previous version drops out of search. Explicit `superseded_by` field, no
        // name-matching; also hides duplicates that predate the field.
        let superseded_food_ids: std::collections::HashSet<String> = recipes_res
            .get()
            .unwrap_or_default()
            .into_iter()
            .filter(|r| r.superseded_by.is_some())
            .filter_map(|r| r.food_id)
            .collect();

        let mut list: Vec<(Food, &'static str, String)> = Vec::new();
        for f in foods.get() {
            if f.archived { continue; }
            if exclude_restaurant && f.is_restaurant { continue; }
            // Skip the finished food of a superseded (re-cooked) recipe.
            if superseded_food_ids.contains(&f.id) { continue; }
            if !q.is_empty() && !f.name.to_lowercase().contains(&q) { continue; }
            let icon = if f.is_recipe { "\u{1f373}" } else { "\u{1f37d}\u{fe0f}" };
            let sort_key = diary_times.get(&f.id).cloned().unwrap_or_else(|| f.created_at.clone());
            list.push((f, icon, sort_key));
        }
        for draft in drafts_res.get().unwrap_or_default() {
            if draft.food_id.is_some() { continue; }
            if food_ids.contains(&draft.id) { continue; }
            let food = draft.to_food();
            if !q.is_empty() && !food.name.to_lowercase().contains(&q) { continue; }
            list.push((food, "\u{270f}\u{fe0f}", draft.created_at.clone()));
        }
        list.sort_by(|a, b| b.2.cmp(&a.2));
        list.into_iter().map(|(f, icon, _)| (f, icon)).collect::<Vec<_>>()
    });

    // Paginate: show PAGE at a time, "show more" adds another PAGE. Resets to one
    // page whenever the search query changes. Keeps the list short (so it fits
    // without an awkward inner scroll) while staying fully reachable.
    const PAGE: usize = 7;
    let limit = create_rw_signal(PAGE);
    create_effect(move |_| {
        let _ = search.get();
        limit.set(PAGE);
    });
    let visible = Signal::derive(move || {
        let all = items.get();
        let n = limit.get().min(all.len());
        all.into_iter().take(n).collect::<Vec<_>>()
    });

    let weight_food = create_rw_signal(None::<Food>);
    let grams = create_rw_signal("100".to_string());
    let waste = create_rw_signal(String::new());
    let restaurant = create_rw_signal(false);
    let pending_draft_id = create_rw_signal(None::<String>);

    let adjust = move |delta: f64| {
        let cur: f64 = grams.get().replace(',', ".").parse().unwrap_or(0.0);
        grams.set(format!("{}", (cur + delta).max(0.0)));
    };

    view! {
        // Search row — hidden while the new-food editor is open.
        <Show when=move || !show_editor.get()>
            <div style="display: flex; gap: 6px; align-items: center; margin-bottom: 0.75rem;">
                <input
                    attr:data-testid="diary-add-input-search"
                    type="text"
                    placeholder=t("diary_add.search_placeholder")
                    class="is-size-6"
                    style="flex: 1; padding: 8px 12px; border: 1px solid var(--bulma-border); border-radius: 10px; background: var(--bulma-scheme-main); color: var(--bulma-text); outline: none;"
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

        <Show when=move || !show_editor.get()>
            {move || {
                if items.get().is_empty() {
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
                                each=move || visible.get()
                                key=|(f, _)| f.id.clone()
                                children=move |(food, icon)| {
                                    let fid = food.id.clone();
                                    let f = food.clone();
                                    let is_added = Signal::derive(move || disabled_ids.get().contains(&fid) || picked.get().contains(&fid));
                                    view! {
                                        <FoodListItem food=food goals=goals icon=icon>
                                            <button
                                                attr:data-testid="diary-add-btn-pick-food"
                                                class="button is-success has-text-weight-bold"
                                                style="width: 2.75rem; height: 2.75rem; border-radius: 50%; border: none; font-size: 1.4rem; cursor: pointer;"
                                                disabled=move || is_added.get()
                                                on:click={
                                                    let f = f.clone();
                                                    move |_| {
                                                        pending_draft_id.set(None);
                                                        restaurant.set(f.is_restaurant);
                                                        weight_food.set(Some(f.clone()));
                                                        grams.set("100".into());
                                                        waste.set(String::new());
                                                    }
                                                }
                                            >{move || if is_added.get() { "\u{2713}" } else { "+" }}</button>
                                        </FoodListItem>
                                    }
                                }
                            />
                            // "Show more N/Total products" — only when the filter
                            // returns more than what's currently shown. Each tap
                            // reveals another PAGE.
                            {move || {
                                let total = items.get().len();
                                let shown = limit.get();
                                (total > shown).then(|| {
                                    let next = (total - shown).min(PAGE);
                                    view! {
                                        <div style="text-align: center; padding: 8px 0;">
                                            <button
                                                attr:data-testid="diary-add-btn-more"
                                                class="button is-light is-fullwidth is-small"
                                                on:click=move |_| limit.update(|l| *l += PAGE)
                                            >{format!("{} {}/{} {}", t("diary_add.more"), next, total, t("diary_add.products"))}</button>
                                        </div>
                                    }
                                })
                            }}
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

        // Recreated on each open (via .then, not <Show>) so the name
        // field is freshly seeded from the current search query.
        {move || show_editor.get().then(|| view! {
            <FoodEditor
                custom_nutrients=custom_nutrients
                initial_name=search.get_untracked()
                on_draft=Callback::new(move |(food, d_id): (Food, Option<String>)| {
                    pending_draft_id.set(d_id);
                    on_food_created.call(food.clone());
                    weight_food.set(Some(food));
                    grams.set("100".into());
                    waste.set(String::new());
                    restaurant.set(false);
                    show_editor.set(false);
                })
            />
        })}

        // Grams sub-step (small in-component overlay).
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
                            </header>
                            <section class="modal-card-body">
                                <p class="is-size-7 has-text-grey has-text-weight-semibold mb-3" style="text-transform: uppercase;">{food_name}</p>
                                <div class="field has-addons has-addons-centered mb-3">
                                    <div class="control is-expanded">
                                        <input
                                            attr:data-testid="diary-add-weight-input-grams"
                                            type="text"
                                            inputmode="decimal"
                                            class="input has-text-centered"
                                            prop:value=move || grams.get()
                                            on:input=move |ev| grams.set(event_target_value(&ev))
                                            // Clear the default 100 on focus so a new weight can be typed immediately.
                                            on:focus=move |_| if grams.get_untracked() == "100" { grams.set(String::new()); }
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
                                {pkg.map(|pw| view! {
                                    <div class="buttons is-centered mb-3">
                                        <button attr:data-testid="diary-add-weight-btn-pkg-minus" type="button" class="button is-small" on:click=move |_| adjust(-pw)>{format!("-{:.0}g", pw)}</button>
                                        <button attr:data-testid="diary-add-weight-btn-pkg-exact" type="button" class="button is-small" on:click=move |_| grams.set(format!("{pw}"))>{format!("={:.0}g", pw)}</button>
                                        <button attr:data-testid="diary-add-weight-btn-pkg-plus" type="button" class="button is-small" on:click=move |_| adjust(pw)>{format!("+{:.0}g", pw)}</button>
                                    </div>
                                })}
                                {allow_waste.then(|| view! {
                                    <WasteField grams=Signal::derive(move || grams.get().replace(',', ".").parse().unwrap_or(0.0)) waste=waste />
                                    <RestaurantField value=restaurant />
                                })}
                            </section>
                            <footer class="modal-card-foot" style="justify-content: flex-end;">
                                <button attr:data-testid="diary-add-weight-btn-cancel" class="button" on:click=move |_| weight_food.set(None)>{move || t("diary_add.cancel")}</button>
                                <button attr:data-testid="diary-add-weight-btn-confirm" class="button is-link"
                                    on:click={
                                        let food = food_c.clone();
                                        move |_| {
                                            let g: f64 = grams.get_untracked().replace(',', ".").parse().unwrap_or(0.0);
                                            if g <= 0.0 { return; }
                                            let food = food.clone();
                                            weight_food.set(None);
                                            // Mark as picked in this picker session so it can't be
                                            // added twice until recreated.
                                            picked.update(|s| { s.insert(food.id.clone()); });
                                            // Link the draft to the saved food so it doesn't linger.
                                            if let Some(d_id) = pending_draft_id.get_untracked() {
                                                let fid = food.id.clone();
                                                spawn_local(async move { local::set_draft_food_id(&d_id, &fid).await; });
                                            }
                                            on_pick.call((food, g, waste.get_untracked().replace(',', ".").parse().unwrap_or(0.0), restaurant.get_untracked()));
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
