//! Help pages. `HelpFoodPage` (/help/food) explains how to add food — opened from
//! the dashboard progress widget's «?» before the first entry. The screenshot slots
//! are placeholders for now (real renders of our controls come next). `HelpArticlePage`
//! (/help/:id) serves the linked sub-topics as stubs until their content is written.

use leptos::*;
use leptos_router::{use_navigate, use_params_map};
use api_types::{Food, Goal};

use crate::components::food_list_item::FoodListItem;
use crate::services::i18n::t;

/// A throwaway `Food` used only to render an example row in the help demos.
fn demo_food(name: String, kcal: f64, protein: f64, fat: f64, carbs: f64) -> Food {
    Food {
        id: String::new(),
        name,
        kcal,
        protein,
        fat,
        carbs,
        nutrients: Default::default(),
        package_weight: None,
        is_recipe: false,
        recipe_id: None,
        archived: false,
        is_restaurant: false,
        is_snack: None,
        created_at: String::new(),
        updated_at: String::new(),
    }
}

/// One example food row (real `FoodListItem`) with an inert green «+» action.
fn food_row(name_key: &'static str, kcal: f64, p: f64, f: f64, c: f64) -> impl IntoView {
    let goals = Signal::derive(Vec::<Goal>::new);
    view! {
        <FoodListItem food=demo_food(t(name_key).to_string(), kcal, p, f, c) goals=goals>
            <span style="width: 28px; height: 28px; border-radius: 50%; background: var(--bulma-success); \
                    color: #fff; display: inline-flex; align-items: center; justify-content: center; font-size: 1.1rem;">
                "+"
            </span>
        </FoodListItem>
    }
}

const CARD: &str = "background: var(--bulma-scheme-main); border-radius: 12px; padding: 14px 16px; \
    display: flex; flex-direction: column; gap: 8px;";
// A subtle frame that holds a LIVE control so it doesn't stand out against the text.
const DEMO: &str = "background: var(--bulma-background); border-radius: 10px; padding: 20px; \
    display: flex; align-items: center; justify-content: center;";
const H2: &str = "font-weight: 700; margin: 18px 0 4px;";
const ROW: &str = "display: flex; align-items: center; justify-content: space-between; \
    padding: 12px 4px; color: inherit; text-decoration: none;";

fn back_bar() -> impl IntoView {
    view! {
        <button class="button is-light is-small" style="margin-bottom: 12px;"
            on:click=move |_| use_navigate()("/", Default::default())>
            "‹ " {move || t("help.back")}
        </button>
    }
}

