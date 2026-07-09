//! Menstrual-cycle widget (linear v1). On the dashboard it's a single full-width
//! line — «Цикл: N День» — that opens a full-page panel showing the current cycle
//! day, the phase (description + how it should affect weight and training), and a
//! button to set the first day of the cycle via a small date dialog.
//!
//! The cycle is modelled simply as a fixed 28-day loop anchored on the stored
//! first day (`profile::cycle_start`).

use leptos::*;

use crate::services::i18n::{relative_date, t};
use crate::services::profile;

const CYCLE_LEN: i64 = 28;

#[derive(Clone, Copy)]
enum Phase {
    Menstrual,
    Follicular,
    Ovulation,
    Luteal,
}

impl Phase {
    fn key(self) -> &'static str {
        match self {
            Phase::Menstrual => "menstrual",
            Phase::Follicular => "follicular",
            Phase::Ovulation => "ovulation",
            Phase::Luteal => "luteal",
        }
    }
}

/// Current cycle day (1-based) for a "YYYY-MM-DD" first day, looping every 28 days.
fn cycle_day(start: &str) -> Option<i64> {
    let s = chrono::NaiveDate::parse_from_str(start, "%Y-%m-%d").ok()?;
    let today = chrono::Local::now().date_naive();
    let diff = (today - s).num_days();
    if diff < 0 {
        return Some(1);
    }
    Some(diff % CYCLE_LEN + 1)
}

fn phase_of(day: i64) -> Phase {
    match day {
        1..=5 => Phase::Menstrual,
        6..=13 => Phase::Follicular,
        14..=16 => Phase::Ovulation,
        _ => Phase::Luteal,
    }
}

fn today_str() -> String {
    chrono::Local::now().format("%Y-%m-%d").to_string()
}

/// Collapsed one-liner for the dashboard grid: «Цикл: N День» (or «—» if unset).
#[component]
pub fn CycleLine() -> impl IntoView {
    let label = profile::get_cycle_start()
        .and_then(|s| cycle_day(&s))
        .map(|d| format!("{d}"))
        .unwrap_or_else(|| t("cycle.not_set").to_string());
    view! {
        <div style="height: 100%; display: flex; align-items: center; gap: 8px; \
                    padding: 0 14px; background: var(--bulma-scheme-main); border-radius: 16px; \
                    box-shadow: 0 2px 10px rgba(0,0,0,0.06);">
            <span class="is-size-6 has-text-weight-semibold">{move || t("cycle.title")}":"</span>
            <span class="is-size-6 has-text-weight-bold" style="color: var(--bulma-link);">{label}</span>
            <span class="is-size-6 has-text-grey">{move || t("cycle.day_label")}</span>
        </div>
    }
}

