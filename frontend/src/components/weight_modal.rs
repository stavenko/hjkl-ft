use leptos::*;
use crate::services::i18n::t;

#[component]
pub fn WeightModal(
    food_name: String,
    current_grams: f64,
    package_weight: Option<f64>,
    on_save: Callback<f64>,
    on_close: Callback<()>,
) -> impl IntoView {
    let grams = create_rw_signal(format!("{}", current_grams));

    let current_val = move || -> f64 { grams.get().parse().unwrap_or(0.0) };

    let adjust = move |delta: f64| {
        let new = (current_val() + delta).max(0.0);
        grams.set(format!("{new}"));
    };

    let set_one_package = move |_| {
        if let Some(pw) = package_weight {
            grams.set(format!("{pw}"));
        }
    };

    let on_submit = move |ev: leptos::ev::SubmitEvent| {
        ev.prevent_default();
        let v = current_val();
        if v > 0.0 {
            on_save.call(v);
        }
    };

    view! {
        <div class="modal is-active">
            <div class="modal-background" on:click=move |_| on_close.call(())></div>
            <div class="modal-card" style="max-width: 20rem;">
                <header class="modal-card-head">
                    <p class="modal-card-title is-size-7">{food_name}</p>
                    <button class="delete" on:click=move |_| on_close.call(())></button>
                </header>
                <section class="modal-card-body">
                    <form on:submit=on_submit>
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
                                <a class="button is-static">{move || t("common.unit.g")}</a>
                            </div>
                        </div>

                        <div class="buttons is-centered mb-3">
                            <button type="button" class="button is-small" on:click=move |_| adjust(-100.0)>"-100"</button>
                            <button type="button" class="button is-small" on:click=move |_| adjust(-10.0)>"-10"</button>
                            <button type="button" class="button is-small" on:click=move |_| adjust(10.0)>"+10"</button>
                            <button type="button" class="button is-small" on:click=move |_| adjust(100.0)>"+100"</button>
                        </div>

                        {package_weight.filter(|pw| *pw > 0.0).map(|pw| {
                            view! {
                                <div class="buttons is-centered mb-3">
                                    <button type="button" class="button is-small" on:click=move |_| adjust(-pw)>
                                        {move || format!("-{:.0}{}", pw, t("common.unit.g"))}
                                    </button>
                                    <button type="button" class="button is-small" on:click=set_one_package>
                                        {move || format!("={:.0}{}", pw, t("common.unit.g"))}
                                    </button>
                                    <button type="button" class="button is-small" on:click=move |_| adjust(pw)>
                                        {move || format!("+{:.0}{}", pw, t("common.unit.g"))}
                                    </button>
                                </div>
                                <p class="is-size-7 has-text-grey has-text-centered">
                                    {move || format!("{}: {:.0}{}", t("weight.package"), pw, t("common.unit.g"))}
                                </p>
                            }
                        })}

                        <div class="field is-grouped is-grouped-right mt-4">
                            <div class="control">
                                <button type="button" class="button is-small"
                                    on:click=move |_| on_close.call(())>{move || t("weight.cancel")}</button>
                            </div>
                            <div class="control">
                                <button type="submit" class="button is-small is-link">{move || t("weight.ok")}</button>
                            </div>
                        </div>
                    </form>
                </section>
            </div>
        </div>
    }
}
