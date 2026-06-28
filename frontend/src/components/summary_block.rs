use leptos::*;
use leptos_router::use_navigate;

use crate::services::db;
use crate::services::i18n::t;
use crate::services::summary;

const CARD: &str = "background: var(--bulma-scheme-main-bis); border-radius: 12px; padding: 14px 16px; margin: 1rem 0 0.5rem 0;";
const SECTION_LABEL: &str = "margin: 0 0 6px 0; text-transform: uppercase; letter-spacing: 0.02em;";

// Each assessment section sits in its own tinted block. Colours are Bulma 1.0
// theme-aware CSS vars, so they flip automatically between light/dark themes and
// keep `--bulma-text` readable: neutral grey for facts, soft green for "good",
// soft yellow for "improve".
// Neutral grey via a translucent mid-grey overlay: it reads as "slightly darker"
// on a light theme and "slightly lighter" on a dark one, so it stays a visible
// neutral block either way (scheme-main-ter was too close to the page bg).
const FACTS_BLOCK: &str = "background: rgba(128, 128, 128, 0.14); border-radius: 12px; padding: 12px 14px; margin: 1rem 0 0.5rem 0;";
const GOOD_BLOCK: &str = "background: var(--bulma-success-soft); border-radius: 12px; padding: 12px 14px; margin-bottom: 0.5rem;";
const IMPROVE_BLOCK: &str = "background: var(--bulma-warning-soft); border-radius: 12px; padding: 12px 14px; margin-bottom: 0.5rem;";

/// Render a list of summary bullets; an item with a `url` gets a source link
/// that opens in the system browser (target=_blank).
fn items_list(items: Vec<summary::SummaryItem>) -> View {
    items
        .into_iter()
        .map(|it| {
            let link = it.url.filter(|u| !u.is_empty()).map(|u| {
                view! {
                    " "
                    <a href=u target="_blank" rel="noopener noreferrer" class="is-size-7 has-text-link">
                        {t("summary.source")}" \u{2197}"
                    </a>
                }
            });
            view! { <li class="is-size-6" style="margin-bottom: 4px; line-height: 1.4;">{it.text}{link}</li> }
        })
        .collect_view()
}

/// The deterministic "Facts" block: total КБЖУ for the day + vegetable/fruit count.
fn facts_block(f: summary::DayFacts) -> View {
    let g = t("common.unit.g");
    let row = |label: &str, val: f64, unit: &str| {
        view! {
            <li class="is-size-6" style="margin-bottom: 4px;">
                {format!("{}: {:.0} {}", label, val, unit)}
            </li>
        }
    };
    view! {
        <div style=FACTS_BLOCK>
            <p class="is-size-7 has-text-weight-semibold has-text-grey" style=SECTION_LABEL>{t("summary.facts_title")}</p>
            <ul style="margin: 0; padding-left: 1.1rem; list-style: disc;">
                {row(t("food_editor.calories"), f.kcal, t("common.unit.kcal"))}
                {row(t("food_editor.protein"), f.protein, g)}
                {row(t("food_editor.fat"), f.fat, g)}
                {row(t("food_editor.carbs"), f.carbs, g)}
                {row(t("summary.facts_veg_fruit"), f.veg_fruit_grams, g)}
            </ul>
        </div>
    }
    .into_view()
}

/// The daily assessment card: "facts" + "what went well" + "what to improve" (or
/// a single "great job" line when there's nothing to improve). Falls back to
/// plain text for legacy free-text summaries that aren't JSON.
fn day_card(text: String) -> View {
    match summary::parse_day(&text) {
        None => view! {
            <div style=CARD>
                <p class="is-size-7 has-text-weight-semibold has-text-grey" style=SECTION_LABEL>{t("summary.day_title")}</p>
                <p class="is-size-6" style="white-space: pre-wrap; line-height: 1.45;">{text}</p>
            </div>
        }
        .into_view(),
        Some(ds) => view! {
            <div>
                {ds.facts.map(facts_block)}
                {(!ds.good.is_empty()).then(|| view! {
                    <div style=GOOD_BLOCK>
                        <p class="is-size-7 has-text-weight-semibold" style=SECTION_LABEL>{t("summary.good_title")}</p>
                        <ul style="margin: 0; padding-left: 1.1rem; list-style: disc;">{items_list(ds.good)}</ul>
                    </div>
                })}
                <div style=IMPROVE_BLOCK>
                    <p class="is-size-7 has-text-weight-semibold" style=SECTION_LABEL>{t("summary.improve_title")}</p>
                    {if ds.improve.is_empty() {
                        view! { <p class="is-size-6">{t("summary.all_good")}</p> }.into_view()
                    } else {
                        view! { <ul style="margin: 0; padding-left: 1.1rem; list-style: disc;">{items_list(ds.improve)}</ul> }.into_view()
                    }}
                </div>
            </div>
        }
        .into_view(),
    }
}

