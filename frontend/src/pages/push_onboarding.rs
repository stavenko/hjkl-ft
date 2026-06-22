use leptos::*;

use crate::services::{i18n::t, push};

#[component]
pub fn PushOnboarding(on_done: Callback<()>) -> impl IntoView {
    let loading = create_rw_signal(false);
    let error = create_rw_signal(None::<String>);

    // Ask for notification permission only. The per-meal schedule is NOT set up
    // here — reminders are introduced gradually through the Story (and can be
    // tuned in Settings). On grant (or skip) we finish onboarding.
    let allow = move |_| {
        loading.set(true);
        error.set(None);
        spawn_local(async move {
            match push::subscribe().await {
                Ok(()) => on_done.call(()),
                Err(e) => {
                    error.set(Some(e));
                    loading.set(false);
                }
            }
        });
    };

    let skip = move |_| {
        push::dismiss_onboarding();
        on_done.call(());
    };

    view! {
        <div
            attr:data-testid="push-onboarding"
            style="display: flex; flex-direction: column; align-items: center; justify-content: center; min-height: 100vh; padding: 32px; background: var(--bulma-scheme-main); text-align: center;"
        >
            <div attr:data-testid="push-onboarding-step-1">
                <div style="font-size: 64px; margin-bottom: 24px;">"🔔"</div>

                <h1
                    attr:data-testid="push-onboarding-title"
                    class="is-size-3 has-text-weight-bold"
                    style="margin: 0 0 16px 0;"
                >
                    {move || t("push_onboarding.title")}
                </h1>

                <p
                    attr:data-testid="push-onboarding-description"
                    class="is-size-6 has-text-grey-light"
                    style="max-width: 320px; line-height: 1.5; margin: 0 auto 32px auto;"
                >
                    {move || t("push_onboarding.description")}
                </p>

                {move || error.get().map(|e| view! {
                    <p class="is-size-7 has-text-danger" style="margin-bottom: 16px;">{e}</p>
                })}

                <button
                    attr:data-testid="push-onboarding-btn-allow"
                    class="button is-link is-size-5 has-text-weight-semibold"
                    style="border: none; border-radius: 12px; padding: 14px 32px; cursor: pointer; width: 100%; max-width: 320px;"
                    prop:disabled=move || loading.get()
                    on:click=allow
                >
                    {move || t("push_onboarding.allow")}
                </button>

                <button
                    attr:data-testid="push-onboarding-btn-skip"
                    class="has-text-grey-light is-size-6"
                    style="background: none; border: none; cursor: pointer; margin-top: 16px; padding: 8px;"
                    on:click=skip
                >
                    {move || t("push_onboarding.skip")}
                </button>
            </div>
        </div>
    }
}
