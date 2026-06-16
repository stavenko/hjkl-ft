use leptos::*;
use api_types::*;
use crate::services::i18n::t;
use super::waste_field::WasteField;
use super::restaurant_field::RestaurantField;

#[component]
pub fn FoodWeightModal(
    food: Food,
    goals: Signal<Vec<Goal>>,
    initial_grams: f64,
    #[prop(default = 0.0)]
    initial_waste: f64,
    #[prop(default = false)]
    initial_restaurant: bool,
    submit_label: &'static str,
    on_save: Callback<(f64, f64, bool)>,
    on_close: Callback<()>,
) -> impl IntoView {
    let grams = create_rw_signal(format!("{}", initial_grams));
    let waste = create_rw_signal(if initial_waste > 0.0 { format!("{:.0}", initial_waste) } else { String::new() });
    let restaurant = create_rw_signal(initial_restaurant);
    let show_details = create_rw_signal(false);

    // Normalise ',' → '.' : mobile keyboards emit a comma decimal separator.
    let current_val = move || -> f64 { grams.get().replace(',', ".").parse().unwrap_or(0.0) };
    let waste_val = move || -> f64 { waste.get().replace(',', ".").parse().unwrap_or(0.0) };
    // Restaurant food carries a +20% calorie surcharge (mirrors Food::effective_kcal).
    let kcal_mult = move || -> f64 { if restaurant.get() { 1.2 } else { 1.0 } };

    let adjust = move |delta: f64| {
        let new = (current_val() + delta).max(0.0);
        grams.set(format!("{new}"));
    };

    let pkg = food.package_weight.filter(|w| *w > 0.0);

    let on_submit = move |ev: leptos::ev::SubmitEvent| {
        ev.prevent_default();
        let v = current_val();
        if v > 0.0 {
            on_save.call((v, waste_val(), restaurant.get()));
        }
    };

    let food_for_details = food.clone();
    let food_for_nutrients = food.clone();

    view! {
        <div class="modal is-active">
            <div class="modal-background" on:click=move |_| on_close.call(())></div>
            <div class="modal-card" style="max-width: 24rem;">
                <header class="modal-card-head">
                    <button
                        class="modal-card-title button is-ghost has-text-link is-size-7 px-0"
                        style="text-decoration: none; text-align: left; justify-content: flex-start;"
                        on:click=move |_| show_details.update(|v| *v = !*v)
                    >
                        {&food.name}
                        <span class="is-size-7 has-text-grey ml-1">
                            {move || if show_details.get() { "\u{25b2}" } else { "\u{25bc}" }}
                        </span>
                    </button>
                    <button class="delete" on:click=move |_| on_close.call(())></button>
                </header>
                <section class="modal-card-body">
                    // Food details panel
                    <Show when=move || show_details.get()>
                        <div class="notification mb-3">
                            <p class="has-text-weight-medium is-size-7 mb-1">{move || t("weight.per_100g")}</p>
                            <div class="tags">
                                <span class="tag is-small">{format!("{:.1} kcal", food_for_details.kcal)}</span>
                                <span class="tag is-small">{format!("P {:.1}g", food_for_details.protein)}</span>
                                <span class="tag is-small">{format!("F {:.1}g", food_for_details.fat)}</span>
                                <span class="tag is-small">{format!("C {:.1}g", food_for_details.carbs)}</span>
                            </div>
                            {(!food_for_details.nutrients.is_empty()).then(|| {
                                let items: Vec<String> = food_for_details.nutrients.iter()
                                    .map(|(k, v)| format!("{k}: {v}"))
                                    .collect();
                                view! { <p class="is-size-7">{items.join(", ")}</p> }
                            })}
                            {food_for_details.package_weight.filter(|w| *w > 0.0).map(|w| {
                                view! { <p class="is-size-7">{move || format!("{}: {w:.0}{}", t("weight.package"), t("common.unit.g"))}</p> }
                            })}
                        </div>
                    </Show>

                    // Nutrient chips
                    <div class="tags mb-3">
                        {move || {
                            let gs = goals.get();
                            let factor = (current_val() - waste_val()).max(0.0) / 100.0;
                            let f = &food_for_nutrients;
                            gs.iter()
                                .filter(|g| g.period == GoalPeriod::Day)
                                .map(|goal| {
                                    let val = match goal.nutrient.as_str() {
                                        "Calories" => f.kcal * factor * kcal_mult(),
                                        "Protein" => f.protein * factor,
                                        "Fat" => f.fat * factor,
                                        "Carbs" => f.carbs * factor,
                                        custom => f.nutrients.get(custom).copied().unwrap_or(0.0) * factor,
                                    };
                                    let unit = goal.unit.label();
                                    let label = match goal.nutrient.as_str() {
                                        "Calories" => "C",
                                        "Protein" => "P",
                                        "Fat" => "F",
                                        "Carbs" => "Cb",
                                        _ => &goal.nutrient,
                                    };
                                    view! {
                                        <span class="tag is-small">
                                            {format!("{label} {val:.0}{unit}")}
                                        </span>
                                    }
                                })
                                .collect::<Vec<_>>()
                        }}
                    </div>

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

                        {pkg.map(|pw| {
                            view! {
                                <div class="buttons is-centered mb-3">
                                    <button type="button" class="button is-small" on:click=move |_| adjust(-pw)>
                                        {format!("-{:.0}g", pw)}
                                    </button>
                                    <button type="button" class="button is-small" on:click=move |_| grams.set(format!("{pw}"))>
                                        {format!("={:.0}g", pw)}
                                    </button>
                                    <button type="button" class="button is-small" on:click=move |_| adjust(pw)>
                                        {format!("+{:.0}g", pw)}
                                    </button>
                                </div>
                            }
                        })}

                        <WasteField grams=Signal::derive(current_val) waste=waste />

                        <RestaurantField value=restaurant />

                        <div class="field is-grouped is-grouped-right mt-4">
                            <div class="control">
                                <button type="button" class="button is-small"
                                    on:click=move |_| on_close.call(())>{move || t("weight.cancel")}</button>
                            </div>
                            <div class="control">
                                <button type="submit" class="button is-small is-link">{submit_label}</button>
                            </div>
                        </div>
                    </form>
                </section>
            </div>
        </div>
    }
}
