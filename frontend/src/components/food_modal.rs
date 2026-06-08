use leptos::*;
use api_types::*;

use crate::services::i18n::t;
use super::food_editor::FoodEditor;

#[component]
pub fn FoodModal(
    custom_nutrients: Signal<Vec<NutrientSpec>>,
    on_created: Callback<Food>,
    on_close: Callback<()>,
) -> impl IntoView {
    view! {
        <div class="modal is-active">
            <div class="modal-background" on:click=move |_| on_close.call(())></div>
            <div class="modal-card" style="max-width: 28rem;">
                <header class="modal-card-head">
                    <p class="modal-card-title">{t("food_modal.title")}</p>
                    <button class="delete" on:click=move |_| on_close.call(())></button>
                </header>
                <section class="modal-card-body">
                    <FoodEditor
                        custom_nutrients=custom_nutrients
                        on_draft=on_created
                    />
                </section>
            </div>
        </div>
    }
}
