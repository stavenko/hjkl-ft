use leptos::*;
use leptos_router::*;

use crate::services::{local, i18n::t};

const PAGE_BG: &str = "background: var(--bulma-background); min-height: 100vh; padding: 0; margin: -0.75rem;";
const CARD: &str = "background: var(--bulma-scheme-main); border-radius: 12px; overflow: hidden;";

#[component]
pub fn StepsPage() -> impl IntoView {
    let navigate = use_navigate();

    let steps_str = create_rw_signal(String::new());
    let for_yesterday = create_rw_signal(false);

    // Pre-fill with today's existing entry so it can be corrected/edited.
    spawn_local(async move {
        let today = chrono::Local::now().format("%Y-%m-%d").to_string();
        if let Some(e) = local::get_steps_for_date(&today).await {
            steps_str.set(e.steps.to_string());
        }
    });

    let nav_save = navigate.clone();
    let on_save = move |_| {
        let val: u32 = match steps_str.get().replace(' ', "").parse() {
            Ok(v) if v > 0 => v,
            _ => return,
        };
        let date = if for_yesterday.get() {
            (chrono::Local::now() - chrono::Duration::days(1))
                .format("%Y-%m-%d")
                .to_string()
        } else {
            chrono::Local::now().format("%Y-%m-%d").to_string()
        };
        let nav = nav_save.clone();
        leptos::spawn_local(async move {
            local::save_steps(&date, val).await;
            nav("/diary", Default::default());
        });
    };

    let can_save = move || {
        steps_str.get().replace(' ', "").parse::<u32>().map(|v| v > 0).unwrap_or(false)
    };

    let radio_style = "display: flex; align-items: center; padding: 12px 16px; cursor: pointer; gap: 12px;";

    view! {
        <div style=PAGE_BG>
            <div style="display: flex; align-items: center; padding: 12px 16px;">
                <button
                    style="appearance: none; -webkit-appearance: none; border: none; background: none; cursor: pointer; padding: 4px; font: inherit;"
                    class="is-size-5"
                    on:click={
                        let nav = navigate.clone();
                        move |_| nav("/diary", Default::default())
                    }
                >
                    <span class="has-text-link">{move || t("common.back")}</span>
                </button>
            </div>

            <h1 class="is-size-1 has-text-weight-bold" style="margin: 0 16px 16px 16px;">{move || t("steps.title")}</h1>

            <div style="padding: 0 16px; margin-bottom: 16px;">
                <div style=CARD>
                    <label style=radio_style>
                        <input type="radio" name="steps_day"
                            style="width: 20px; height: 20px; accent-color: var(--bulma-link);"
                            prop:checked=move || !for_yesterday.get()
                            on:change=move |_| for_yesterday.set(false)
                        />
                        <span class="is-size-6">{move || t("steps.for_today")}</span>
                    </label>
                    <div style="border-bottom: 0.5px solid var(--bulma-border-weak); margin-left: 48px;"></div>
                    <label style=radio_style>
                        <input type="radio" name="steps_day"
                            style="width: 20px; height: 20px; accent-color: var(--bulma-link);"
                            prop:checked=move || for_yesterday.get()
                            on:change=move |_| for_yesterday.set(true)
                        />
                        <span class="is-size-6">{move || t("steps.for_yesterday")}</span>
                    </label>
                </div>
            </div>

            <div style="padding: 0 16px; margin-bottom: 24px;">
                <div style=CARD>
                    <div style="display: flex; align-items: center; padding: 12px 16px;">
                        <input type="number"
                            inputmode="numeric"
                            step="100"
                            placeholder="0000"
                            class="is-size-4 has-text-weight-semibold"
                            style="width: 0; flex: 1; min-width: 0; border: none; background: none; outline: none; color: var(--bulma-text); padding: 4px 0; text-align: right;"
                            prop:value=move || steps_str.get()
                            on:input=move |ev| {
                                steps_str.set(event_target_value(&ev));
                            }
                        />
                        <span class="is-size-5 has-text-grey" style="flex-shrink: 0; margin-left: 8px;">{move || t("steps.unit")}</span>
                    </div>
                </div>
            </div>

            <div style="padding: 0 16px;">
                <button
                    class="button is-link is-fullwidth is-medium"
                    disabled=move || !can_save()
                    on:click=on_save
                >
                    {move || t("steps.save")}
                </button>
            </div>
        </div>
    }
}
