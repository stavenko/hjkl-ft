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
                    let secret = resp.get("secret")
                        .and_then(|v| v.as_str())
                        .unwrap_or_default()
                        .to_string();
                    step.set(PairStep::ShowQr { qr_url, pairing_id: pairing_id.clone() });
                    loading.set(false);
                    poll_until_done_unauthenticated(pairing_id, secret, step, error).await;
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

    // --- QR scanned: parse hjkl-pair://pairing_id/secret ---
    let on_scan = Callback::new(move |data: String| {
        error.set(None);
        loading.set(true);
        match parse_pair_url(&data) {
            Some((pairing_id, secret)) => {
                spawn_local(async move {
                    match auth::pair_claim(&pairing_id, &secret).await {
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
        <div style="min-height: 100vh; display: flex; flex-direction: column; align-items: center; justify-content: center; padding: 2rem; text-align: center; background: var(--bulma-scheme-main);">
            <div style="max-width: 24rem; width: 100%;">
                <h1 class="title is-4 mb-4">{move || t("pair.title")}</h1>

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
                                attr:data-testid="pair-new-btn-scan"
                                class="button is-link is-medium is-fullwidth"
                                disabled=move || loading.get()
                                on:click=open_scanner
                            >
                                {move || t("pair.scan_qr")}
                            </button>
                            <button
                                attr:data-testid="pair-new-btn-show"
                                class="button is-medium is-fullwidth"
                                disabled=move || loading.get()
                                on:click=show_qr
                            >
                                {move || t("pair.show_qr")}
                            </button>
                            <button
                                attr:data-testid="pair-new-btn-back"
                                class="button is-text is-medium is-fullwidth"
                                on:click=move |_| on_back.call(())
                            >
                                {move || t("pair.back")}
                            </button>
                        </div>
                    }.into_view(),

                    PairStep::ShowQr { ref qr_url, .. } => {
                        let url = qr_url.clone();
                        let url_for_copy = qr_url.clone();
                        let copied = create_rw_signal(false);
                        view! {
                            <p class="mb-4 has-text-grey">{move || t("pair.show_hint_new")}</p>
                            <div attr:data-testid="pair-new-qr-display" style="display: flex; justify-content: center; margin-bottom: 1rem;">
                                <QrCode data=url size=240 />
                            </div>
                            <button
                                attr:data-testid="pair-new-btn-copy-link"
                                class="button is-small is-fullwidth mb-3"
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
                            <p class="has-text-grey is-size-7 mb-4">{move || t("pair.waiting")}</p>
                            <button
                                attr:data-testid="pair-new-btn-back"
                                class="button is-text"
                                on:click=move |_| step.set(PairStep::Menu)
                            >
                                {move || t("pair.back")}
                            </button>
                        }.into_view()
                    }

                    PairStep::Scanning => view! {
                        <QrScanner on_scan=on_scan on_close=on_scanner_close />
                    }.into_view(),

                    PairStep::Done => view! {
                        <p class="has-text-success is-size-5 mb-4">{move || t("pair.success")}</p>
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
                    poll_until_done_authenticated(pairing_id, step, error).await;
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
            Some((pairing_id, secret)) => {
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
            <h2 class="title is-4 mb-4">{move || t("pair.title")}</h2>

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
                            attr:data-testid="pair-logged-btn-show"
                            class="button is-link is-medium is-fullwidth"
                            disabled=move || loading.get()
                            on:click=show_qr
                        >
                            {move || t("pair.show_qr")}
                        </button>
                        <button
                            attr:data-testid="pair-logged-btn-scan"
                            class="button is-medium is-fullwidth"
                            disabled=move || loading.get()
                            on:click=open_scanner
                        >
                            {move || t("pair.scan_qr")}
                        </button>
                        <button
                            attr:data-testid="pair-logged-btn-back"
                            class="button is-text is-medium is-fullwidth"
                            on:click=move |_| on_close.call(())
                        >
                            {move || t("pair.back")}
                        </button>
                    </div>
                }.into_view(),

                PairStep::ShowQr { ref qr_url, .. } => {
                    let url = qr_url.clone();
                    let url_for_copy = qr_url.clone();
                    let copied = create_rw_signal(false);
                    view! {
                        <p class="mb-4 has-text-grey">{move || t("pair.show_hint_logged")}</p>
                        <div attr:data-testid="pair-logged-qr-display" style="display: flex; justify-content: center; margin-bottom: 1rem;">
                            <QrCode data=url size=240 />
                        </div>
                        <button
                            attr:data-testid="pair-logged-btn-copy-link"
                            class="button is-small is-fullwidth mb-3"
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
                        <p class="has-text-grey is-size-7 mb-4">{move || t("pair.waiting")}</p>
                        <button
                            attr:data-testid="pair-logged-btn-back"
                            class="button is-text"
                            on:click=move |_| step.set(PairStep::Menu)
                        >
                            {move || t("pair.back")}
                        </button>
                    }.into_view()
                }

                PairStep::Scanning => view! {
                    <QrScanner on_scan=on_scan on_close=on_scanner_close />
                }.into_view(),

                PairStep::Done => view! {
                    <div>
                        <p class="has-text-success is-size-5 mb-4">{move || t("pair.success")}</p>
                        <button
                            attr:data-testid="pair-logged-btn-back"
                            class="button is-link"
                            on:click=move |_| on_close.call(())
                        >
                            {move || t("pair.back")}
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

/// Parse QR pair URL into (pairing_id, secret).
/// Format: "hjkl-pair://pairing_id/secret"
fn parse_pair_url(url: &str) -> Option<(String, String)> {
    let rest = url.strip_prefix("hjkl-pair://")?;
    let parts: Vec<&str> = rest.splitn(2, '/').collect();
    match parts.len() {
        2 if !parts[0].is_empty() && !parts[1].is_empty() => {
            Some((parts[0].to_string(), parts[1].to_string()))
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_two_parts() {
        let result = parse_pair_url("hjkl-pair://abc123/secretXYZ");
        assert_eq!(result, Some(("abc123".into(), "secretXYZ".into())));
    }

    #[test]
    fn test_parse_wrong_prefix() {
        assert_eq!(parse_pair_url("https://example.com"), None);
    }

    #[test]
    fn test_parse_empty() {
        assert_eq!(parse_pair_url("hjkl-pair://"), None);
    }

    #[test]
    fn test_parse_single_part() {
        assert_eq!(parse_pair_url("hjkl-pair://onlyid"), None);
    }

    #[test]
    fn test_parse_real_request_url() {
        let url = "hjkl-pair://i9o2kaa/1-i-2uAFu8-PN3qF3eyrqQUiqj_LYjbf98edQbiOV60";
        let result = parse_pair_url(url);
        assert_eq!(result, Some(("i9o2kaa".into(), "1-i-2uAFu8-PN3qF3eyrqQUiqj_LYjbf98edQbiOV60".into())));
    }

    #[test]
    fn test_parse_pairing_url() {
        let url = "hjkl-pair://abc456/secret789";
        let result = parse_pair_url(url);
        assert_eq!(result, Some(("abc456".into(), "secret789".into())));
    }
}

/// Poll /pair/status/:id every 2 seconds until status is "completed" or "expired".
/// Poll for logged-in device (uses authenticated /pair/status)
async fn poll_until_done_authenticated(
    pairing_id: String,
    step: RwSignal<PairStep>,
    error: RwSignal<Option<String>>,
) {
    loop {
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
                    "completed" | "claimed" => {
                        step.set(PairStep::Done);
                        return;
                    }
                    "expired" => {
                        error.set(Some(t("pair.expired").to_string()));
                        step.set(PairStep::Menu);
                        return;
                    }
                    _ => {}
                }
            }
            Err(_) => {}
        }
    }
}

/// Poll for new device (uses unauthenticated /pair/check)
async fn poll_until_done_unauthenticated(
    pairing_id: String,
    secret: String,
    step: RwSignal<PairStep>,
    error: RwSignal<Option<String>>,
) {
    loop {
        if !matches!(step.get_untracked(), PairStep::ShowQr { .. }) {
            return;
        }
        gloo_timers_sleep(2000).await;
        match auth::pair_check(&pairing_id, &secret).await {
            Ok(resp) => {
                let status = resp.get("status")
                    .and_then(|v| v.as_str())
                    .unwrap_or("pending");
                match status {
                    "completed" | "claimed" | "approved" => {
                        step.set(PairStep::Done);
                        return;
                    }
                    "expired" => {
                        error.set(Some(t("pair.expired").to_string()));
                        step.set(PairStep::Menu);
                        return;
                    }
                    _ => {}
                }
            }
            Err(_) => {}
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
