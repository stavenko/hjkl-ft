//! Help pages. `HelpFoodPage` (/help/food) explains how to add food — opened from
//! the dashboard progress widget's «?» before the first entry. The screenshot slots
//! are placeholders for now (real renders of our controls come next). `HelpArticlePage`
//! (/help/:id) serves the linked sub-topics as stubs until their content is written.

use leptos::*;
use leptos_router::{use_navigate, use_params_map};
use api_types::{Food, Goal, NutrientSpec, StepEntry, WeightEntry};

use crate::components::food_list_item::FoodListItem;
use crate::components::food_picker::FoodPicker;
use crate::components::food_editor::FoodEditor;
use crate::components::weight_widget::{EmptyPrompt, WeightWidget};
use crate::components::steps_widget::StepsWidget;
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
        is_liquid_cal: None,
        is_veg_fruit: None,
        created_at: String::new(),
        updated_at: String::new(),
    }
}

const CARD: &str ="background: var(--bulma-scheme-main); border-radius: 12px; padding: 14px 16px; \
    display: flex; flex-direction: column; gap: 8px;";
// A subtle frame that holds a LIVE control so it doesn't stand out against the text.
const DEMO: &str = "background: var(--bulma-background); border-radius: 10px; padding: 20px; \
    display: flex; align-items: center; justify-content: center;";
const H2: &str = "font-weight: 700; margin: 18px 0 4px;";
const ROW: &str = "display: flex; align-items: center; justify-content: space-between; \
    padding: 12px 4px; color: inherit; text-decoration: none;";

