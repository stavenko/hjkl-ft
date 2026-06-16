use leptos::*;

use crate::services::i18n::t;

/// "Не съел целиком" checkbox + waste-grams input (with 10% / 20% shortcuts).
/// The waste value (in grams, as a string) is owned by the parent.
#[component]
pub fn WasteField(
    /// Current total grams — used by the percentage shortcuts.
    grams: Signal<f64>,
    waste: RwSignal<String>,
) -> impl IntoView {
    let expanded = create_rw_signal(
        waste.get_untracked().replace(',', ".").parse::<f64>().map(|v| v > 0.0).unwrap_or(false),
    );

    let set_pct = move |pct: f64| {
        waste.set(format!("{:.0}", grams.get() * pct));
    };

    view! {
        <label style="display: flex; align-items: center; gap: 8px; cursor: pointer; margin-top: 4px;">
            <input type="checkbox"
                style="width: 18px; height: 18px; accent-color: var(--bulma-link);"
                prop:checked=move || expanded.get()
                on:change=move |ev| {
                    let on = checked(&ev);
                    expanded.set(on);
                    if !on { waste.set(String::new()); }
                }
            />
            <span class="is-size-6">{move || t("waste.not_whole")}</span>
        </label>
        <Show when=move || expanded.get()>
            <div style="display: flex; align-items: center; gap: 6px; margin-top: 8px;">
                <input type="text" inputmode="decimal" class="input is-small" style="width: 6rem;"
                    placeholder=t("waste.placeholder")
                    prop:value=move || waste.get()
                    on:input=move |ev| waste.set(event_target_value(&ev))
                />
                <span class="is-size-7 has-text-grey" style="margin-right: 4px;">{move || t("common.unit.g")}</span>
                <button type="button" class="button is-small" on:click=move |_| set_pct(0.10)>"10%"</button>
                <button type="button" class="button is-small" on:click=move |_| set_pct(0.20)>"20%"</button>
            </div>
        </Show>
    }
}

fn checked(ev: &web_sys::Event) -> bool {
    use wasm_bindgen::JsCast;
    ev.target()
        .and_then(|t| t.dyn_into::<web_sys::HtmlInputElement>().ok())
        .map(|i| i.checked())
        .unwrap_or(false)
}
