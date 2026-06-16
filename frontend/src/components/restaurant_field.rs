use leptos::*;

use crate::services::i18n::t;

/// "Ресторанная еда" checkbox. The boolean is owned by the parent and, on save,
/// becomes the food's `is_restaurant` flag (Copy-on-Write in `local`).
#[component]
pub fn RestaurantField(value: RwSignal<bool>) -> impl IntoView {
    view! {
        <label style="display: flex; align-items: center; gap: 8px; cursor: pointer; margin-top: 4px;">
            <input type="checkbox"
                style="width: 18px; height: 18px; accent-color: var(--bulma-link);"
                prop:checked=move || value.get()
                on:change=move |ev| value.set(checked(&ev))
            />
            <span class="is-size-6">{move || t("restaurant.eaten_out")}</span>
        </label>
    }
}

fn checked(ev: &web_sys::Event) -> bool {
    use wasm_bindgen::JsCast;
    ev.target()
        .and_then(|t| t.dyn_into::<web_sys::HtmlInputElement>().ok())
        .map(|i| i.checked())
        .unwrap_or(false)
}
