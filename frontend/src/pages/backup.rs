use leptos::*;
use leptos_router::*;

use crate::services::{auth, i18n::t};

/// Backup access: generate / re-show the account's recovery phrase (a 5-word,
/// user-language phrase minted by the model). The phrase is stored plaintext so it can
/// be shown again here at any time; logging in with it is username-less (see AuthPage).
#[component]
pub fn BackupPage() -> impl IntoView {
    let navigate = use_navigate();

    let phrase = create_rw_signal(None::<String>);
    let loading = create_rw_signal(true); // initial fetch
    let generating = create_rw_signal(false);
    let error = create_rw_signal(None::<String>);

    // Load the current phrase (if any) on mount.
    spawn_local(async move {
        match auth::get_backup_phrase().await {
            Ok(p) => phrase.set(p),
            Err(e) => error.set(Some(e)),
        }
        loading.set(false);
    });

    // Generate a fresh phrase and store it. Retries a few times on the rare uniqueness
    // collision (`taken`) before giving up.
    let do_generate = move |_| {
        generating.set(true);
        error.set(None);
        spawn_local(async move {
            for _ in 0..3 {
                let p = match auth::generate_backup_phrase().await {
                    Ok(p) => p,
                    Err(e) => {
                        error.set(Some(e));
                        generating.set(false);
                        return;
                    }
                };
                match auth::set_backup_phrase(&p).await {
                    Ok(s) if s == "ok" => {
                        phrase.set(Some(p));
                        generating.set(false);
                        return;
                    }
                    // Collision / too short → try another phrase.
                    Ok(s) if s == "taken" || s == "too_short" => continue,
                    Ok(s) => {
                        error.set(Some(format!("{}: {s}", t("backup.retry_failed"))));
                        generating.set(false);
                        return;
                    }
                    Err(e) => {
                        error.set(Some(e));
                        generating.set(false);
                        return;
                    }
                }
            }
            error.set(Some(t("backup.retry_failed").to_string()));
            generating.set(false);
        });
    };

    view! {
        <div style="background: var(--bulma-background); min-height: 100vh; padding: 0; margin: -0.75rem;">

            // Nav bar
            <div style="display: flex; align-items: center; padding: 12px 16px; background: var(--bulma-background);">
                <button
                    attr:data-testid="backup-btn-back"
                    class="has-text-link is-size-5"
                    style="background: none; border: none; cursor: pointer; padding: 0; display: flex; align-items: center; gap: 4px;"
                    on:click={
                        let nav = navigate.clone();
                        move |_| { let nav = nav.clone(); nav("/settings", Default::default()); }
                    }
                >
                    {move || t("backup.back")}
                </button>
                <h1 class="is-size-5 has-text-weight-semibold" style="margin: 0 auto;">{move || t("backup.title")}</h1>
                <span class="is-size-5" style="visibility: hidden;">{move || t("backup.back")}</span>
            </div>

            <div style="padding: 0 16px; max-width: 32rem; margin: 0 auto;">
                <p class="is-size-6 has-text-grey" style="line-height: 1.6; margin: 8px 0 20px 0;">
                    {move || t("backup.desc")}
                </p>

                {move || error.get().map(|e| view! {
                    <div class="notification is-danger is-light" style="text-align: left;">{e}</div>
                })}

                {move || if loading.get() {
                    view! { <div class="ft-spinner" style="margin: 24px auto;"></div> }.into_view()
                } else {
                    view! {
                        {move || phrase.get().map(|p| view! {
                            <div
                                attr:data-testid="backup-phrase"
                                style="background: var(--bulma-scheme-main); border-radius: 12px; padding: 20px; margin-bottom: 12px; font-size: 1.25rem; font-weight: 600; letter-spacing: 0.02em; line-height: 1.7; text-align: center; word-spacing: 0.3em;"
                            >
                                {p}
                            </div>
                            <p class="is-size-7 has-text-grey" style="line-height: 1.5; margin-bottom: 20px;">
                                {move || t("backup.warning")}
                            </p>
                        })}

                        <button
                            attr:data-testid="backup-btn-generate"
                            class="button is-link is-fullwidth has-text-weight-semibold"
                            disabled=move || generating.get()
                            on:click=do_generate
                        >
                            {move || if generating.get() {
                                t("backup.generating")
                            } else if phrase.get().is_some() {
                                t("backup.regenerate")
                            } else {
                                t("backup.generate")
                            }}
                        </button>
                    }.into_view()
                }}
            </div>
        </div>
    }
}
