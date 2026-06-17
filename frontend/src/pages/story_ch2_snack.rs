use leptos::*;
use leptos_router::*;

use crate::services::{db, i18n::t, local};

const PAGE_BG: &str = "background: var(--bulma-background); min-height: 100vh; padding: 0; margin: -0.75rem;";
const CARD: &str = "background: var(--bulma-scheme-main); border-radius: 12px; overflow: hidden;";

#[component]
pub fn StoryCh2SnackPage() -> impl IntoView {
    let navigate = use_navigate();

    let paragraphs = ["story.ch2.snack.p1", "story.ch2.snack.p2", "story.ch2.snack.p3"];
    let body = paragraphs.iter().map(|&key| view! {
        <p class="is-size-6" style="line-height: 1.55; margin: 0 0 14px 0;">{move || t(key)}</p>
    }).collect_view();

    // Task: log a low-calorie snack. Reactive completion = report-ready(yesterday)
    // AND yesterday's diary contains a snack food.
    let diary_ver = db::version("diary");
    let summaries_ver = db::version("summaries");
    let done = create_rw_signal(false);
    let report_ready = create_rw_signal(false);
    create_effect(move |_| {
        diary_ver.get();
        summaries_ver.get();
        spawn_local(async move {
            let y = local::yesterday();
            let ready = local::report_ready_on(&y).await;
            report_ready.set(ready);
            done.set(ready && local::snack_logged_on(&y).await);
        });
    });

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

            <h1 class="is-size-1 has-text-weight-bold" style="margin: 0 16px 16px 16px;">{move || t("story.ch2.s4")}</h1>

            <div style="padding: 0 16px 8px 16px;">
                {body}
            </div>

            // ---- Task ----
            <div style="padding: 16px 16px 0 16px;">
                <p class="is-size-7 has-text-grey-light" style="text-transform: uppercase; letter-spacing: 0.02em; margin: 0 0 8px 4px;">
                    {move || t("story.ch2.snack.task_label")}
                </p>
                <div style=CARD>
                    <div style="display: flex; align-items: flex-start; gap: 12px; padding: 14px 16px;">
                        {move || if done.get() {
                            view! { <span style="font-size: 22px; width: 22px; text-align: center;">"\u{2705}"</span> }.into_view()
                        } else {
                            view! { <span style="font-size: 22px; width: 22px; text-align: center;">"\u{23f3}"</span> }.into_view()
                        }}
                        <span class="is-size-6 has-text-weight-semibold" style="flex: 1; line-height: 1.4;">{move || t("story.ch2.snack.task")}</span>
                    </div>
                </div>

                {move || if !report_ready.get() {
                    view! {
                        <p class="is-size-7 has-text-grey" style="margin: 8px 0 0 4px;">
                            {move || t("story.ch2.snack.no_report")}
                        </p>
                    }.into_view()
                } else {
                    view! {}.into_view()
                }}

                {move || done.get().then(|| view! {
                    <p class="is-size-6 has-text-weight-semibold has-text-success" style="margin-top: 16px;">
                        {move || t("story.ch2.mistake.next_unlocked")}
                    </p>
                })}

                <button
                    class="button is-link is-fullwidth is-medium"
                    style="margin-top: 16px;"
                    on:click={ let nav = navigate.clone(); move |_| nav("/diary", Default::default()) }
                >
                    {move || t("story.ch2.mistake.open_diary")}
                </button>
            </div>

            <div style="height: 40px;"></div>
        </div>
    }
}
