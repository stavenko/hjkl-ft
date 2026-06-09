use leptos::*;
use crate::services::auth;
use crate::services::i18n::t;
use crate::components::qr_code::QrCode;
use crate::components::qr_scanner::QrScanner;

async fn copy_to_clipboard(text: &str) -> Result<(), wasm_bindgen::JsValue> {
    let window = web_sys::window().expect("no window");
    let clipboard = window.navigator().clipboard();
    let promise = clipboard.write_text(text);
    wasm_bindgen_futures::JsFuture::from(promise).await?;
    Ok(())
}

// ---------------------------------------------------------------------------
// Shared types
// ---------------------------------------------------------------------------

#[derive(Clone, PartialEq)]
enum PairStep {
    /// Initial menu (show QR / scan QR)
    Menu,
    /// Displaying a QR code (with optional polling)
    ShowQr { qr_url: String, pairing_id: String },
    /// Camera is open, scanning
    Scanning,
    /// Pairing completed
    Done,
}

// ---------------------------------------------------------------------------
// New-device mode (not logged in) -- shown from auth_page
// ---------------------------------------------------------------------------

/// Pairing UI for a device that is NOT yet logged in.
/// `on_done` is called once pairing succeeds (the device is now authenticated).
#[component]
pub fn PairPageNew(on_done: Callback<()>, on_back: Callback<()>) -> impl IntoView {
    let step = create_rw_signal(PairStep::Menu);
    let error = create_rw_signal(None::<String>);
    let loading = create_rw_signal(false);

    // --- "Show QR code" pressed: POST /pair/request ---
    let show_qr = move |_| {
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
                    step.set(PairStep::ShowQr { qr_url, pairing_id: pairing_id.clone() });
                    loading.set(false);
                    // Start polling for approval
                    poll_until_done(pairing_id, step, error).await;
                }
                Err(e) => {
                    error.set(Some(e));
                    loading.set(false);
                }
            }
        });
    };

    // --- "Scan QR" pressed: open camera ---
    let open_scanner = move |_| {
        error.set(None);
        step.set(PairStep::Scanning);
    };

    // --- QR scanned: parse hjkl-pair://username/pairing_id/secret ---
    let on_scan = Callback::new(move |data: String| {
        error.set(None);
        loading.set(true);
        match parse_pair_url(&data) {
            Some((username, pairing_id, secret)) => {
                spawn_local(async move {
                    match auth::pair_claim(&username, &pairing_id, &secret).await {
                        Ok(_) => {
                            step.set(PairStep::Done);
                            loading.set(false);
                            on_done.call(());
                        }
                        Err(e) => {
                            error.set(Some(e));
                            loading.set(false);
                            step.set(PairStep::Menu);
                        }
                    }
                });
            }
            None => {
                error.set(Some(t("pair.error_invalid_qr").to_string()));
                loading.set(false);
                step.set(PairStep::Menu);
            }
        }
    });

    let on_scanner_close = Callback::new(move |_| {
        step.set(PairStep::Menu);
    });

    view! {
        <div style="min-height: 100vh; display: flex; flex-direction: column; align-items: center; justify-content: center; padding: 2rem; text-align: center; background: white;">
            <div style="max-width: 24rem; width: 100%;">
                <h1 class="title is-4 mb-4">{t("pair.title")}</h1>

                {move || error.get().map(|e| view! {
                    <div class="notification is-danger is-light mb-4" style="text-align: left;">
                        <button class="delete" on:click=move |_| error.set(None)></button>
                        {e}
                    </div>
                })}

                {move || match step.get() {
                    PairStep::Menu => view! {
                        <div style="display: flex; flex-direction: column; gap: 1rem;">
                            <button
                                class="button is-link is-medium is-fullwidth"
                                disabled=move || loading.get()
                                on:click=open_scanner
                            >
                                {t("pair.scan_qr")}
                            </button>
                            <button
                                class="button is-light is-medium is-fullwidth"
                                disabled=move || loading.get()
                                on:click=show_qr
                            >
                                {t("pair.show_qr")}
                            </button>
                            <button
                                class="button is-text is-medium is-fullwidth"
                                on:click=move |_| on_back.call(())
                            >
                                {t("pair.back")}
                            </button>
                        </div>
                    }.into_view(),

                    PairStep::ShowQr { ref qr_url, .. } => {
                        let url = qr_url.clone();
                        let url_for_copy = qr_url.clone();
                        let copied = create_rw_signal(false);
                        view! {
                            <p class="mb-4 has-text-grey">{t("pair.show_hint_new")}</p>
                            <div style="display: flex; justify-content: center; margin-bottom: 1rem;">
                                <QrCode data=url size=240 />
                            </div>
                            <button
                                class="button is-small is-light is-fullwidth mb-3"
                                on:click=move |_| {
                                    let u = url_for_copy.clone();
                                    spawn_local(async move {
                                        let _ = copy_to_clipboard(&u).await;
                                        copied.set(true);
                                    });
                                }
                            >
                                {move || if copied.get() { t("qr.copied") } else { t("qr.copy_link") }}
                            </button>
                            <p class="has-text-grey is-size-7 mb-4">{t("pair.waiting")}</p>
                            <button
                                class="button is-text"
                                on:click=move |_| step.set(PairStep::Menu)
                            >
                                {t("pair.back")}
                            </button>
                        }.into_view()
                    }

                    PairStep::Scanning => view! {
                        <QrScanner on_scan=on_scan on_close=on_scanner_close />
                    }.into_view(),

                    PairStep::Done => view! {
                        <p class="has-text-success is-size-5 mb-4">{t("pair.success")}</p>
                    }.into_view(),
                }}
            </div>
        </div>
    }
}

