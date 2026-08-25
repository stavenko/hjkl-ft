//! Экраны кураторского приложения.
//!
//! Роутера нет — как в админке, вид держится в одном перечислении. Экранов мало,
//! и адресная строка здесь никому не нужна: приложение открывается с иконки.

use leptos::*;

use crate::i18n::{t, Lang};
use crate::{api, auth, config, i18n};

/// Что показываем сейчас.
#[derive(Clone, PartialEq)]
pub enum View {
    /// Приложение открыто в браузере — сперва установка.
    Install,
    Login,
    Clients,
    Client { id: String, name: String },
    Chat { id: String, name: String },
    Settings,
}

/// Установлено ли приложение (или мы всё ещё во вкладке браузера).
fn is_pwa() -> bool {
    let Some(win) = web_sys::window() else { return false };
    let mm = |q: &str| win.match_media(q).ok().flatten().map(|m| m.matches()).unwrap_or(false);
    if mm("(display-mode: standalone)") || mm("(display-mode: window-controls-overlay)") {
        return true;
    }
    // iOS до сих пор отвечает только этим нестандартным полем.
    js_sys::Reflect::get(&win.navigator(), &wasm_bindgen::JsValue::from_str("standalone"))
        .ok()
        .and_then(|v| v.as_bool())
        .unwrap_or(false)
}

fn initial_view() -> View {
    if !is_pwa() {
        View::Install
    } else if auth::has_live_session() {
        View::Clients
    } else {
        View::Login
    }
}

#[component]
pub fn App() -> impl IntoView {
    let view = create_rw_signal(initial_view());

    view! {
        <div class="app">
            {move || match view.get() {
                View::Install => view! { <Install/> }.into_view(),
                View::Login => view! { <Login on_done=Callback::new(move |_| view.set(View::Clients))/> }.into_view(),
                View::Clients => view! { <Clients view=view/> }.into_view(),
                View::Client { id, name } => view! { <ClientScreen id=id name=name view=view/> }.into_view(),
                View::Chat { id, name } => view! { <ChatScreen id=id name=name view=view/> }.into_view(),
                View::Settings => view! { <Settings view=view/> }.into_view(),
            }}
        </div>
    }
}

/// Экран установки: те же инструкции, что у приложения худеющего, из общего
/// крейта. Кураторское приложение ставится теми же способами, и держать для него
/// вторую копию этих экранов нельзя — они выстраданы на живых устройствах.
#[component]
fn Install() -> impl IntoView {
    let platform = pwa_prompt::detect_platform();
    view! {
        <div class="screen">
            <div class="center">
                <div class="brandmark"></div>
                <p class="h1">{move || t("install.title")}</p>
                <p class="sub">{move || t("install.body")}</p>
                {pwa_prompt::render_steps(platform, t)}
            </div>
        </div>
    }
}

/// Вход. Паскей на этом домене плюс заведение профиля куратора: личность выдаёт
/// auth-worker, а роль хранит support-worker, и второй шаг нужен всегда — аппрува
/// у нас нет, и профиль это единственное, что отличает куратора от любого другого
/// владельца токена.
#[component]
fn Login(on_done: Callback<()>) -> impl IntoView {
    let busy = create_rw_signal(false);
    let error = create_rw_signal(None::<String>);
    let name = create_rw_signal(String::new());

    let finish = move || {
        spawn_local(async move {
            match api::register().await {
                Ok(_) => on_done.call(()),
                Err(e) => {
                    leptos::logging::error!("curator register: {e}");
                    error.set(Some(e.message().to_string()));
                    busy.set(false);
                }
            }
        });
    };

    let sign_in = move |_| {
        if busy.get_untracked() {
            return;
        }
        busy.set(true);
        error.set(None);
        spawn_local(async move {
            match auth::authenticate().await {
                Ok(_) => finish(),
                Err(e) => {
                    error.set(Some(e));
                    busy.set(false);
                }
            }
        });
    };

    let register = move |_| {
        if busy.get_untracked() {
            return;
        }
        let display = name.get_untracked().trim().to_string();
        if display.is_empty() {
            error.set(Some(t("login.name_required").to_string()));
            return;
        }
        busy.set(true);
        error.set(None);
        spawn_local(async move {
            match auth::register(&display).await {
                Ok(_) => {
                    // Имя, которым человек назвался при создании ключа, — то же
                    // имя, которое увидят его клиенты. Спрашивать дважды незачем.
                    if let Err(e) = api::register().await {
                        leptos::logging::error!("curator register: {e}");
                        error.set(Some(e.message().to_string()));
                        busy.set(false);
                        return;
                    }
                    if let Err(e) = api::set_profile(&display, lang_code()).await {
                        leptos::logging::error!("curator profile: {e}");
                    }
                    on_done.call(());
                }
                Err(e) => {
                    error.set(Some(e));
                    busy.set(false);
                }
            }
        });
    };

    view! {
        <div class="screen">
            <div class="center">
                <div class="brandmark"></div>
                <p class="h1">{move || t("login.title")}</p>
                <p class="sub">{move || t("login.sub")}</p>
                {move || error.get().map(|e| view! { <div class="banner">{e}</div> })}
                <button class="btn" prop:disabled=move || busy.get() on:click=sign_in
                    attr:data-testid="curator-login">
                    {move || t("login.enter")}
                </button>
                <details style="margin-top: 18px;">
                    <summary class="sub" style="cursor: pointer;">{move || t("login.first_time")}</summary>
                    <div class="field" style="margin-top: 12px;">
                        <input class="input" placeholder=move || t("login.name")
                            attr:data-testid="curator-name"
                            prop:value=move || name.get()
                            on:input=move |ev| name.set(event_target_value(&ev)) />
                    </div>
                    <button class="btn" prop:disabled=move || busy.get() on:click=register
                        attr:data-testid="curator-register">
                        {move || t("login.register")}
                    </button>
                </details>
            </div>
        </div>
    }
}

pub fn lang_code() -> &'static str {
    match i18n::get() {
        Lang::En => "en",
        Lang::Ru => "ru",
    }
}

/// Пригласительная ссылка для человека: она ведёт в приложение ХУДЕЮЩЕГО, а не
/// сюда — согласие даётся там, где лежат его данные.
pub fn invite_url(code: &str) -> String {
    format!("{}/curator?c={code}", config::get().app_origin.trim_end_matches('/'))
}

// Экраны списка, клиента, переписки и настроек — в следующих шагах.
#[component]
fn Clients(view: RwSignal<View>) -> impl IntoView {
    let _ = view;
    view! { <div class="screen"><div class="center"><p class="sub">{move || t("common.loading")}</p></div></div> }
}

#[component]
fn ClientScreen(id: String, name: String, view: RwSignal<View>) -> impl IntoView {
    let _ = (id, name, view);
    view! { <div class="screen"></div> }
}

#[component]
fn ChatScreen(id: String, name: String, view: RwSignal<View>) -> impl IntoView {
    let _ = (id, name, view);
    view! { <div class="screen"></div> }
}

#[component]
fn Settings(view: RwSignal<View>) -> impl IntoView {
    let _ = view;
    view! { <div class="screen"></div> }
}