fn back_bar() -> impl IntoView {
    // Back = previous page in history (consistent across all help pages); Close =
    // leave help, back to the main screen. Shared by every help page.
    let go_back = move |_| {
        if let Some(win) = web_sys::window() {
            if let Ok(history) = win.history() {
                let _ = history.back();
            }
        }
    };
    view! {
        <div style="display: flex; align-items: center; justify-content: space-between; margin-bottom: 12px;">
            <button class="button is-light is-small" on:click=go_back>
                "‹ " {move || t("help.back")}
            </button>
            <button class="button is-light is-small" attr:aria-label="close"
                on:click=move |_| use_navigate()("/", Default::default())>
                "\u{2715}"
            </button>
        </div>
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

        // Show that you need the Diary tab and the «+» button.
        <p class="is-size-6" style="line-height: 1.5; margin-bottom: 10px;">{move || t("help.food.where_text")}</p>
        {fab_demo()}

        // There is no global product base — you build your own.
        <p class="is-size-6" style="line-height: 1.5; margin: 14px 0 4px;">{move || t("help.food.no_base")}</p>

        <div style=H2>{move || t("help.food.methods_title")}</div>
        <div style=format!("{CARD} gap: 0;")>
            {link("/help/food-search", "help.link.food_search")}
            <div style="border-bottom: 0.5px solid var(--bulma-border-weak);"></div>
            {link("/help/food-ai", "help.link.food_ai")}
            <div style="border-bottom: 0.5px solid var(--bulma-border-weak);"></div>
            {link("/help/food-photo", "help.link.food_photo")}
        </div>

        <div style=H2>{move || t("help.food.more_title")}</div>
        <div style=format!("{CARD} gap: 0;")>
            {link("/help/copy-day", "help.link.copy_day")}
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

/// A tappable «label ›» row linking to another help route.
fn link_row(route: &'static str, key: &'static str) -> impl IntoView {
    view! {
        <a href=route style=ROW>
            <span class="is-size-6">{move || t(key)}</span>
            <span style="color: var(--bulma-text-weak); font-size: 18px;">"›"</span>
        </a>
    }
}

/// A bulleted list rendered from a slice of i18n keys.
fn points(keys: &'static [&'static str]) -> impl IntoView {
    view! {
        <ul style="margin: 0 0 10px; padding-left: 1.2rem; list-style: disc;">
            {keys.iter().map(|&k| view! {
                <li class="is-size-6" style="line-height: 1.5; margin-bottom: 6px;">{move || t(k)}</li>
            }).collect_view()}
        </ul>
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

/// A subtle frame wrapping a LIVE-but-inert real component so it reads as a demo.
const FRAME: &str = "background: var(--bulma-background); border-radius: 12px; \
    padding: 14px; pointer-events: none;";

/// Inert copy of the real diary «+» FAB.
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

/// Inert copy of the REAL food picker (search over your personal base + list with
/// «+» + «add your product»). Seeded with a couple of example foods so the list
/// isn't empty — it renders the exact production component, not a mock-up.
fn picker_demo() -> impl IntoView {
    let foods = Signal::derive(|| {
        let mk = |id: &str, name: &str, k: f64, p: f64, f: f64, c: f64| {
            let mut fd = demo_food(name.to_string(), k, p, f, c);
            fd.id = id.to_string();
            fd
        };
        vec![
            mk("demo-buckwheat", "Гречка варёная", 92.0, 3.4, 0.6, 18.6),
            mk("demo-omelette", "Омлет из 2 яиц", 190.0, 13.0, 15.0, 1.0),
        ]
    });
    view! {
        <div style=FRAME>
            <FoodPicker
                foods=foods
                disabled_ids=Signal::derive(Vec::<String>::new)
                goals=Signal::derive(Vec::<Goal>::new)
                custom_nutrients=Signal::derive(Vec::<NutrientSpec>::new)
                on_pick=Callback::new(|_| {})
                on_food_created=Callback::new(|_| {})
                show_editor=create_rw_signal(false)
            />
        </div>
    }
}

/// Inert copy of the REAL new-product editor, opened on the given tab
/// (0 = by name / AI, 1 = by photo).
fn editor_demo(initial_name: &'static str, tab: u8) -> impl IntoView {
    view! {
        <div style=FRAME>
            <FoodEditor
                custom_nutrients=Signal::derive(Vec::<NutrientSpec>::new)
                on_draft=Callback::new(|_| {})
                initial_name=initial_name
                initial_tab=tab
            />
        </div>
    }
}

/// Inert example: the «+ New» button that starts a recipe on the Recipes tab.
fn recipe_new_demo() -> impl IntoView {
    view! {
        <div style=format!("{DEMO} pointer-events: none;")>
            <button class="button is-link">{move || t("recipes.new")}</button>
        </div>
    }
}

/// Inert example of the recipe builder: two ingredient rows (name + weight) plus
/// the «+ Add ingredient» and «Finalize» buttons — showing what to tap.
fn recipe_demo() -> impl IntoView {
    let goals = Signal::derive(Vec::<Goal>::new);
    let ingredient = |name_key: &'static str, kcal: f64, p: f64, f: f64, c: f64, grams: f64| {
        view! {
            <FoodListItem food=demo_food(t(name_key).to_string(), kcal, p, f, c) goals=goals grams=grams>
                <span class="is-size-7 has-text-grey">{format!("{:.0} {}", grams, t("common.unit.g"))}</span>
            </FoodListItem>
        }
    };
    view! {
        <div style=format!("{DEMO} flex-direction: column; align-items: stretch; gap: 6px; padding: 12px; pointer-events: none;")>
            {ingredient("help.demo.recipe1_name", 389.0, 12.6, 6.9, 66.3, 80.0)}
            {ingredient("help.demo.recipe2_name", 121.0, 17.2, 5.0, 1.8, 200.0)}
            <div style="display: flex; gap: 8px; margin-top: 6px;">
                <button class="button is-small">{move || t("recipe.add_ingredient")}</button>
                <button class="button is-link is-small">{move || t("recipe.finalize")}</button>
            </div>
        </div>
    }
}

/// Inert replica of the past-day row's «repeat» control — the two circular-arrows
/// icon button with its popover open on «Repeat today». Logged past-day entries use
/// THIS button, not the «⋮» menu.
fn repeat_demo() -> impl IntoView {
    view! {
        <div style=format!("{DEMO} justify-content: flex-end; padding: 16px 24px; pointer-events: none;")>
            <div style="position: relative; display: flex; flex-direction: column; align-items: flex-end; gap: 6px;">
                <button class="button is-ghost" style="height: 2.5rem; width: 2.5rem; padding: 0;">
                    <svg xmlns="http://www.w3.org/2000/svg" width="20" height="20" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">
                        <polyline points="17 1 21 5 17 9"/>
                        <path d="M3 11V9a4 4 0 0 1 4-4h14"/>
                        <polyline points="7 23 3 19 7 15"/>
                        <path d="M21 13v2a4 4 0 0 1-4 4H3"/>
                    </svg>
                </button>
                <div style="background: var(--bulma-scheme-main); border-radius: 6px; box-shadow: 0 2px 12px rgba(0,0,0,0.15); min-width: 10rem; padding: 0.25rem 0;">
                    <button class="button is-ghost is-small is-fullwidth" style="justify-content: flex-start; text-decoration: none;">
                        {move || t("diary.repeat_today")}
                    </button>
                </div>
            </div>
        </div>
    }
}

/// The rounded card used on the weight/steps entry pages.
const ENTRY_CARD: &str = "background: var(--bulma-scheme-main); border-radius: 12px; overflow: hidden;";

/// Inert copy of the dashboard's empty weight/steps TILE (prompt + round «+») —
/// what the user taps on the main screen to open the chart window. Uses the real
/// `EmptyPrompt` so it matches the widget exactly.
fn widget_tile_demo(text_key: &'static str) -> impl IntoView {
    view! {
        <div style=FRAME>
            <div style="background: var(--bulma-scheme-main); border-radius: 12px; padding: 10px 12px; \
                    height: 130px; box-sizing: border-box; max-width: 220px; margin: 0 auto;">
                <EmptyPrompt text_key=text_key/>
            </div>
        </div>
    }
}

/// Example weight series (a gentle downtrend) so the widget renders a real chart.
fn sample_weights() -> Vec<WeightEntry> {
    let mk = |d: &str, kg: f64| WeightEntry {
        id: d.to_string(), date: d.to_string(), weight_kg: kg,
        no_water: true, no_food: true, no_wash: true, used_toilet: true, morning: true,
        created_at: String::new(), updated_at: String::new(),
    };
    vec![
        mk("2026-06-01", 78.2), mk("2026-06-02", 78.0), mk("2026-06-03", 78.3),
        mk("2026-06-04", 77.8), mk("2026-06-05", 77.6), mk("2026-06-06", 77.7),
        mk("2026-06-07", 77.3),
    ]
}

/// Example step series so the steps widget renders a real chart.
fn sample_steps() -> Vec<StepEntry> {
    let mk = |d: &str, s: u32| StepEntry {
        id: d.to_string(), date: d.to_string(), steps: s,
        created_at: String::new(), updated_at: String::new(),
    };
    vec![
        mk("2026-06-01", 7200), mk("2026-06-02", 8100), mk("2026-06-03", 6800),
        mk("2026-06-04", 9000), mk("2026-06-05", 7600), mk("2026-06-06", 8400),
        mk("2026-06-07", 7900),
    ]
}

/// Inert copy of the weight tile AFTER some data — the chart look, so a returning
/// user recognises the tile to tap even without the «tap here» prompt.
fn weight_chart_demo() -> impl IntoView {
    view! {
        <div style=FRAME>
            <div style="max-width: 220px; margin: 0 auto; height: 120px;">
                <WeightWidget entries=Signal::derive(sample_weights)/>
            </div>
        </div>
    }
}

/// Inert copy of the steps tile with data (the chart look).
fn steps_chart_demo() -> impl IntoView {
    view! {
        <div style=FRAME>
            <div style="max-width: 220px; margin: 0 auto; height: 120px;">
                <StepsWidget entries=Signal::derive(sample_steps)/>
            </div>
        </div>
    }
}

/// Inert copy of the chart window's «add» button (the one that opens the entry form).
fn open_button_demo(label_key: &'static str) -> impl IntoView {
    view! {
        <div style=FRAME>
            <button class="button is-link is-fullwidth">{move || t(label_key)}</button>
        </div>
    }
}

/// Inert copy of the weight-entry screen: value field + the 5 condition checkboxes
/// (shown ticked) + «Save» — the same controls the real page renders.
fn weigh_demo() -> impl IntoView {
    let row = |label: &'static str, last: bool| view! {
        <div style="display: flex; align-items: center; padding: 12px 16px; gap: 12px;">
            <input type="checkbox" prop:checked=true
                style="width: 20px; height: 20px; accent-color: var(--bulma-link);"/>
            <span class="is-size-6">{move || t(label)}</span>
        </div>
        {(!last).then(|| view! {
            <div style="border-bottom: 0.5px solid var(--bulma-border-weak); margin-left: 48px;"></div>
        })}
    };
    view! {
        <div style=format!("{FRAME} display: flex; flex-direction: column; gap: 12px;")>
            <div style=ENTRY_CARD>
                <div style="display: flex; align-items: center; padding: 12px 16px;">
                    <span class="is-size-4 has-text-weight-semibold" style="flex: 1;">"72,5"</span>
                    <span class="is-size-5 has-text-grey">{move || t("weight.unit_kg")}</span>
                </div>
            </div>
            <div style=ENTRY_CARD>
                {row("weight.morning", false)}
                {row("weight.no_food", false)}
                {row("weight.no_water", false)}
                {row("weight.used_toilet", false)}
                {row("weight.no_wash", true)}
            </div>
            <button class="button is-link is-fullwidth is-medium">{move || t("weight.save")}</button>
        </div>
    }
}

/// Inert copy of the steps-entry screen: today/yesterday choice + the count field +
/// «Save».
fn steps_demo() -> impl IntoView {
    let radio = |label: &'static str, checked: bool, last: bool| view! {
        <div style="display: flex; align-items: center; padding: 12px 16px; gap: 12px;">
            <input type="radio" prop:checked=checked
                style="width: 20px; height: 20px; accent-color: var(--bulma-link);"/>
            <span class="is-size-6">{move || t(label)}</span>
        </div>
        {(!last).then(|| view! {
            <div style="border-bottom: 0.5px solid var(--bulma-border-weak); margin-left: 48px;"></div>
        })}
    };
    view! {
        <div style=format!("{FRAME} display: flex; flex-direction: column; gap: 12px;")>
            <div style=ENTRY_CARD>
                {radio("steps.for_today", true, false)}
                {radio("steps.for_yesterday", false, true)}
            </div>
            <div style=ENTRY_CARD>
                <div style="display: flex; align-items: center; padding: 12px 16px;">
                    <span class="is-size-4 has-text-weight-semibold" style="flex: 1; text-align: right;">"8000"</span>
                    <span class="is-size-5 has-text-grey" style="margin-left: 8px;">{move || t("steps.unit")}</span>
                </div>
            </div>
            <button class="button is-link is-fullwidth is-medium">{move || t("steps.save")}</button>
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
                // Hub opened from the dashboard progress widget — the three daily params.
                "diary" => view! {
                    {para("help.article.diary.intro")}
                    <div style=format!("{CARD} gap: 0;")>
                        {link_row("/help/food", "help.link.food_diary")}
                        <div style="border-bottom: 0.5px solid var(--bulma-border-weak);"></div>
                        {link_row("/help/weigh", "help.link.weigh")}
                        <div style="border-bottom: 0.5px solid var(--bulma-border-weak);"></div>
                        {link_row("/help/steps", "help.link.steps")}
                    </div>
                }.into_view(),
                "weigh" => view! {
                    {para("help.article.weigh.intro")}
                    {points(&[
                        "help.article.weigh.p1", "help.article.weigh.p2", "help.article.weigh.p3",
                        "help.article.weigh.p4", "help.article.weigh.p5",
                    ])}
                    <div style=H2>{move || t("help.article.weigh.how_title")}</div>
                    {para("help.article.weigh.open1")}
                    {widget_tile_demo("weight.empty_prompt")}
                    {para("help.article.weigh.open1b")}
                    {weight_chart_demo()}
                    {para("help.article.weigh.open2")}
                    {open_button_demo("weight.add")}
                    {para("help.article.weigh.open3")}
                    {weigh_demo()}
                    {para("help.article.weigh.fluct")}
                }.into_view(),
                "steps" => view! {
                    {para("help.article.steps.intro")}
                    {points(&[
                        "help.article.steps.p1", "help.article.steps.p2",
                        "help.article.steps.p3", "help.article.steps.p4",
                    ])}
                    <div style=H2>{move || t("help.article.steps.how_title")}</div>
                    {para("help.article.steps.open1")}
                    {widget_tile_demo("steps.empty_prompt")}
                    {para("help.article.steps.open1b")}
                    {steps_chart_demo()}
                    {para("help.article.steps.open2")}
                    {open_button_demo("steps.add")}
                    {para("help.article.steps.open3")}
                    {steps_demo()}
                }.into_view(),
                "copy-day" => view! {
                    {para("help.article.copy_day.p1")}
                    {para("help.article.copy_day.p2")}
                    {repeat_demo()}
                }.into_view(),
                "food-search" => view! {
                    {para("help.food.search_text")}
                    {picker_demo()}
                }.into_view(),
                "food-ai" => view! {
                    <div style=H2>{move || t("help.food.new_how_title")}</div>
                    {para("help.food.new_how1")}
                    {picker_demo()}
                    {para("help.food.new_how2")}
                    {para("help.food.ai_text")}
                    {editor_demo("Омлет из двух яиц", 0)}
                }.into_view(),
                "food-photo" => view! {
                    <div style=H2>{move || t("help.food.new_how_title")}</div>
                    {para("help.food.new_how1")}
                    {picker_demo()}
                    {para("help.food.new_how2")}
                    {para("help.food.photo_text")}
                    {editor_demo("", 1)}
                }.into_view(),
                "recipes" => view! {
                    {para("help.article.recipes.p1")}
                    {recipe_new_demo()}
                    {para("help.article.recipes.p2")}
                    {recipe_demo()}
                    {para("help.article.recipes.p3")}
                    <div style=format!("{CARD} gap: 0;")>
                        <a href="/help/food-search" style=ROW>
                            <span class="is-size-6">{move || t("help.link.food_search")}</span>
                            <span style="color: var(--bulma-text-weak); font-size: 18px;">"›"</span>
                        </a>
                    </div>
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
