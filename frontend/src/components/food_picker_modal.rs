use leptos::*;
use api_types::*;

use super::food_picker::FoodPicker;
use crate::services::i18n::t;

/// Thin modal wrapper around [`FoodPicker`]: modal chrome (title, scrolling
/// body, footer close button) plus tap-outside handling. Used by the diary-add
/// and recipe-ingredient flows — the caller supplies the list, the disabled set
/// and what happens when a (food, grams, waste, restaurant) is confirmed.
#[component]
pub fn FoodPickerModal(
    /// i18n key for the list-mode title.
    title: &'static str,
    foods: Signal<Vec<Food>>,
    /// Food ids that are already added — shown as a disabled checkmark.
    disabled_ids: Signal<Vec<String>>,
    goals: Signal<Vec<Goal>>,
    custom_nutrients: Signal<Vec<NutrientSpec>>,
    /// Show the "didn't eat it whole" waste field and the "restaurant food"
    /// checkbox in the grams modal (diary only).
    #[prop(default = false)]
    allow_waste: bool,
    /// Hide restaurant-flagged foods from the list (recipe ingredients only).
    #[prop(default = false)]
    exclude_restaurant: bool,
    on_pick: Callback<(Food, f64, f64, bool)>,
    on_food_created: Callback<Food>,
    on_close: Callback<()>,
) -> impl IntoView {
    // Owned here so the title can swap and tap-outside can act as a back button
    // out of the new-food editor.
    let show_editor = create_rw_signal(false);

    view! {
        <div class="modal is-active" style="z-index: 50;">
            // Tap outside: from the new-food editor go BACK to the list; from the
            // list, close the whole modal. (Replaces the removed header ✕, which
            // doubled as a back button.)
            <div class="modal-background" on:click=move |_| {
                if show_editor.get() { show_editor.set(false); } else { on_close.call(()); }
            }></div>
            <div class="modal-card" style="max-width: 28rem; height: 80vh;">
                <header class="modal-card-head" style="display: block; padding-bottom: 0.5rem;">
                    <div style="display: flex; align-items: center; justify-content: space-between;">
                        <p class="modal-card-title">
                            {move || if show_editor.get() { t("diary_add.new_food") } else { t(title) }}
                        </p>
                    </div>
                </header>
                // `--kb-inset` bottom padding (set in index.html from visualViewport):
                // gives the list exactly the keyboard's height of extra scroll room
                // so its tail (incl. "Новая еда") can be scrolled above the keyboard.
                <section attr:data-ios-scroll="1" class="modal-card-body" style="overflow-y: auto; padding-bottom: var(--kb-inset, 0px);">
                    <FoodPicker
                        foods=foods
                        disabled_ids=disabled_ids
                        goals=goals
                        custom_nutrients=custom_nutrients
                        allow_waste=allow_waste
                        exclude_restaurant=exclude_restaurant
                        on_pick=on_pick
                        on_food_created=on_food_created
                        show_editor=show_editor
                    />
                </section>
                <footer class="modal-card-foot" style="justify-content: flex-end;">
                    <button attr:data-testid="diary-add-btn-done" class="button is-link" on:click=move |_| on_close.call(())>{move || t("diary_add.close")}</button>
                </footer>
            </div>
        </div>
    }
}
