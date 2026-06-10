use leptos::*;
use crate::services::auth;
use crate::services::i18n::t;
use crate::components::qr_code::QrCode;
use crate::components::qr_scanner::QrScanner;

#[derive(Clone, PartialEq)]
enum AuthStep {
    Main,
    Login,
    ShowQr { qr_url: String, pairing_id: String },
    Scanning,
}

#[component]
pub fn AuthPage(on_authenticated: Callback<()>) -> impl IntoView {
    let step = create_rw_signal(AuthStep::Main);
    let loading = create_rw_signal(false);
    let error = create_rw_signal(None::<String>);
    let display_name = create_rw_signal(String::new());

    let on_register = move |_| {
        let name = display_name.get_untracked();
        loading.set(true);
        error.set(None);
        spawn_local(async move {
            match auth::register(&name).await {
                Ok(_) => on_authenticated.call(()),
                Err(e) => {
                    error.set(Some(e));
                    loading.set(false);
                }
            }
        });
    };

    let on_try_passkey = move |_| {
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

    let on_show_qr = move |_| {
        loading.set(true);
        error.set(None);
        spawn_local(async move {
            match auth::pair_request().await {
                Ok(resp) => {
                    let qr_url = resp.get("qr_url")
                        .and_then(|v| v.as_str())
                        .unwrap_or_default()
                        .to_string();
                    let pairing_id = resp.get("pairing_id")
                        .and_then(|v| v.as_str())
                        .unwrap_or_default()
                        .to_string();
                    let secret = resp.get("secret")
                        .and_then(|v| v.as_str())
                        .unwrap_or_default()
                        .to_string();
                    step.set(AuthStep::ShowQr { qr_url, pairing_id: pairing_id.clone() });
                    loading.set(false);

                    // Poll: check status until approved, then claim
                    let pid = pairing_id;
                    let sec = secret;
                    spawn_local(async move {
                        for _ in 0..150 { // 5 minutes max
                            sleep_ms(2000).await;
                            match auth::pair_check(&pid, &sec).await {
                                Ok(status) => {
                                    let s = status.get("status")
                                        .and_then(|v| v.as_str())
                                        .unwrap_or("pending");
                                    if s == "approved" || s == "claimed" {
                                        // Now claim and create PassKey
                                        match auth::pair_claim(&pid, &sec).await {
                                            Ok(_) => {
                                                on_authenticated.call(());
                                                return;
                                            }
                                            Err(e) => {
                                                error.set(Some(e));
                                                step.set(AuthStep::Login);
                                                return;
                                            }
                                        }
                                    } else if s == "expired" {
                                        error.set(Some(t("pair.expired").to_string()));
                                        step.set(AuthStep::Login);
                                        return;
                                    }
                                    // pending — keep polling
                                }
                                Err(_) => {
                                    // Network error — keep trying
                                }
                            }
                        }
                        error.set(Some(t("pair.expired").to_string()));
                        step.set(AuthStep::Login);
                    });
                }
                Err(e) => {
                    error.set(Some(e));
                    loading.set(false);
                }
            }
        });
    };

    let on_scan = Callback::new(move |data: String| {
        error.set(None);
        loading.set(true);
        if let Some((pairing_id, secret)) = parse_pair_url(&data) {
            spawn_local(async move {
                match auth::pair_claim(&pairing_id, &secret).await {
                    Ok(_) => on_authenticated.call(()),
                    Err(e) => {
                        error.set(Some(e));
                        loading.set(false);
                        step.set(AuthStep::Login);
                    }
                }
            });
        } else {
            error.set(Some(t("pair.error_invalid_qr").to_string()));
            loading.set(false);
            step.set(AuthStep::Login);
        }
    });

    let error_view = move || {
        error.get().map(|e| view! {
            <div attr:data-testid="auth-error" class="notification is-danger is-light mb-4" style="text-align: left;">
                <button attr:data-testid="auth-btn-dismiss-error" class="delete" on:click=move |_| error.set(None)></button>
                {e}
            </div>
        })
    };

    view! {
        {move || match step.get() {
            AuthStep::ShowQr { ref qr_url, ref pairing_id } => {
                let url = qr_url.clone();
                let url_copy = qr_url.clone();
                let copied = create_rw_signal(false);
                view! {
                    <div style="min-height: 100vh; display: flex; flex-direction: column; align-items: center; justify-content: center; padding: 2rem; text-align: center; background: white;">
                        <div style="max-width: 24rem; width: 100%;">
                            <h1 class="title is-4 mb-4">{t("pair.show_qr")}</h1>
                            <p class="has-text-grey mb-4">{t("auth.show_qr_hint")}</p>
                            <div attr:data-testid="auth-qr-display" style="display: flex; justify-content: center; margin-bottom: 1rem;">
                                <QrCode data=url size=240 />
                            </div>
                            <button
                                attr:data-testid="auth-btn-copy-link"
                                class="button is-small is-light is-fullwidth mb-3"
                                on:click=move |_| {
                                    let u = url_copy.clone();
                                    spawn_local(async move {
                                        let window = web_sys::window().expect("no window");
                                        let clipboard = window.navigator().clipboard();
                                        let _ = wasm_bindgen_futures::JsFuture::from(clipboard.write_text(&u)).await;
                                        copied.set(true);
                                    });
                                }
                            >
                                {move || if copied.get() { t("qr.copied") } else { t("qr.copy_link") }}
                            </button>
                            <p class="has-text-grey is-size-7 mb-4">{t("pair.waiting")}</p>
                            <button
                                attr:data-testid="auth-btn-back"
                                class="button is-ghost has-text-grey"
                                style="text-decoration: underline;"
                                on:click=move |_| step.set(AuthStep::Login)
                            >{t("auth.back")}</button>
                        </div>
                    </div>
                }.into_view()
            }

            AuthStep::Scanning => view! {
                <QrScanner
                    on_scan=on_scan
                    on_close=Callback::new(move |_| step.set(AuthStep::Login))
                />
            }.into_view(),

            AuthStep::Login => view! {
                <div style="min-height: 100vh; display: flex; flex-direction: column; align-items: center; justify-content: center; padding: 2rem; text-align: center; background: white;">
                    <div style="max-width: 24rem; width: 100%;">
                        <img src="/icon-192.png" alt="Food Tracker" style="width: 64px; height: 64px; border-radius: 12px; margin-bottom: 1rem;" />
                        <h1 class="title is-4" style="margin-bottom: 1.5rem;">{t("auth.login_title")}</h1>

                        {error_view}

                        // Section 1: pair via another device
                        <div style="text-align: left; margin-bottom: 1.5rem;">
                            <p class="has-text-weight-semibold mb-3">{t("auth.login_have_device")}</p>

                            <div class="box mb-3" style="padding: 0.75rem;">
                                <p class="is-size-7 has-text-grey mb-2">{t("auth.login_option1_hint")}</p>
                                <button
                                    attr:data-testid="auth-btn-show-qr"
                                    class="button is-link is-fullwidth"
                                    disabled=move || loading.get()
                                    on:click=on_show_qr
                                >
                                    {t("pair.show_qr")}
                                </button>
                            </div>

                            <div class="box" style="padding: 0.75rem;">
                                <p class="is-size-7 has-text-grey mb-2">{t("auth.login_option2_hint")}</p>
                                <button
                                    attr:data-testid="auth-btn-scan-qr"
                                    class="button is-link is-light is-fullwidth"
                                    disabled=move || loading.get()
                                    on:click=move |_| step.set(AuthStep::Scanning)
                                >
                                    {t("pair.scan_qr")}
                                </button>
                            </div>
                        </div>

                        // Section 2: no logged-in device
                        <div style="text-align: left; margin-bottom: 1.5rem;">
                            <p class="has-text-weight-semibold mb-3">{t("auth.login_no_device")}</p>
                            <button
                                attr:data-testid="auth-btn-try-passkey"
                                class="button is-light is-fullwidth"
                                disabled=move || loading.get()
                                on:click=on_try_passkey
                            >
                                {move || if loading.get() { t("auth.authenticating") } else { t("auth.try_passkey") }}
                            </button>
                        </div>

                        <button
                            attr:data-testid="auth-btn-back"
                            class="button is-ghost has-text-grey is-fullwidth"
                            style="font-size: 0.85rem; text-decoration: underline;"
                            on:click=move |_| { step.set(AuthStep::Main); error.set(None); }
                        >
                            {t("auth.back")}
                        </button>
                    </div>
                </div>
            }.into_view(),

            AuthStep::Main => view! {
                <div style="min-height: 100vh; display: flex; flex-direction: column; align-items: center; justify-content: center; padding: 2rem; text-align: center; background: white;">
                    <div style="max-width: 24rem; width: 100%;">
                        <img src="/icon-192.png" alt="Food Tracker" style="width: 80px; height: 80px; border-radius: 16px; margin-bottom: 1rem;" />
                        <h1 class="title is-3" style="margin-bottom: 0.5rem;">"Food Tracker"</h1>
                        <p class="has-text-grey mb-5" style="font-size: 1.05rem; line-height: 1.6;">
                            {t("auth.main_description")}
                        </p>

                        {error_view}

                        <div style="display: flex; flex-direction: column; gap: 1rem;">
                            <input
                                attr:data-testid="auth-input-name"
                                class="input is-medium"
                                type="text"
                                placeholder=t("auth.name_placeholder")
                                prop:value=move || display_name.get()
                                on:input=move |ev| {
                                    display_name.set(event_target_value(&ev));
                                }
                            />
                            <button
                                attr:data-testid="auth-btn-register"
                                class="button is-link is-medium is-fullwidth"
                                disabled=move || loading.get() || display_name.get().trim().is_empty()
                                on:click=on_register
                            >
                                {move || if loading.get() { t("auth.creating") } else { t("auth.create_account") }}
                            </button>
                            <p class="has-text-grey mt-2 mb-1" style="font-size: 0.95rem;">
                                {t("auth.already_used")}
                            </p>
                            <button
                                attr:data-testid="auth-btn-login"
                                class="button is-light is-medium is-fullwidth"
                                disabled=move || loading.get()
                                on:click=move |_| { step.set(AuthStep::Login); error.set(None); }
                            >
                                {t("auth.login_title")}
                            </button>
                        </div>
                    </div>
                </div>
            }.into_view(),
        }}
    }
}

async fn sleep_ms(ms: u32) {
    let promise = js_sys::Promise::new(&mut |resolve, _| {
        let window = web_sys::window().expect("no window");
        let _ = window.set_timeout_with_callback_and_timeout_and_arguments_0(&resolve, ms as i32);
    });
    let _ = wasm_bindgen_futures::JsFuture::from(promise).await;
}

fn parse_pair_url(url: &str) -> Option<(String, String)> {
    let rest = url.strip_prefix("hjkl-pair://")?;
    let parts: Vec<&str> = rest.splitn(2, '/').collect();
    if parts.len() == 2 && !parts[0].is_empty() && !parts[1].is_empty() {
        Some((parts[0].to_string(), parts[1].to_string()))
    } else {
        None
    }
}
