use leptos::*;
use leptos_router::*;

use crate::pages;
use crate::services::i18n::t;
use crate::services::{auth, platform};

#[derive(Clone, Copy, PartialEq)]
enum AppState {
    PwaPrompt,
    Auth,
    Ready,
}

fn initial_state() -> AppState {
    if platform::needs_pwa_prompt() {
        AppState::PwaPrompt
    } else if auth::is_logged_in() {
        AppState::Ready
    } else {
        AppState::Auth
    }
}

#[component]
pub fn App() -> impl IntoView {
    let state = create_rw_signal(initial_state());

    view! {
        // Overlays
        {move || match state.get() {
            AppState::PwaPrompt => Some(view! {
                <pages::pwa_prompt::PwaPrompt on_dismiss=Callback::new(move |_| {
                    if auth::is_logged_in() {
                        state.set(AppState::Ready);
                    } else {
                        state.set(AppState::Auth);
                    }
                }) />
            }.into_view()),

            AppState::Auth => Some(view! {
                <div style="position: fixed; inset: 0; z-index: 100; background: white;">
                    <pages::auth_page::AuthPage on_authenticated=Callback::new(move |_| {
                        state.set(AppState::Ready);
                    }) />
                </div>
            }.into_view()),

            AppState::Ready => None,
        }}

        // Router always mounted
        <Router>
            // Session expiry banner — renew via PassKey, panic on failure
            {
                let renewing = create_rw_signal(false);
                let token_version = create_rw_signal(0u32);

                // Re-schedule expire timer whenever token changes
                create_effect(move |_| {
                    let _ = token_version.get();
                    spawn_local(async move {
                        if let Some(expires_str) = web_sys::window()
                            .and_then(|w| w.local_storage().ok().flatten())
                            .and_then(|s| s.get_item("token_expires_at").ok().flatten())
                        {
                            if let Ok(expires) = expires_str.parse::<f64>() {
                                let now = js_sys::Date::now() / 1000.0;
                                let ms = ((expires - now) * 1000.0).max(0.0) as i32;
                                if ms > 0 {
                                    let promise = js_sys::Promise::new(&mut |resolve, _| {
                                        let window = web_sys::window().expect("no window");
                                        let _ = window.set_timeout_with_callback_and_timeout_and_arguments_0(&resolve, ms);
                                    });
                                    let _ = wasm_bindgen_futures::JsFuture::from(promise).await;
                                    token_version.update(|v| *v += 1);
                                }
                            }
                        }
                    });
                });

                let renew = move |_| {
                    renewing.set(true);
                    spawn_local(async move {
                        auth::authenticate().await
                            .expect("PassKey re-authentication failed on existing user — this is a bug");
                        renewing.set(false);
                        token_version.update(|v| *v += 1);
                    });
                };
                move || {
                    let _ = token_version.get(); // subscribe to token changes
                    if auth::is_token_expired() {
                        Some(view! {
                            <div style="position: fixed; top: 0; left: 0; right: 0; z-index: 50; background: #ffe08a; color: #363636; padding: 0.5rem 1rem; display: flex; align-items: center; justify-content: space-between; font-size: 0.85rem;">
                                <span>{t("auth.session_expired")}</span>
                                <button class="button is-small" disabled=move || renewing.get() on:click=renew>
                                    {move || if renewing.get() { "..." } else { t("auth.login_again") }}
                                </button>
                            </div>
                        })
                    } else if auth::is_token_expiring_soon() {
                        Some(view! {
                            <div style="position: fixed; top: 0; left: 0; right: 0; z-index: 50; background: #f5f5f5; color: #363636; padding: 0.5rem 1rem; display: flex; align-items: center; justify-content: space-between; font-size: 0.85rem;">
                                <span>{t("auth.session_expiring")}</span>
                                <button class="button is-small" disabled=move || renewing.get() on:click=renew>
                                    {move || if renewing.get() { "..." } else { t("auth.login_again") }}
                                </button>
                            </div>
                        })
                    } else {
                        None
                    }
                }
            }
            <div style="padding-bottom: 4.5rem;">
                <div style="padding: 0.75rem;">
                    <Routes>
                        <Route path="/" view=pages::diary::DiaryPage />
                        <Route path="/foods" view=pages::foods::FoodsPage />
                        <Route path="/recipes" view=pages::recipes::RecipesPage />
                        <Route path="/recipes/:id" view=pages::recipe_detail::RecipeDetailPage />
                        <Route path="/settings" view=pages::settings::SettingsPage />
                    </Routes>
                </div>
            </div>

            <nav style="position: fixed; bottom: 0.75rem; left: 50%; transform: translateX(-50%); z-index: 40; background: white; display: flex; justify-content: space-around; align-items: center; height: 3.5rem; width: min(22rem, calc(100% - 2rem)); border-radius: 1rem; box-shadow: 0 4px 24px rgba(0,0,0,0.15);">
                <a href="/" style="display: flex; flex-direction: column; align-items: center; justify-content: center; flex: 1; height: 100%; color: #4a4a4a; text-decoration: none;">
                    <svg xmlns="http://www.w3.org/2000/svg" width="28" height="28" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">
                        <path d="M4 19.5v-15A2.5 2.5 0 0 1 6.5 2H20v20H6.5a2.5 2.5 0 0 1 0-5H20" />
                        <line x1="8" y1="7" x2="15" y2="7" />
                        <line x1="8" y1="11" x2="13" y2="11" />
                    </svg>
                    <span style="font-size: 0.6rem; margin-top: 2px;">{t("nav.diary")}</span>
                </a>
                <a href="/recipes" style="display: flex; flex-direction: column; align-items: center; justify-content: center; flex: 1; height: 100%; color: #4a4a4a; text-decoration: none;">
                    <svg xmlns="http://www.w3.org/2000/svg" width="28" height="28" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">
                        <path d="M14 2H6a2 2 0 0 0-2 2v16a2 2 0 0 0 2 2h12a2 2 0 0 0 2-2V8z" />
                        <polyline points="14 2 14 8 20 8" />
                        <line x1="16" y1="13" x2="8" y2="13" />
                        <line x1="16" y1="17" x2="8" y2="17" />
                        <polyline points="10 9 9 9 8 9" />
                    </svg>
                    <span style="font-size: 0.6rem; margin-top: 2px;">{t("nav.recipes")}</span>
                </a>
                <a href="/settings" style="display: flex; flex-direction: column; align-items: center; justify-content: center; flex: 1; height: 100%; color: #4a4a4a; text-decoration: none;">
                    <svg xmlns="http://www.w3.org/2000/svg" width="28" height="28" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">
                        <circle cx="12" cy="12" r="3" />
                        <path d="M19.4 15a1.65 1.65 0 0 0 .33 1.82l.06.06a2 2 0 0 1 0 2.83 2 2 0 0 1-2.83 0l-.06-.06a1.65 1.65 0 0 0-1.82-.33 1.65 1.65 0 0 0-1 1.51V21a2 2 0 0 1-2 2 2 2 0 0 1-2-2v-.09A1.65 1.65 0 0 0 9 19.4a1.65 1.65 0 0 0-1.82.33l-.06.06a2 2 0 0 1-2.83 0 2 2 0 0 1 0-2.83l.06-.06A1.65 1.65 0 0 0 4.68 15a1.65 1.65 0 0 0-1.51-1H3a2 2 0 0 1-2-2 2 2 0 0 1 2-2h.09A1.65 1.65 0 0 0 4.6 9a1.65 1.65 0 0 0-.33-1.82l-.06-.06a2 2 0 0 1 0-2.83 2 2 0 0 1 2.83 0l.06.06A1.65 1.65 0 0 0 9 4.68a1.65 1.65 0 0 0 1-1.51V3a2 2 0 0 1 2-2 2 2 0 0 1 2 2v.09a1.65 1.65 0 0 0 1 1.51 1.65 1.65 0 0 0 1.82-.33l.06-.06a2 2 0 0 1 2.83 0 2 2 0 0 1 0 2.83l-.06.06A1.65 1.65 0 0 0 19.4 9a1.65 1.65 0 0 0 1.51 1H21a2 2 0 0 1 2 2 2 2 0 0 1-2 2h-.09a1.65 1.65 0 0 0-1.51 1z" />
                    </svg>
                    <span style="font-size: 0.6rem; margin-top: 2px;">{t("nav.settings")}</span>
                </a>
            </nav>
        </Router>
    }
}
