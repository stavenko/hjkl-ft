use std::cell::{Cell, RefCell};
use std::rc::Rc;

use leptos::*;
use leptos_router::use_navigate;
use wasm_bindgen::closure::Closure;
use wasm_bindgen::JsCast;

use crate::services::ai::AiPhase;
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
/// weekly report (computed on the following Monday). The day assessment is
/// generated by the model (grounded strictly on the day's facts) and cached in
/// IndexedDB; the "Переделать оценку" button re-runs the model.
#[component]
pub fn SummaryBlock(#[prop(into)] date: Signal<String>) -> impl IntoView {
    // Day assessment state.
    let day_text = create_rw_signal(None::<String>); // the rendered assessment, if any
    let ai_error = create_rw_signal(None::<String>);

    // Live AI-stream UI: phase 0=working, 1=thinking, 2=answering, plus token
    // counts. The "it's working" feedback is the token counters, which tick
    // straight off the stream events — no wall-clock timer / polling needed.
    let ai_loading = create_rw_signal(false);
    let ai_phase = create_rw_signal(0u8);
    let ai_think = create_rw_signal(0u32);
    let ai_answer = create_rw_signal(0u32);
    // Elapsed-seconds ticker. The model spends seconds REASONING on the server,
    // emitting NO tokens that reach us until the answer arrives in a burst at the
    // very end — so token counts alone leave the button frozen at ⌛. A 1s wall
    // clock is the only "it's working" signal during that phase (same as the AI
    // product search). `ai_start` = epoch ms when the run began.
    let ai_start = create_rw_signal(0f64);
    let ai_tick = create_rw_signal(0u32); // bumped ~once/second to re-render elapsed

    // Liveness + generation guards held in plain Rc<Cell>, NOT signals: an
    // in-flight model stream keeps calling `on_token` from a spawned task, and
    // if the user changes day (supersede) or leaves the page (dispose) those
    // late callbacks must NOT touch component signals — reading a disposed
    // signal panics. So we check these cheap, dispose-safe cells FIRST and only
    // touch signals while still alive and current.
    let alive = Rc::new(Cell::new(true));
    let gen = Rc::new(Cell::new(0u32));
    on_cleanup({
        let alive = alive.clone();
        move || alive.set(false)
    });

    let elapsed = move || {
        ai_tick.get(); // re-render each tick
        ((js_sys::Date::now() - ai_start.get_untracked()) / 1000.0).max(0.0) as u32
    };

    let navigate = use_navigate();

    // Generate (or re-generate) the assessment for the current date. `force`
    // drops the cached entry first. Drives the live stream UI throughout.
    // A Callback (Copy) so it can be reused by both the effect and the button.
    let run = Callback::new(move |force: bool| {
        if !alive.get() {
            return;
        }
        let d = date.get_untracked();
        let navigate = navigate.clone();
        // Supersede any previous run.
        let my_id = gen.get().wrapping_add(1);
        gen.set(my_id);
        ai_loading.set(true);
        ai_error.set(None);
        ai_phase.set(0);
        ai_think.set(0);
        ai_answer.set(0);
        ai_tick.set(0);
        ai_start.set(js_sys::Date::now());
        // "Still working" elapsed-seconds ticker — a SELF-RESCHEDULING setTimeout,
        // not setInterval. Each fire bumps ai_tick (re-renders the "Ns" display)
        // and schedules the next ONLY while this run is still alive + current, so
        // it stops itself on supersede/dispose. The closure is parked in `holder`
        // (an Rc) to outlive each tick; on stop it `take()`s itself, breaking the
        // cycle so it's dropped. No handle to track, nothing to clear.
        {
            let alive_t = alive.clone();
            let gen_t = gen.clone();
            let holder: Rc<RefCell<Option<Closure<dyn Fn()>>>> = Rc::new(RefCell::new(None));
            let holder2 = holder.clone();
            *holder.borrow_mut() = Some(Closure::<dyn Fn()>::new(move || {
                // Keep ticking only while alive (dispose-safe Rc, checked FIRST),
                // current, AND still loading. `ai_loading` is read only after the
                // alive check short-circuits, so it's never touched post-dispose.
                let keep = alive_t.get() && gen_t.get() == my_id && ai_loading.get_untracked();
                if !keep {
                    holder2.borrow_mut().take(); // done — drop the closure (breaks the cycle)
                    return;
                }
                ai_tick.update(|v| *v += 1);
                if let (Some(w), Some(cb)) = (web_sys::window(), holder2.borrow().as_ref()) {
                    let _ = w.set_timeout_with_callback_and_timeout_and_arguments_0(
                        cb.as_ref().unchecked_ref(), 1000,
                    );
                }
            }));
            {
                let b = holder.borrow();
                if let (Some(w), Some(cb)) = (web_sys::window(), b.as_ref()) {
                    let _ = w.set_timeout_with_callback_and_timeout_and_arguments_0(
                        cb.as_ref().unchecked_ref(), 1000,
                    );
                }
            }
        }

        let alive_tok = alive.clone();
        let gen_tok = gen.clone();
        let alive_done = alive.clone();
        let gen_done = gen.clone();
        spawn_local(async move {
            let on_token = move |phase: AiPhase| {
                // Dispose-safe guard: bail before touching any signal.
                if !alive_tok.get() || gen_tok.get() != my_id {
                    return;
                }
                match phase {
                    AiPhase::Thinking => {
                        ai_think.update(|v| *v += 1);
                        if ai_phase.get_untracked() == 0 { ai_phase.set(1); }
                    }
                    AiPhase::Answer => {
                        ai_answer.update(|v| *v += 1);
                        if ai_phase.get_untracked() != 2 { ai_phase.set(2); }
                    }
                }
            };
            // Race the generation against a timeout: a stalled SSE stream (e.g.
            // a mobile reconnect mid-request) would otherwise leave `output.await`
            // pending forever — ai_loading stuck true, button dead. On timeout we
            // surface an error so the button frees up; the stale request is
            // abandoned (the gen guard ignores its late writes).
            let gen_fut = async {
                if force {
                    summary::regenerate_day(&d, on_token).await
                } else {
                    summary::ensure_day(&d, on_token).await
                }
            };
            let timeout = crate::services::ai::sleep_ms(90_000);
            futures::pin_mut!(gen_fut, timeout);
            let res = match futures::future::select(gen_fut, timeout).await {
                futures::future::Either::Left((r, _)) => r,
                futures::future::Either::Right(_) => Err(t("summary.gen_failed").to_string()),
            };
            // Discard if the component unmounted or a newer run superseded us.
            if !alive_done.get() || gen_done.get() != my_id {
                return;
            }
            // Flips the button out of loading; the ticker also watches ai_loading
            // and stops itself on the next tick once this is false.
            ai_loading.set(false);
            match res {
                Ok(Some(s)) => day_text.set(Some(s.text)),
                Ok(None) => day_text.set(None),
                Err(e) => {
                    if e.contains("HTTP 402") {
                        navigate("/paywall", Default::default());
                    } else {
                        ai_error.set(Some(e));
                    }
                }
            }
        });
    });

    // Load the existing assessment for the selected day — DISPLAY only, NO
    // generation. The report is produced on app activation (lib.rs), not when a
    // day is opened. Reactive to summaries writes, so an activation-time
    // generation appears here live; the "Переделать" button below regenerates.
    let sum_ver = db::version("summaries");
    create_effect(move |_| {
        let d = date.get();
        sum_ver.get();
        ai_error.set(None);
        spawn_local(async move {
            day_text.set(summary::get_day(&d).await.map(|s| s.text));
        });
    });

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
            // Assessment card: the result, or a "generating…" placeholder during
            // the first generation. Empty days (loaded, no assessment) render nothing.
            {move || {
                if let Some(text) = day_text.get() {
                    day_card(text)
                } else if ai_loading.get() {
                    view! {
                        <div style=CARD>
                            <p class="is-size-7 has-text-grey">{move || t("summary.generating")}</p>
                        </div>
                    }.into_view()
                } else {
                    ().into_view()
                }
            }}

            {move || ai_error.get().map(|e| view! {
                <p class="is-size-7 has-text-danger" style="margin: 0.25rem 0;">{e}</p>
            })}

            // Regenerate button. Shown when there's an assessment, one is being
            // generated, OR generation failed (so a stuck/errored run can always
            // be retried). NOT disabled while loading: tapping supersedes a stalled
            // run, so the user is never stuck with a dead button.
            {move || (day_text.get().is_some() || ai_loading.get() || ai_error.get().is_some()).then(|| view! {
                <button type="button"
                    class="button is-link is-size-7 is-fullwidth"
                    style="border: none; border-radius: 10px; cursor: pointer; margin-top: 0.25rem;"
                    on:click=move |_| run.call(true)
                >
                    {move || if ai_loading.get() {
                        match ai_phase.get() {
                            1 => format!("\u{1f9e0} Thinking ({} tok) \u{00b7} {}s", ai_think.get(), elapsed()),
                            2 => format!("\u{270d}\u{fe0f} Answer ({} tok) \u{00b7} {}s", ai_answer.get(), elapsed()),
                            _ => format!("\u{231b} {}s", elapsed()),
                        }
                    } else {
                        format!("\u{2728} {}", t("summary.regenerate"))
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
