use leptos::*;
use leptos_router::*;

use crate::services::{local, push, story, i18n::{t, weight_unit_signal, WeightUnit}};

const PAGE_BG: &str = "background: var(--bulma-background); min-height: 100vh; padding: 0; margin: -0.75rem;";
const CARD: &str = "background: var(--bulma-scheme-main); border-radius: 12px; overflow: hidden;";

#[component]
pub fn WeightPage() -> impl IntoView {
    let navigate = use_navigate();
    let unit = weight_unit_signal();

    let weight_str = create_rw_signal(String::new());
    let no_water = create_rw_signal(false);
    let no_food = create_rw_signal(false);
    let no_wash = create_rw_signal(false);
    let used_toilet = create_rw_signal(false);
    let morning = create_rw_signal(false);

    // Pre-fill with today's existing measurement so it can be corrected/edited
    // (re-saving upserts today's entry).
    spawn_local(async move {
        let today = chrono::Local::now().format("%Y-%m-%d").to_string();
        if let Some(e) = local::get_weight_for_date(&today).await {
            weight_str.set(format!("{:.1}", unit.get_untracked().from_kg(e.weight_kg)));
            no_water.set(e.no_water);
            no_food.set(e.no_food);
            no_wash.set(e.no_wash);
            used_toilet.set(e.used_toilet);
            morning.set(e.morning);
        }
    });


    let nav_save = navigate.clone();
    let on_save = move |_| {
        let val_str = weight_str.get();
        let val: f64 = match val_str.replace(',', ".").parse() {
            Ok(v) if v > 0.0 => v,
            _ => return,
        };
        let kg = unit.get_untracked().to_kg(val);
        let nav = nav_save.clone();
        leptos::spawn_local(async move {
            let was_first = local::list_weight_entries().await.is_empty();
            local::save_weight(
                kg,
                no_water.get_untracked(),
                no_food.get_untracked(),
                no_wash.get_untracked(),
                used_toilet.get_untracked(),
                morning.get_untracked(),
            ).await;
            // Making a measurement is the milestone event for accounting task 2 —
            // record it in the story DB (idempotent), not derived from entries.
            story::set_flag(story::FIRST_WEIGH, true).await;
            // First-ever measurement: send a congratulatory push (task 2 of the
            // accounting section) confirming it's done.
            if was_first {
                if let Err(e) = push::send_test(t("story.acc.push_first_weigh"), "/story/accounting").await {
                    leptos::logging::error!("first-weigh push: {}", e);
                }
            }
            nav("/", Default::default());
        });
    };

    let can_save = move || {
        weight_str.get().replace(',', ".").parse::<f64>().map(|v| v > 0.0).unwrap_or(false)
    };

    let unit_label = move || match unit.get() {
        WeightUnit::Kg => t("weight.unit_kg"),
        WeightUnit::Lbs => t("weight.unit_lbs"),
    };

    let checkbox = move |signal: RwSignal<bool>, label_key: &'static str| {
        view! {
            <label style="display: flex; align-items: center; padding: 12px 16px; cursor: pointer; gap: 12px;">
                <input type="checkbox"
                    style="width: 20px; height: 20px; accent-color: var(--bulma-link);"
                    prop:checked=move || signal.get()
                    on:change=move |ev| signal.set(event_target_checked(&ev))
                />
                <span class="is-size-6">{move || t(label_key)}</span>
            </label>
            <div style="border-bottom: 0.5px solid var(--bulma-border-weak); margin-left: 48px;"></div>
        }
    };

    view! {
        <div style=PAGE_BG>
            // Nav bar
            <div style="display: flex; align-items: center; padding: 12px 16px;">
                <button
                    style="appearance: none; -webkit-appearance: none; border: none; background: none; cursor: pointer; padding: 4px; font: inherit;"
                    class="is-size-5"
                    on:click={
                        let nav = navigate.clone();
                        move |_| nav("/", Default::default())
                    }
                >
                    <span class="has-text-link">{move || t("common.back")}</span>
                </button>
            </div>

            <h1 class="is-size-1 has-text-weight-bold" style="margin: 0 16px 16px 16px;">{move || t("weight.title")}</h1>

            // Weight input card
            <div style="padding: 0 16px; margin-bottom: 16px;">
                <div style=CARD>
                    <div style="display: flex; align-items: center; padding: 12px 16px;">
                        // type="text" (not "number"): a "number" input wipes its
                        // value to "" the moment a comma is typed, erasing the
                        // user's input. We accept the raw string and normalise the
                        // decimal separator (',' → '.') at parse time.
                        <input type="text"
                            inputmode="decimal"
                            placeholder=move || t("weight.input_placeholder")
                            class="is-size-4 has-text-weight-semibold"
                            style="flex: 1; border: none; background: none; outline: none; color: var(--bulma-text); padding: 4px 0;"
                            prop:value=move || weight_str.get()
                            on:input=move |ev| {
                                weight_str.set(event_target_value(&ev));
                            }
                        />
                        <span class="is-size-5 has-text-grey">{unit_label}</span>
                    </div>
                </div>
            </div>

            // Conditions checklist
            <div style="padding: 0 16px; margin-bottom: 24px;">
                <div style=CARD>
                    {checkbox(no_water, "weight.no_water")}
                    {checkbox(no_food, "weight.no_food")}
                    {checkbox(no_wash, "weight.no_wash")}
                    {checkbox(used_toilet, "weight.used_toilet")}
                    // Last item — no separator
                    <label style="display: flex; align-items: center; padding: 12px 16px; cursor: pointer; gap: 12px;">
                        <input type="checkbox"
                            style="width: 20px; height: 20px; accent-color: var(--bulma-link);"
                            prop:checked=move || morning.get()
                            on:change=move |ev| morning.set(event_target_checked(&ev))
                        />
                        <span class="is-size-6">{move || t("weight.morning")}</span>
                    </label>
                </div>
            </div>

            // Save button
            <div style="padding: 0 16px;">
                <button
                    class="button is-link is-fullwidth is-medium"
                    disabled=move || !can_save()
                    on:click=on_save
                >
                    {move || t("weight.save")}
                </button>
            </div>
        </div>
    }
}

fn event_target_checked(ev: &web_sys::Event) -> bool {
    use wasm_bindgen::JsCast;
    ev.target()
        .and_then(|t| t.dyn_into::<web_sys::HtmlInputElement>().ok())
        .map(|i| i.checked())
        .unwrap_or(false)
}