#[component]
pub fn HelpFoodPage() -> impl IntoView {
    let link = |route: &'static str, key: &'static str| {
        view! {
            <a href=route style=ROW>
                <span class="is-size-6">{move || t(key)}</span>
                <span style="color: var(--bulma-text-weak); font-size: 18px;">"›"</span>
            </a>
        }
    };

    view! {
        {back_bar()}
        <h1 class="is-size-4 has-text-weight-bold" style="margin: 0 0 10px;">{move || t("help.food.title")}</h1>
        <p class="is-size-6" style="line-height: 1.5;">{move || t("help.food.intro")}</p>

        <div style=H2>{move || t("help.food.where_title")}</div>
        <p class="is-size-6" style="line-height: 1.5; margin-bottom: 8px;">{move || t("help.food.where_text")}</p>
        // Live «+» FAB — the exact same button the diary shows (not a screenshot).
        <div style=DEMO>
            <button class="button is-success is-rounded" attr:aria-label="+"
                style="width: 3.5rem; height: 3.5rem; font-size: 1.5rem; box-shadow: 0 4px 12px rgba(0,0,0,0.2); border: none; cursor: default;">
                "+"
            </button>
        </div>

        <div style=H2>{move || t("help.food.methods_title")}</div>

        <div style=CARD>
            <span class="is-size-6 has-text-weight-bold">{move || t("help.food.search_title")}</span>
            <p class="is-size-6" style="line-height: 1.5;">{move || t("help.food.search_text")}</p>
            // Inert example: search field + a couple of result rows.
            <div style=format!("{DEMO} flex-direction: column; align-items: stretch; gap: 4px; padding: 12px; pointer-events: none;")>
                <input class="input" readonly=true prop:value=move || t("help.demo.search_query") style="margin-bottom: 4px;"/>
                {food_row("help.demo.food1_name", 92.0, 3.4, 0.6, 18.6)}
                {food_row("help.demo.food2_name", 343.0, 12.6, 3.3, 62.0)}
            </div>
        </div>
        <div style=format!("{CARD} margin-top: 10px;")>
            <span class="is-size-6 has-text-weight-bold">{move || t("help.food.ai_title")}</span>
            <p class="is-size-6" style="line-height: 1.5;">{move || t("help.food.ai_text")}</p>
            // Inert example: the request field, the parse button, and the parsed rows.
            <div style=format!("{DEMO} flex-direction: column; align-items: stretch; gap: 4px; padding: 12px; pointer-events: none;")>
                <textarea class="textarea" readonly=true rows="2" prop:value=move || t("help.demo.ai_query")></textarea>
                <button class="button is-link is-small is-fullwidth" style="margin: 6px 0;">{move || t("help.demo.ai_button")}</button>
                {food_row("help.demo.ai1_name", 190.0, 13.0, 15.0, 1.0)}
                {food_row("help.demo.ai2_name", 90.0, 3.0, 1.0, 17.0)}
            </div>
        </div>
        <div style=format!("{CARD} margin-top: 10px;")>
            <span class="is-size-6 has-text-weight-bold">{move || t("help.food.photo_title")}</span>
            <p class="is-size-6" style="line-height: 1.5;">{move || t("help.food.photo_text")}</p>
            // Inert example: the capture button, a photo placeholder, the recognised row.
            <div style=format!("{DEMO} flex-direction: column; align-items: stretch; gap: 4px; padding: 12px; pointer-events: none;")>
                <button class="button is-small is-fullwidth">{move || t("help.demo.photo_button")}</button>
                <div style="height: 88px; background: var(--bulma-border-weak); border-radius: 8px; margin: 6px 0; \
                        display: flex; align-items: center; justify-content: center; font-size: 1.6rem;">
                    "🍽"
                </div>
                {food_row("help.demo.photo_name", 250.0, 12.0, 14.0, 20.0)}
            </div>
        </div>

        <div style=H2>{move || t("help.food.more_title")}</div>
        <div style=format!("{CARD} gap: 0;")>
            {link("/help/copy-day", "help.link.copy_day")}
            <div style="border-bottom: 0.5px solid var(--bulma-border-weak);"></div>
            {link("/help/add-food", "help.link.add_food")}
            <div style="border-bottom: 0.5px solid var(--bulma-border-weak);"></div>
            {link("/help/recipes", "help.link.recipes")}
            <div style="border-bottom: 0.5px solid var(--bulma-border-weak);"></div>
            {link("/help/delete-food", "help.link.delete_food")}
            <div style="border-bottom: 0.5px solid var(--bulma-border-weak);"></div>
            {link("/help/edit-weight", "help.link.edit_weight")}
            <div style="border-bottom: 0.5px solid var(--bulma-border-weak);"></div>
            {link("/help/rename-food", "help.link.rename_food")}
        </div>
        <div style="height: 24px;"></div>
    }
}

/// Sub-topic article (/help/:id). Content is a stub until it's written.
#[component]
pub fn HelpArticlePage() -> impl IntoView {
    let params = use_params_map();
    let title_key = move || {
        let id = params.with(|p| p.get("id").cloned().unwrap_or_default());
        format!("help.link.{}", id.replace('-', "_"))
    };
    view! {
        {back_bar()}
        <h1 class="is-size-4 has-text-weight-bold" style="margin: 0 0 12px;">
            {move || t(&title_key()).to_string()}
        </h1>
        <p class="is-size-6 has-text-grey" style="line-height: 1.5;">{move || t("help.article.stub")}</p>
    }
}
