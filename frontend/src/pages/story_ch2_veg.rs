use leptos::*;
use leptos_router::*;

use crate::services::{db, i18n::t, profile, summary};

const PAGE_BG: &str = "background: var(--bulma-background); min-height: 100vh; padding: 0; margin: -0.75rem;";
const CARD: &str = "background: var(--bulma-scheme-main); border-radius: 12px; overflow: hidden;";

/// Daily vegetable/fruit target by biological sex: 600 g for women, 800 g for
/// men. Defaults to the male target when sex is unset.
fn veg_fruit_target() -> f64 {
    match profile::get_sex() {
        Some(profile::Sex::Female) => 600.0,
        Some(profile::Sex::Male) | None => 800.0,
    }
}

#[component]
pub fn StoryCh2VegPage() -> impl IntoView {
    let navigate = use_navigate();

    // Post-factum: read YESTERDAY's veg_fruit_grams from the stored daily
    // summary. None when that day's summary is absent / had no food / unparseable.
    let summaries_ver = db::version("summaries");
    let grams = create_rw_signal::<Option<f64>>(None);
    create_effect(move |_| {
        summaries_ver.get();
        spawn_local(async move {
            let yesterday = (chrono::Local::now() - chrono::Duration::days(1))
                .format("%Y-%m-%d")
                .to_string();
            let value = match summary::get_day(&yesterday).await {
                Some(s) => summary::parse_day(&s.text).and_then(|d| d.facts.map(|f| f.veg_fruit_grams)),
                None => None,
            };
            grams.set(value);
        });
    });

    let target = veg_fruit_target();

    let paragraphs = ["story.ch2.veg.p1", "story.ch2.veg.p2"];
    let body = paragraphs.iter().map(|&key| view! {
        <p class="is-size-6" style="line-height: 1.55; margin: 0 0 14px 0;">{move || t(key)}</p>
    }).collect_view();

    view! {
        <div style=PAGE_BG>
            <div style="display: flex; align-items: center; padding: 12px 16px;">
                <button
                    style="appearance: none; -webkit-appearance: none; border: none; background: none; cursor: pointer; padding: 4px; font: inherit;"
                    class="is-size-5"
                    on:click={ let nav = navigate.clone(); move |_| nav("/", Default::default()) }
                >
                    <span class="has-text-link">{move || t("common.back")}</span>
                </button>
            </div>

            <h1 class="is-size-1 has-text-weight-bold" style="margin: 0 16px 16px 16px;">{move || t("story.ch2.s2")}</h1>

            <div style="padding: 0 16px 8px 16px;">
                {body}
            </div>

            // ---- Yesterday's vegetables/fruit vs target ----
            <div style="padding: 16px 16px 0 16px;">
                <p class="is-size-7 has-text-grey-light" style="text-transform: uppercase; letter-spacing: 0.02em; margin: 0 0 8px 4px;">
                    {move || t("story.ch2.veg.target_label")}
                </p>
                <div style=CARD>
                    <div style="display: flex; align-items: center; justify-content: center; padding: 18px 16px;">
                        {move || match grams.get() {
                            Some(g) => view! {
                                <span class="is-size-3 has-text-weight-bold">
                                    {format!("{} / {} \u{0433}", g.round() as i64, target.round() as i64)}
                                </span>
                            }.into_view(),
                            None => view! {
                                <span class="is-size-6 has-text-grey">{move || t("story.ch2.veg.no_data")}</span>
                            }.into_view(),
                        }}
                    </div>
                </div>
            </div>

            <div style="height: 40px;"></div>
        </div>
    }
}
