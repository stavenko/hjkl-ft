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
                {food_row("help.demo.photo_name", 200.0, 18.0, 13.0, 2.0)}
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

fn para(key: &'static str) -> impl IntoView {
    view! { <p class="is-size-6" style="line-height: 1.5; margin: 0 0 10px;">{move || t(key)}</p> }
}

/// Inert replica of the diary row's «⋮» menu popover with the given items
/// (label key, `danger`). Matches the real popover styling.
fn menu_demo(items: Vec<(&'static str, bool)>) -> impl IntoView {
    view! {
        <div style=format!("{DEMO} justify-content: flex-start; padding: 16px; pointer-events: none;")>
            <div style="background: var(--bulma-scheme-main); border-radius: 6px; \
                    box-shadow: 0 2px 12px rgba(0,0,0,0.15); min-width: 12rem; padding: 0.25rem 0;">
                {items.into_iter().map(|(key, danger)| view! {
                    <button class=if danger { "button is-ghost is-small is-fullwidth has-text-danger" }
                                  else { "button is-ghost is-small is-fullwidth" }
                        style="justify-content: flex-start; text-decoration: none;">
                        {move || t(key)}
                    </button>
                }).collect_view()}
            </div>
        </div>
    }
}

/// Inert example of a diary row where the grams value «150 г» is the tappable
/// control that opens the weight editor.
fn grams_row_demo() -> impl IntoView {
    let goals = Signal::derive(Vec::<Goal>::new);
    view! {
        <div style=format!("{DEMO} align-items: stretch; padding: 12px; pointer-events: none;")>
            <FoodListItem food=demo_food(t("help.demo.food1_name").to_string(), 92.0, 3.4, 0.6, 18.6)
                goals=goals grams=150.0>
                <span class="is-size-6 has-text-link has-text-weight-semibold">
                    {move || format!("150 {}", t("common.unit.g"))}
                </span>
            </FoodListItem>
        </div>
    }
}

fn fab_demo() -> impl IntoView {
    view! {
        <div style=format!("{DEMO} pointer-events: none;")>
            <button class="button is-success is-rounded"
                style="width: 3.5rem; height: 3.5rem; font-size: 1.5rem; box-shadow: 0 4px 12px rgba(0,0,0,0.2); border: none;">
                "+"
            </button>
        </div>
    }
}

/// Sub-topic article (/help/:id).
#[component]
pub fn HelpArticlePage() -> impl IntoView {
    let params = use_params_map();
    let id = move || params.with(|p| p.get("id").cloned().unwrap_or_default());
    let row_menu = || menu_demo(vec![("diary.duplicate", false), ("diary.edit", false), ("diary.delete", true)]);
    view! {
        {back_bar()}
        {move || {
            let id = id();
            let title_key = format!("help.link.{}", id.replace('-', "_"));
            let body = match id.as_str() {
                "copy-day" => view! {
                    {para("help.article.copy_day.p1")}
                    {para("help.article.copy_day.p2")}
                    {menu_demo(vec![("diary.repeat_today", false)])}
                }.into_view(),
                "add-food" => view! {
                    {para("help.article.add_food.p1")}
                    {para("help.article.add_food.p2")}
                    {fab_demo()}
                }.into_view(),
                "recipes" => view! {
                    {para("help.article.recipes.p1")}
                    {para("help.article.recipes.p2")}
                }.into_view(),
                "delete-food" => view! {
                    {para("help.article.delete_food.p1")}
                    {row_menu()}
                }.into_view(),
                "edit-weight" => view! {
                    {para("help.article.edit_weight.p1")}
                    {para("help.article.edit_weight.p2")}
                    {grams_row_demo()}
                }.into_view(),
                "rename-food" => view! {
                    {para("help.article.rename_food.p1")}
                    {para("help.article.rename_food.p2")}
                    {row_menu()}
                }.into_view(),
                _ => view! { <p class="is-size-6 has-text-grey">{move || t("help.article.stub")}</p> }.into_view(),
            };
            view! {
                <h1 class="is-size-4 has-text-weight-bold" style="margin: 0 0 12px;">{t(&title_key).to_string()}</h1>
                {body}
                <div style="height: 24px;"></div>
            }
        }}
    }
}
