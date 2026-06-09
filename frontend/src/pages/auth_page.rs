use leptos::*;
use crate::services::auth;
use crate::services::i18n::t;

#[component]
pub fn AuthPage(on_authenticated: Callback<()>) -> impl IntoView {
    let loading = create_rw_signal(false);
    let error = create_rw_signal(None::<String>);

    let on_register = move |_| {
        loading.set(true);
        error.set(None);
        spawn_local(async move {
            match auth::register().await {
                Ok(_) => on_authenticated.call(()),
                Err(e) => {
                    error.set(Some(e));
                    loading.set(false);
                }
            }
        });
    };

    let on_login = move |_| {
        loading.set(true);
        error.set(None);
        spawn_local(async move {
            match auth::authenticate().await {
                Ok(_) => on_authenticated.call(()),
                Err(e) => {
                    error.set(Some(e));
                    loading.set(false);
                }
            }
        });
    };

    view! {
        <div style="min-height: 100vh; display: flex; flex-direction: column; align-items: center; justify-content: center; padding: 2rem; text-align: center; background: white;">
            <div style="max-width: 24rem; width: 100%;">
                <img src="/icon-192.png" alt="Food Tracker" style="width: 80px; height: 80px; border-radius: 16px; margin-bottom: 1rem;" />
                <h1 class="title is-3" style="margin-bottom: 0.5rem;">"Food Tracker"</h1>
                <p class="has-text-grey mb-5" style="font-size: 1.05rem;">
                    {t("auth.subtitle")}
                </p>

                {move || error.get().map(|e| view! {
                    <div class="notification is-danger is-light mb-4" style="text-align: left;">
                        <button class="delete" on:click=move |_| error.set(None)></button>
                        {e}
                    </div>
                })}

                <div style="display: flex; flex-direction: column; gap: 1rem;">
                    <button
                        class="button is-link is-medium is-fullwidth"
                        disabled=move || loading.get()
                        on:click=on_register
                    >
                        {move || if loading.get() { t("auth.creating") } else { t("auth.create_account") }}
                    </button>
                    <button
                        class="button is-light is-medium is-fullwidth"
                        disabled=move || loading.get()
                        on:click=on_login
                    >
                        {move || if loading.get() { t("auth.authenticating") } else { t("auth.login_device") }}
                    </button>
                </div>
            </div>
        </div>
    }
}