/// Daily AI assessment shown under the food list on PAST diary days, plus the
/// weekly report. The day assessment is produced by an APP-SCOPED background
/// generator (`summary::start_day_generation`) — this component only DISPLAYS the
/// stored result / error and OBSERVES the generator's live status, so leaving and
/// returning to the page never loses progress, the error, or the report.
#[component]
pub fn SummaryBlock(#[prop(into)] date: Signal<String>) -> impl IntoView {
    let day_text = create_rw_signal(None::<String>); // last good report, if any
    let ai_error = create_rw_signal(None::<String>); // last persisted generation error
    let navigate = use_navigate();

    // Load the stored assessment + last error for the selected day (DISPLAY only).
    // Reactive to `summaries` writes, so when the background generator finishes
    // (success or error) the result/error appears here live. A persisted 402 error
    // means the subscription lapsed → send the user to the paywall.
    let sum_ver = db::version("summaries");
    {
        let navigate = navigate.clone();
        create_effect(move |_| {
            let d = date.get();
            sum_ver.get();
            let navigate = navigate.clone();
            spawn_local(async move {
                match summary::get_day(&d).await {
                    Some(s) => {
                        day_text.set((!s.text.is_empty()).then_some(s.text));
                        if let Some(err) = s.error.as_ref() {
                            if err.contains("HTTP 402") {
                                navigate("/settings/subscription", Default::default());
                            }
                        }
                        ai_error.set(s.error);
                    }
                    None => {
                        day_text.set(None);
                        ai_error.set(None);
                    }
                }
            });
        });
    }

    // Live status of the background generator, for THIS day (None when idle).
    let progress = move || {
        let d = date.get();
        summary::gen_active().get().filter(|g| g.date == d)
    };
    let elapsed =
        move || progress().map(|g| ((js_sys::Date::now() - g.started_ms) / 1000.0).max(0.0) as u32);

    // Weekly report state. `week_start` is the Monday of the selected day's week.
    let show_week = create_rw_signal(false);
    let week = create_resource(
        move || (date.get(), show_week.get()),
        |(d, show)| async move {
            if !show {
                return None;
            }
            let ws = summary::week_start_of(&d);
            summary::ensure_week(&ws).await.map(|s| s.text)
        },
    );

    view! {
        <div>
            // Assessment card: the stored result, or a live "generating…" placeholder
            // while the background generator runs for this day. Empty days render nothing.
            {move || {
                if let Some(text) = day_text.get() {
                    day_card(text)
                } else if progress().is_some() {
                    view! {
                        <div style=CARD>
                            <p class="is-size-7 has-text-grey">{move || t("summary.generating")}</p>
                        </div>
                    }.into_view()
                } else {
                    ().into_view()
                }
            }}

            // Last persisted error (e.g. a failed regeneration). Any good report
            // above is preserved — the assessment is overwritten only on success.
            {move || ai_error.get().map(|e| view! {
                <p class="is-size-7 has-text-danger" style="margin: 0.25rem 0;">{e}</p>
            })}

            // Regenerate button: shown when there's a stored result, a stored error,
            // or a run in flight. Re-runs in the background generator (idempotent per
            // day, so a tap mid-run is a harmless no-op).
            {move || (day_text.get().is_some() || ai_error.get().is_some() || progress().is_some()).then(|| view! {
                <button type="button"
                    class="button is-link is-size-7 is-fullwidth"
                    style="border: none; border-radius: 10px; cursor: pointer; margin-top: 0.25rem;"
                    on:click=move |_| summary::start_day_generation(date.get_untracked(), true)
                >
                    {move || match progress() {
                        Some(g) => match g.phase {
                            1 => format!("\u{1f9e0} Thinking ({} tok) \u{00b7} {}s", g.think, elapsed().unwrap_or(0)),
                            2 => format!("\u{270d}\u{fe0f} Answer ({} tok) \u{00b7} {}s", g.answer, elapsed().unwrap_or(0)),
                            _ => format!("\u{231b} {}s", elapsed().unwrap_or(0)),
                        },
                        None => format!("\u{2728} {}", t("summary.regenerate")),
                    }}
                </button>
            })}

            // Weekly report: either show the ready report (button → generate),
            // or the "will be computed on <date>" notice until the week ends.
            {move || {
                let ws = summary::week_start_of(&date.get());
                if summary::week_ready(&ws) {
                    if show_week.get() {
                        view! {
                            {move || match week.get() {
                                Some(Some(text)) => view! {
                                    <div style=CARD>
                                        <p class="is-size-7 has-text-weight-semibold has-text-grey" style="margin: 0 0 6px 0; text-transform: uppercase; letter-spacing: 0.02em;">
                                            {move || t("summary.week_title")}
                                        </p>
                                        <p class="is-size-6" style="white-space: pre-wrap; line-height: 1.45;">{text}</p>
                                    </div>
                                }.into_view(),
                                Some(None) => ().into_view(),
                                None => view! {
                                    <div style=CARD>
                                        <p class="is-size-7 has-text-grey">{move || t("summary.generating")}</p>
                                    </div>
                                }.into_view(),
                            }}
                        }.into_view()
                    } else {
                        view! {
                            <button
                                class="button is-light is-fullwidth is-rounded"
                                style="margin-top: 0.25rem;"
                                on:click=move |_| show_week.set(true)
                            >{move || t("summary.week_button")}</button>
                        }.into_view()
                    }
                } else {
                    view! {
                        <p class="is-size-7 has-text-grey" style="text-align: center; margin-top: 0.5rem;">
                            {move || format!("{} {}", t("summary.week_pending"), summary::next_monday(&ws))}
                        </p>
                    }.into_view()
                }
            }}
        </div>
    }
}
