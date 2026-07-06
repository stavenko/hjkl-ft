use leptos::*;

use crate::services::auth;

async fn sleep_ms(ms: u32) {
    let promise = js_sys::Promise::new(&mut |resolve, _| {
        let window = web_sys::window().expect("no window");
        let _ = window.set_timeout_with_callback_and_timeout_and_arguments_0(&resolve, ms as i32);
    });
    let _ = wasm_bindgen_futures::JsFuture::from(promise).await;
}

/// Unified code login used in BOTH the browser onboarding and the installed PWA. The user
/// requests a one-time code (delivered to their Telegram — our payment bot), then enters it.
/// `user_id` is the non-secret account id carried in the URL / manifest. On success the
/// session is established locally and `on_authenticated` fires.
#[component]
pub fn CodeAuth(user_id: String, on_authenticated: Callback<()>) -> impl IntoView {
    let user_id = store_value(user_id);
    let code = create_rw_signal(String::new());
    let error = create_rw_signal(None::<String>);
    let busy = create_rw_signal(false);
    let sent_once = create_rw_signal(false);
    let cooldown = create_rw_signal(0i32); // seconds left before another send is allowed

    // Count the cooldown down to 0 (1s ticks). Self-cancels when it reaches 0.
    let arm_cooldown = move || {
        cooldown.set(60);
        spawn_local(async move {
            loop {
                sleep_ms(1000).await;
                let left = cooldown.get_untracked() - 1;
                cooldown.set(left.max(0));
                if left <= 0 {
                    break;
                }
            }
        });
    };

    let on_send = move |_| {
        if cooldown.get_untracked() > 0 || busy.get_untracked() {
            return;
        }
        busy.set(true);
        error.set(None);
        spawn_local(async move {
            match auth::code_request(&user_id.get_value()).await {
                Ok(()) => {
                    sent_once.set(true);
                    arm_cooldown();
                }
                Err(e) => {
                    if e.contains("429") {
                        // Already within cooldown on the server — reflect it.
                        sent_once.set(true);
                        arm_cooldown();
                    } else {
                        error.set(Some("Не удалось отправить код. Попробуйте ещё раз.".into()));
                    }
                }
            }
            busy.set(false);
        });
    };

    // Verify the 6-digit code. Fired automatically once all six digits are in (below), and by
    // the explicit «Войти» button. No-ops until exactly 6 digits are present, or while busy.
    let verify_now = move || {
        let digits: String = code
            .get_untracked()
            .chars()
            .filter(|c| c.is_ascii_digit())
            .collect();
        if digits.len() != 6 || busy.get_untracked() {
            return;
        }
        busy.set(true);
        error.set(None);
        spawn_local(async move {
            match auth::code_verify(&user_id.get_value(), &digits).await {
                Ok(()) => on_authenticated.call(()),
                Err(e) => {
                    let msg = if e.contains("401") {
                        "Неверный или просроченный код."
                    } else if e.contains("429") {
                        "Слишком много попыток. Запросите новый код."
                    } else {
                        "Не удалось войти. Проверьте код."
                    };
                    error.set(Some(msg.to_string()));
                    busy.set(false);
                }
            }
        });
    };

    view! {
        <div style="max-width: 22rem; width: 100%; margin: 0 auto;">
            <p class="has-text-grey mb-4" style="line-height: 1.6;">
                "Чтобы войти, запросите одноразовый код — мы пришлём его в Telegram, в нашего бота оплаты. Скопируйте код из чата и введите здесь."
            </p>

            {move || error.get().map(|e| view! {
                <div class="notification is-danger is-light mb-4" style="text-align: left;">{e}</div>
            })}

            // Send / resend with a live cooldown.
            <button
                attr:data-testid="codeauth-btn-send"
                class="button is-link is-medium is-fullwidth has-text-weight-semibold"
                style="margin-bottom: 1rem;"
                disabled=move || busy.get() || (cooldown.get() > 0)
                on:click=on_send
            >
                {move || {
                    let cd = cooldown.get();
                    if cd > 0 {
                        format!("Отправить ещё раз через {cd} с")
                    } else if sent_once.get() {
                        "Прислать код ещё раз".to_string()
                    } else {
                        "Прислать код в Telegram".to_string()
                    }
                }}
            </button>

            {move || sent_once.get().then(|| view! {
                <div>
                    <input
                        attr:data-testid="codeauth-input"
                        class="input is-large has-text-centered"
                        type="text"
                        inputmode="numeric"
                        autocomplete="one-time-code"
                        maxlength="6"
                        style="margin-bottom: 1rem; letter-spacing: 0.3em; font-weight: 700;"
                        placeholder="000000"
                        prop:value=move || code.get()
                        on:input=move |ev| {
                            // Keep only digits; auto-submit the moment all six are in.
                            let digits: String = event_target_value(&ev)
                                .chars()
                                .filter(|c| c.is_ascii_digit())
                                .take(6)
                                .collect();
                            code.set(digits.clone());
                            if digits.len() == 6 {
                                verify_now();
                            }
                        }
                    />
                    <button
                        attr:data-testid="codeauth-btn-verify"
                        class="button is-primary is-medium is-fullwidth has-text-weight-semibold"
                        disabled=move || busy.get() || code.get().chars().filter(|c| c.is_ascii_digit()).count() != 6
                        on:click=move |_| verify_now()
                    >
                        {move || if busy.get() { "Входим…" } else { "Войти" }}
                    </button>
                </div>
            })}
        </div>
    }
}
