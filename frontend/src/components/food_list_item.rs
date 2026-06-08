use leptos::*;
use api_types::*;

/// Universal food list row.
/// Shows: name | nutrient badges | right-side action slot.
/// `grams`: if Some — scale nutrients by grams/100, else show per 100g.
#[component]
pub fn FoodListItem(
    food: Food,
    goals: Signal<Vec<Goal>>,
    #[prop(optional)]
    grams: Option<f64>,
    /// Content rendered on the right side (action buttons)
    children: Children,
) -> impl IntoView {
    let factor = grams.unwrap_or(100.0) / 100.0;
    let food_c = food.clone();

    view! {
        <div style="display: flex; align-items: center; padding: 0.5rem 0; border-bottom: 1px solid #f0f0f0;">
            <div style="flex: 1; min-width: 0; overflow-wrap: break-word;">
                <span class="is-size-6 has-text-weight-medium">{&food.name}</span>
                <div style="display: flex; flex-wrap: wrap; gap: 0.25rem; margin-top: 0.25rem;">
                    {move || {
                        let gs = goals.get();
                        let f = &food_c;
                        use crate::services::i18n;
                        let badge = |label: &str, val: f64, unit: &str| {
                            view! {
                                <span class="tag is-light is-small">
                                    {format!("{} {:.0}", label, val)}
                                    " "
                                    <span class="has-text-grey-light">{unit.to_string()}</span>
                                </span>
                            }.into_view()
                        };
                        let mut badges: Vec<View> = vec![
                            badge(i18n::nutrient_badge("Calories"), f.kcal * factor, i18n::unit_label("kcal")),
                            badge(i18n::nutrient_badge("Protein"), f.protein * factor, i18n::unit_label("g")),
                            badge(i18n::nutrient_badge("Fat"), f.fat * factor, i18n::unit_label("g")),
                            badge(i18n::nutrient_badge("Carbs"), f.carbs * factor, i18n::unit_label("g")),
                        ];
                        for goal in gs.iter()
                            .filter(|g| g.period == GoalPeriod::Day)
                            .filter(|g| !matches!(g.nutrient.as_str(), "Calories" | "Protein" | "Fat" | "Carbs"))
                        {
                            let val = f.nutrients.get(&goal.nutrient).copied().unwrap_or(0.0) * factor;
                            let label: String = goal.nutrient.chars().take(3).collect();
                            let unit = i18n::unit_label(goal.unit.label());
                            badges.push(badge(&label, val, unit));
                        }
                        badges
                    }}
                </div>
            </div>
            <div style="flex-shrink: 0; margin-left: 1rem; display: flex; align-items: center; gap: 0.75rem;">
                {children()}
            </div>
        </div>
    }
}