/// Full-page panel (rendered inside the dashboard overlay): current day, phase,
/// weight + training guidance, and the "set first day" control + dialog.
#[component]
pub fn CyclePanel() -> impl IntoView {
    let bump = create_rw_signal(0u32);
    let start = move || {
        bump.get();
        profile::get_cycle_start()
    };
    let dialog_open = create_rw_signal(false);
    // Draft date for the picker (defaults to the current start, or today).
    let draft = create_rw_signal(String::new());

    let open_dialog = move |_| {
        draft.set(profile::get_cycle_start().unwrap_or_else(today_str));
        dialog_open.set(true);
    };
    let save = move |_| {
        let d = draft.get();
        if !d.is_empty() {
            profile::set_cycle_start(&d);
            bump.update(|v| *v += 1);
        }
        dialog_open.set(false);
    };

    view! {
        <div style="display: flex; flex-direction: column; gap: 16px;">
            {move || match start() {
                Some(s) => {
                    let day = cycle_day(&s).unwrap_or(1);
                    let ph = phase_of(day).key();
                    let name = format!("cycle.phase.{ph}.name");
                    let desc = format!("cycle.phase.{ph}.desc");
                    let weight = format!("cycle.phase.{ph}.weight");
                    let training = format!("cycle.phase.{ph}.training");
                    view! {
                        <div style="display: flex; flex-direction: column; gap: 2px;">
                            <span class="is-size-7 has-text-grey">{move || t("cycle.day_label")}</span>
                            <span class="is-size-2 has-text-weight-bold">{day}</span>
                        </div>

                        <div>
                            <span class="is-size-5 has-text-weight-bold">{move || t(&name)}</span>
                            <p class="is-size-6 has-text-grey" style="margin: 4px 0 0; line-height: 1.45;">
                                {move || t(&desc)}
                            </p>
                        </div>

                        <div>
                            <span class="is-size-6 has-text-weight-semibold">{move || t("cycle.weight_heading")}</span>
                            <p class="is-size-6" style="margin: 4px 0 0; line-height: 1.5;">{move || t(&weight)}</p>
                        </div>

                        <div>
                            <span class="is-size-6 has-text-weight-semibold">{move || t("cycle.training_heading")}</span>
                            <p class="is-size-6" style="margin: 4px 0 0; line-height: 1.5;">{move || t(&training)}</p>
                        </div>

                        <div style="display: flex; align-items: center; justify-content: space-between; margin-top: 4px;">
                            <span class="is-size-7 has-text-grey">{move || t("cycle.first_day")}</span>
                            <span class="is-size-6 has-text-weight-semibold">{move || relative_date(&s)}</span>
                        </div>
                        <button class="button is-light is-fullwidth" on:click=open_dialog>
                            {move || t("cycle.set_first_day")}
                        </button>
                    }.into_view()
                }
                None => view! {
                    <p class="is-size-6 has-text-grey" style="line-height: 1.45;">
                        {move || t("cycle.set_prompt")}
                    </p>
                    <button class="button is-link is-fullwidth" on:click=open_dialog>
                        {move || t("cycle.set_first_day")}
                    </button>
                }.into_view(),
            }}
        </div>

        // Date dialog: pick the first day, default today. z above the page overlay.
        {move || dialog_open.get().then(|| view! {
            <div style="position: fixed; inset: 0; z-index: 70; display: flex; align-items: center; justify-content: center;">
                <div style="position: absolute; inset: 0; background: rgba(0,0,0,0.4);"
                    on:click=move |_| dialog_open.set(false)></div>
                <div style="position: relative; z-index: 1; background: var(--bulma-background); \
                            border-radius: 16px; padding: 18px; width: min(20rem, calc(100% - 3rem)); \
                            display: flex; flex-direction: column; gap: 14px;">
                    <span class="is-size-6 has-text-weight-bold">{move || t("cycle.set_first_day")}</span>
                    // The date reads as WORDS (Сегодня/Вчера/…). The real native date
                    // input sits transparently ON TOP, so a tap lands on it and opens
                    // the native picker directly (showPicker() is unreliable on iOS).
                    <div style="position: relative; width: 100%;">
                        <div style="font: inherit; padding: 12px; border-radius: 10px; text-align: center; \
                                border: 1px solid var(--bulma-border); background: var(--bulma-scheme-main); \
                                color: var(--bulma-text); pointer-events: none;">
                            {move || relative_date(&draft.get())}
                        </div>
                        <input type="date" max=today_str()
                            style="position: absolute; inset: 0; width: 100%; height: 100%; opacity: 0; \
                                cursor: pointer; -webkit-appearance: none; appearance: none;"
                            prop:value=move || draft.get()
                            on:change=move |ev| {
                                let v = event_target_value(&ev);
                                if !v.is_empty() { draft.set(v); }
                            }/>
                    </div>
                    <div style="display: flex; gap: 8px;">
                        <button class="button is-light" style="flex: 1;" on:click=move |_| dialog_open.set(false)>
                            {move || t("cycle.cancel")}
                        </button>
                        <button class="button is-link" style="flex: 1;" on:click=save>
                            {move || t("cycle.save")}
                        </button>
                    </div>
                </div>
            </div>
        })}
    }
}
