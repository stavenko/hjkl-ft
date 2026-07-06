use leptos::*;

// Telegram-funnel bridge page. The pay bot links here (/onboard-tg#claim=id.secret)
// instead of straight to /onboard, because passkey CREATION does not work inside
// Telegram's in-app browser (confirmed: fails only in Telegram, fine in a real
// browser). iOS gives no way to programmatically jump to the system browser, so
// this page tells the user to open the real /onboard link in Safari/Chrome and
// offers a one-tap copy. RU-only by design (the bot audience is RU).

/// Parse `#claim=claimId.secret` from the URL fragment (same contract as onboard.rs).
fn parse_claim_fragment() -> Option<(String, String)> {
    let hash = web_sys::window()?.location().hash().ok()?;
    let rest = hash.strip_prefix("#claim=")?;
    let (claim_id, secret) = rest.split_once('.')?;
    if claim_id.is_empty() || secret.is_empty() {
        return None;
    }
    Some((claim_id.to_string(), secret.to_string()))
}

fn origin() -> String {
    web_sys::window()
        .and_then(|w| w.location().origin().ok())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| crate::services::config::get().app_origin.clone())
}

#[component]
pub fn OnboardTgPage() -> impl IntoView {
    let claim = parse_claim_fragment();
    // The REAL onboarding link the user must open in a system browser.
    let onboard_url = claim
        .as_ref()
        .map(|(id, secret)| format!("{}/onboard#claim={}.{}", origin(), id, secret));

    let copied = create_rw_signal(false);
    let url_for_copy = onboard_url.clone();
    let on_copy = move |_| {
        if let (Some(url), Some(win)) = (url_for_copy.clone(), web_sys::window()) {
            let _ = win.navigator().clipboard().write_text(&url);
            copied.set(true);
        }
    };

    let continue_href = onboard_url.clone().unwrap_or_else(|| "/onboard".into());
    let has_claim = onboard_url.is_some();

    view! {
        <div style="min-height: 80vh; display: flex; align-items: center; justify-content: center; padding: 8px;">
            <div style="max-width: 420px; width: 100%; text-align: center;">
                <div style="font-size: 44px; line-height: 1; margin-bottom: 8px;">"✅"</div>
                <h1 class="title is-4" style="margin-bottom: 0.5rem;">"Оплата прошла"</h1>
                <p class="has-text-grey mb-4" style="line-height: 1.6;">
                    "Последний шаг — создать аккаунт. Откройте эту страницу в Safari или Chrome: "
                    "внутри Telegram создание ключа доступа (passkey) не работает."
                </p>

                {has_claim.then(|| view! {
                    <button
                        class="button is-link is-medium is-fullwidth has-text-weight-semibold mb-3"
                        on:click=on_copy
                    >
                        {move || if copied.get() { "Ссылка скопирована ✓" } else { "Скопировать ссылку" }}
                    </button>
                    <p class="is-size-7 has-text-grey mb-4" style="line-height: 1.5;">
                        "Вставьте ссылку в адресную строку Safari или Chrome. "
                        "Либо нажмите «…» вверху справа → «Открыть в Safari»."
                    </p>
                    <a href=continue_href class="is-size-7 has-text-link">
                        "Я уже в Safari или Chrome — продолжить"
                    </a>
                })}

                {(!has_claim).then(|| view! {
                    <p class="has-text-danger is-size-7 mt-3">
                        "Ссылка повреждена. Вернитесь в бот и откройте ссылку привязки заново."
                    </p>
                })}
            </div>
        </div>
    }
}