// ---------------------------------------------------------------------------
// Logged-in mode (from settings)
// ---------------------------------------------------------------------------

/// Pairing UI for a device that IS logged in.
/// `on_close` is called when the user wants to go back.
#[component]
pub fn PairPageLoggedIn(on_close: Callback<()>) -> impl IntoView {
    let step = create_rw_signal(PairStep::Menu);
    let error = create_rw_signal(None::<String>);
    let loading = create_rw_signal(false);

    // --- "Show QR code" pressed: POST /pair/create (authenticated) ---
    let show_qr = move |_| {
        loading.set(true);
        error.set(None);
        spawn_local(async move {
            match auth::pair_create().await {
                Ok(resp) => {
                    let qr_url = resp.get("qr_url")
                        .and_then(|v| v.as_str())
                        .unwrap_or_default()
                        .to_string();
                    let pairing_id = resp.get("pairing_id")
                        .and_then(|v| v.as_str())
                        .unwrap_or_default()
                        .to_string();
                    step.set(PairStep::ShowQr { qr_url, pairing_id: pairing_id.clone() });
                    loading.set(false);
                    // Poll for completion
                    poll_until_done(pairing_id, step, error).await;
                }
                Err(e) => {
                    error.set(Some(e));
                    loading.set(false);
                }
            }
        });
    };

    // --- "Scan QR" pressed: open camera to scan a request from the new device ---
    let open_scanner = move |_| {
        error.set(None);
        step.set(PairStep::Scanning);
    };

    // --- QR scanned: parse and approve ---
    let on_scan = Callback::new(move |data: String| {
        error.set(None);
        loading.set(true);
        match parse_pair_url(&data) {
            Some((_username, pairing_id, secret)) => {
                spawn_local(async move {
                    match auth::pair_approve(&pairing_id, &secret).await {
                        Ok(_) => {
                            step.set(PairStep::Done);
                            loading.set(false);
                        }
                        Err(e) => {
                            error.set(Some(e));
                            loading.set(false);
                            step.set(PairStep::Menu);
                        }
                    }
                });
            }
            None => {
                error.set(Some(t("pair.error_invalid_qr").to_string()));
                loading.set(false);
                step.set(PairStep::Menu);
            }
        }
    });

    let on_scanner_close = Callback::new(move |_| {
        step.set(PairStep::Menu);
    });

    view! {
        <div style="max-width: 24rem; width: 100%; margin: 0 auto; text-align: center;">
            <h2 class="title is-4 mb-4">{t("pair.title")}</h2>

            {move || error.get().map(|e| view! {
                <div class="notification is-danger is-light mb-4" style="text-align: left;">
                    <button class="delete" on:click=move |_| error.set(None)></button>
                    {e}
                </div>
            })}

            {move || match step.get() {
                PairStep::Menu => view! {
                    <div style="display: flex; flex-direction: column; gap: 1rem;">
                        <button
                            class="button is-link is-medium is-fullwidth"
                            disabled=move || loading.get()
                            on:click=show_qr
                        >
                            {t("pair.show_qr")}
                        </button>
                        <button
                            class="button is-light is-medium is-fullwidth"
                            disabled=move || loading.get()
                            on:click=open_scanner
                        >
                            {t("pair.scan_qr")}
                        </button>
                        <button
                            class="button is-text is-medium is-fullwidth"
                            on:click=move |_| on_close.call(())
                        >
                            {t("pair.back")}
                        </button>
                    </div>
                }.into_view(),

                PairStep::ShowQr { ref qr_url, .. } => {
                    let url = qr_url.clone();
                    let url_for_copy = qr_url.clone();
                    let copied = create_rw_signal(false);
                    view! {
                        <p class="mb-4 has-text-grey">{t("pair.show_hint_logged")}</p>
                        <div style="display: flex; justify-content: center; margin-bottom: 1rem;">
                            <QrCode data=url size=240 />
                        </div>
                        <button
                            class="button is-small is-light is-fullwidth mb-3"
                            on:click=move |_| {
                                let u = url_for_copy.clone();
                                spawn_local(async move {
                                    let _ = copy_to_clipboard(&u).await;
                                    copied.set(true);
                                });
                            }
                        >
                            {move || if copied.get() { t("qr.copied") } else { t("qr.copy_link") }}
                        </button>
                        <p class="has-text-grey is-size-7 mb-4">{t("pair.waiting")}</p>
                        <button
                            class="button is-text"
                            on:click=move |_| step.set(PairStep::Menu)
                        >
                            {t("pair.back")}
                        </button>
                    }.into_view()
                }

                PairStep::Scanning => view! {
                    <QrScanner on_scan=on_scan on_close=on_scanner_close />
                }.into_view(),

                PairStep::Done => view! {
                    <div>
                        <p class="has-text-success is-size-5 mb-4">{t("pair.success")}</p>
                        <button
                            class="button is-link"
                            on:click=move |_| on_close.call(())
                        >
                            {t("pair.back")}
                        </button>
                    </div>
                }.into_view(),
            }}
        </div>
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Parse "hjkl-pair://username/pairing_id/secret" into (username, pairing_id, secret).
fn parse_pair_url(url: &str) -> Option<(String, String, String)> {
    let rest = url.strip_prefix("hjkl-pair://")?;
    let parts: Vec<&str> = rest.splitn(3, '/').collect();
    if parts.len() == 3 && !parts[0].is_empty() && !parts[1].is_empty() && !parts[2].is_empty() {
        Some((
            parts[0].to_string(),
            parts[1].to_string(),
            parts[2].to_string(),
        ))
    } else {
        None
    }
}

/// Poll /pair/status/:id every 2 seconds until status is "completed" or "expired".
async fn poll_until_done(
    pairing_id: String,
    step: RwSignal<PairStep>,
    error: RwSignal<Option<String>>,
) {
    loop {
        // If user navigated away from the QR view, stop polling
        if !matches!(step.get_untracked(), PairStep::ShowQr { .. }) {
            return;
        }

        gloo_timers_sleep(2000).await;

        match auth::pair_status(&pairing_id).await {
            Ok(resp) => {
                let status = resp.get("status")
                    .and_then(|v| v.as_str())
                    .unwrap_or("pending");
                match status {
                    "completed" => {
                        step.set(PairStep::Done);
                        return;
                    }
                    "expired" => {
                        error.set(Some(t("pair.expired").to_string()));
                        step.set(PairStep::Menu);
                        return;
                    }
                    _ => { /* keep polling */ }
                }
            }
            Err(_) => {
                // Transient network error -- keep trying
            }
        }
    }
}

async fn gloo_timers_sleep(ms: u32) {
    let promise = js_sys::Promise::new(&mut |resolve, _reject| {
        let window = web_sys::window().expect("no window");
        let _ = window.set_timeout_with_callback_and_timeout_and_arguments_0(&resolve, ms as i32);
    });
    let _ = wasm_bindgen_futures::JsFuture::from(promise).await;
}
