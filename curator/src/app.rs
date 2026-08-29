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
/// Язык для общих шагов: слова у них свои, крейтовые.
fn pwa_lang() -> pwa_prompt::Lang {
    match i18n::get() {
        i18n::Lang::En => pwa_prompt::Lang::En,
        i18n::Lang::Ru => pwa_prompt::Lang::Ru,
    }
}

#[component]
fn Install() -> impl IntoView {
    let platform = pwa_prompt::detect_platform();
    view! {
        <div class="screen screen--center screen--noflow">
            <div class="center">
                <div class="brandmark"></div>
                <p class="h1">{move || t("install.title")}</p>
                <p class="sub">{move || t("install.body")}</p>
                {pwa_prompt::render_steps(platform, pwa_lang)}
            </div>
        </div>
    }
}

/// Вход. Паскей на этом домене плюс заведение профиля куратора: личность выдаёт
/// auth-worker, а роль хранит support-worker, и второй шаг нужен всегда — аппрува
/// у нас нет, и профиль это единственное, что отличает куратора от любого другого
/// владельца токена.
///
/// Экран ведёт РЕГИСТРАЦИЮ, а не вход, и порядок на нём не случаен. Кураторов
/// приглашают, они приходят сюда впервые, и первое, что им нужно, — назваться и
/// завести ключ. Вход — для второго и следующих раз, и это одна строка внизу.
/// Прежде было наоборот: кнопка «Войти», а регистрация пряталась в `<details>`
/// с браузерным треугольником — то есть главное действие экрана было спрятано
/// за раскрывашкой, да ещё и выглядело чужеродно.
#[component]
fn Login(on_done: Callback<()>) -> impl IntoView {
    let busy = create_rw_signal(false);
    let error = create_rw_signal(None::<String>);
    let name = create_rw_signal(String::new());

    let finish = move || {
        spawn_local(async move {
            match api::register().await {
                Ok(_) => {
                    subscribe_to_push();
                    on_done.call(())
                }
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
                    subscribe_to_push();
                    on_done.call(());
                }
                Err(e) => {
                    error.set(Some(e));
                    busy.set(false);
                }
            }
        });
    };

    // Настоящая <form>: на телефоне это даёт кнопку «Go» на клавиатуре, и имя
    // отправляется, не убирая её. `submit` — тот же путь, что и нажатие кнопки.
    let submit = move |ev: leptos::ev::SubmitEvent| {
        ev.prevent_default();
        register(());
    };

    view! {
        <div class="screen screen--center screen--noflow">
            <div class="center">
                <div class="brandmark"></div>
                <p class="h1">{move || t("login.title")}</p>
                <p class="sub">{move || t("login.sub")}</p>

                {move || error.get().map(|e| view! {
                    <div class="banner" attr:role="alert">{e}</div>
                })}

                <form on:submit=submit>
                    <label class="label" attr:for="curator-name">{move || t("login.name")}</label>
                    <input class="field" attr:id="curator-name" attr:type="text"
                        attr:autocomplete="name" attr:autocapitalize="words"
                        attr:enterkeyhint="go" attr:spellcheck="false"
                        attr:data-testid="curator-name"
                        prop:value=move || name.get()
                        on:input=move |ev| name.set(event_target_value(&ev)) />
                    <p class="hint">{move || t("login.name_hint")}</p>
                    <button class="btn btn--primary btn--block" style="margin-top: 20px;"
                        attr:type="submit" prop:disabled=move || busy.get()
                        attr:data-testid="curator-register">
                        {move || t("login.register")}
                    </button>
                </form>

                <p class="alt">
                    {move || t("login.have_key")}
                    <button class="linkbtn" attr:type="button"
                        prop:disabled=move || busy.get() on:click=sign_in
                        attr:data-testid="curator-login">
                        {move || t("login.enter")}
                    </button>
                </p>
            </div>
        </div>
    }
}

/// Подписаться на уведомления о сообщениях клиентов — лучшим усилием.
///
/// Отказ в разрешении не должен мешать входу: работать без уведомлений можно,
/// не войти — нельзя.
fn subscribe_to_push() {
    if crate::push::is_subscribed() || !crate::push::is_supported() {
        return;
    }
    spawn_local(async {
        if let Err(e) = crate::push::subscribe().await {
            leptos::logging::warn!("подписка на уведомления: {e}");
        }
    });
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

/// Скопировать текст в буфер СИНХРОННО из обработчика нажатия.
///
/// Два механизма разом — как в приложении худеющего и по той же причине: в
/// standalone-PWA на iOS асинхронный Clipboard API капризен, а старый
/// execCommand там работает. Оба вызываются внутри жеста, что iOS и проверяет.
fn copy_to_clipboard(text: &str) {
    use wasm_bindgen::JsCast;
    let Some(window) = web_sys::window() else { return };
    let _ = window.navigator().clipboard().write_text(text);

    let Some(document) = window.document() else { return };
    if let Ok(el) = document.create_element("textarea") {
        let ta: web_sys::HtmlTextAreaElement = el.unchecked_into();
        ta.set_value(text);
        let _ = ta.set_attribute("readonly", "");
        let _ = ta.style().set_property("position", "fixed");
        let _ = ta.style().set_property("top", "0");
        let _ = ta.style().set_property("opacity", "0");
        if let Some(body) = document.body() {
            let _ = body.append_child(&ta);
            ta.select();
            let _ = ta.set_selection_range(0, text.len() as u32);
            if let Ok(html_doc) = document.dyn_into::<web_sys::HtmlDocument>() {
                let _ = html_doc.exec_command("copy");
            }
            let _ = body.remove_child(&ta);
        }
    }
}

/// Список клиентов. В строке только имя: всё остальное — внутри.
#[component]
fn Clients(view: RwSignal<View>) -> impl IntoView {
    let clients = create_rw_signal(Vec::<api::Client>::new());
    let loading = create_rw_signal(true);
    let error = create_rw_signal(None::<String>);
    let adding = create_rw_signal(false);
    let new_name = create_rw_signal(String::new());
    let busy = create_rw_signal(false);

    let reload = move || {
        spawn_local(async move {
            match api::clients().await {
                Ok(list) => {
                    clients.set(sorted(list));
                    error.set(None);
                }
                Err(e) => {
                    if e.is_auth() {
                        auth::logout();
                        view.set(View::Login);
                        return;
                    }
                    error.set(Some(e.message().to_string()));
                }
            }
            loading.set(false);
        });
    };
    create_effect(move |_| reload());

    let create = move |_| {
        if busy.get_untracked() {
            return;
        }
        let name = new_name.get_untracked().trim().to_string();
        if name.is_empty() {
            return;
        }
        busy.set(true);
        spawn_local(async move {
            match api::add_client(&name).await {
                Ok(_) => {
                    new_name.set(String::new());
                    adding.set(false);
                    reload();
                }
                Err(e) => error.set(Some(e.message().to_string())),
            }
            busy.set(false);
        });
    };

    view! {
        <div class="appbar">
            <div class="ring"></div>
            <div class="appbar__title">{move || t("clients.title")}</div>
            <button class="btn btn--icon btn--ghost" attr:data-testid="curator-settings"
                on:click=move |_| view.set(View::Settings)>
                <svg viewBox="0 0 24 24"><circle cx="12" cy="12" r="3"/>
                    <path d="M19.4 15a1.65 1.65 0 0 0 .33 1.82l.06.06a2 2 0 1 1-2.83 2.83l-.06-.06a1.65 1.65 0 0 0-1.82-.33 1.65 1.65 0 0 0-1 1.51V21a2 2 0 1 1-4 0v-.09A1.65 1.65 0 0 0 9 19.4a1.65 1.65 0 0 0-1.82.33l-.06.06a2 2 0 1 1-2.83-2.83l.06-.06a1.65 1.65 0 0 0 .33-1.82 1.65 1.65 0 0 0-1.51-1H3a2 2 0 1 1 0-4h.09A1.65 1.65 0 0 0 4.6 9a1.65 1.65 0 0 0-.33-1.82l-.06-.06a2 2 0 1 1 2.83-2.83l.06.06A1.65 1.65 0 0 0 9 4.6c.6-.25 1-.85 1-1.51V3a2 2 0 1 1 4 0v.09c0 .66.4 1.26 1 1.51.6.25 1.3.12 1.82-.33l.06-.06a2 2 0 1 1 2.83 2.83l-.06.06A1.65 1.65 0 0 0 19.4 9c.25.6.85 1 1.51 1H21a2 2 0 1 1 0 4h-.09c-.66 0-1.26.4-1.51 1z"/></svg>
            </button>
        </div>
        <div class="screen pad">
            {move || error.get().map(|e| view! { <div class="banner">{e}</div> })}
            {move || loading.get().then(|| view! { <div class="spinner"></div> })}

            {move || adding.get().then(|| view! {
                <div class="card" style="margin-bottom: 12px;">
                    <input class="field" placeholder=move || t("clients.add_name")
                        attr:data-testid="client-name"
                        prop:value=move || new_name.get()
                        on:input=move |ev| new_name.set(event_target_value(&ev)) />
                    <p class="sub" style="margin: 8px 0 12px; font-size: .8rem;">
                        {move || t("clients.add_hint")}
                    </p>
                    <div style="display: flex; gap: 8px;">
                        <button class="btn btn--primary" prop:disabled=move || busy.get()
                            attr:data-testid="client-create" on:click=create>
                            {move || t("clients.create")}
                        </button>
                        <button class="btn btn--ghost" on:click=move |_| adding.set(false)>
                            {move || t("clients.cancel")}
                        </button>
                    </div>
                </div>
            })}

            {move || {
                let list = clients.get();
                if list.is_empty() && !loading.get() {
                    return view! {
                        <div class="empty">
                            <div class="empty__ring"></div>
                            <p class="sub">{move || t("clients.empty")}</p>
                        </div>
                    }.into_view();
                }
                list.into_iter().map(|c| {
                    let id = c.id.clone();
                    let name = c.name.clone();
                    let bound = c.bound;
                    view! {
                        <button class="row card" style="margin-bottom: 10px;"
                            attr:data-testid="client-row"
                            on:click=move |_| view.set(View::Client {
                                id: id.clone(), name: name.clone(),
                            })>
                            <div class="row__top">
                                <span class="row__title">{c.name.clone()}</span>
                                {(!bound).then(|| view! {
                                    <span class="badge badge--warn">{move || t("clients.pending")}</span>
                                })}
                            </div>
                        </button>
                    }
                }).collect_view()
            }}

            <button class="btn btn--primary btn--block" style="margin-top: 6px;"
                attr:data-testid="client-add" on:click=move |_| adding.set(true)>
                {move || t("clients.add")}
            </button>
        </div>
    }
}

/// Экран клиента. Пока согласия нет — на нём только ссылка; после согласия её
/// место занимают данные и запрос.
#[component]
fn ClientScreen(id: String, name: String, view: RwSignal<View>) -> impl IntoView {
    let cid = store_value(id.clone());
    let title = store_value(name.clone());
    let client = create_rw_signal(None::<api::Client>);
    let report = create_rw_signal(api::ReportResp::default());
    let error = create_rw_signal(None::<String>);
    let busy = create_rw_signal(false);
    let copied = create_rw_signal(false);
    let requested = create_rw_signal(false);
    /// Модалка выбора: что именно попросить.
    let asking = create_rw_signal(false);
    // Разобранный отчёт и открытый редактор планки.
    let parsed = create_rw_signal(None::<datashare::report::Report>);
    let parse_error = create_rw_signal(None::<String>);
    let editing = create_rw_signal(None::<String>);

    let reload = move || {
        spawn_local(async move {
            let id = cid.get_value();
            match api::clients().await {
                Ok(list) => client.set(list.into_iter().find(|c| c.id == id)),
                Err(e) => {
                    if e.is_auth() {
                        auth::logout();
                        view.set(View::Login);
                        return;
                    }
                    error.set(Some(e.message().to_string()));
                }
            }
            match api::report(&id).await {
                Ok(r) => {
                    // Непонятый отчёт — громкая ошибка на экране, а не пустое
                    // место: молча проглотить его значит спрятать поломку
                    // протокола до следующего разбирательства.
                    match r.report.as_deref().map(datashare::report::parse) {
                        Some(Ok(rep)) => {
                            parsed.set(Some(rep));
                            parse_error.set(None);
                        }
                        Some(Err(e)) => {
                            leptos::logging::error!("отчёт клиента: {e}");
                            parsed.set(None);
                            parse_error.set(Some(e));
                        }
                        None => parsed.set(None),
                    }
                    report.set(r);
                }
                // Отчёта может не быть вовсе — это не ошибка, а состояние.
                Err(e) if !e.is_auth() => leptos::logging::warn!("отчёт: {e}"),
                Err(_) => {}
            }
        });
    };
    create_effect(move |_| reload());

    let ask = move |scope: datashare::report::Scope| {
        if busy.get_untracked() {
            return;
        }
        busy.set(true);
        asking.set(false);
        error.set(None);
        spawn_local(async move {
            match api::request_data(&cid.get_value(), scope).await {
                Ok(_) => {
                    requested.set(true);
                    reload();
                }
                Err(e) => error.set(Some(e.message().to_string())),
            }
            busy.set(false);
        });
    };

    let unbind = move |_| {
        if busy.get_untracked() {
            return;
        }
        if !confirm(t("client.unbind_confirm")) {
            return;
        }
        busy.set(true);
        spawn_local(async move {
            match api::unbind_client(&cid.get_value()).await {
                Ok(_) => reload(),
                Err(e) => error.set(Some(e.message().to_string())),
            }
            busy.set(false);
        });
    };

    let remove = move |_| {
        if busy.get_untracked() {
            return;
        }
        if !confirm(t("client.delete_confirm")) {
            return;
        }
        busy.set(true);
        spawn_local(async move {
            match api::delete_client(&cid.get_value()).await {
                Ok(_) => view.set(View::Clients),
                Err(e) => {
                    error.set(Some(e.message().to_string()));
                    busy.set(false);
                }
            }
        });
    };

    view! {
        <div class="appbar">
            <button class="btn btn--icon btn--ghost" on:click=move |_| view.set(View::Clients)>
                <svg viewBox="0 0 24 24"><polyline points="15 18 9 12 15 6"/></svg>
            </button>
            <div class="appbar__title">{title.get_value()}</div>
        </div>
        <div class="screen pad">
            {move || error.get().map(|e| view! { <div class="banner">{e}</div> })}

            {move || match client.get() {
                None => view! { <div class="spinner"></div> }.into_view(),

                // Не привязан — только приглашение. Данных нет и быть не может:
                // согласия человек ещё не давал.
                Some(c) if !c.bound => {
                    let link = c.invite_code.as_deref().map(invite_url).unwrap_or_default();
                    let to_copy = link.clone();
                    view! {
                        <div class="card">
                            <p style="font-weight: 620;">{move || t("client.invite_title")}</p>
                            <p class="sub" style="margin: 8px 0 14px;">{move || t("client.invite_body")}</p>
                            <p class="code-box" style="font-size: .82rem; letter-spacing: 0; \
                                word-break: break-all; text-align: left;"
                                attr:data-testid="invite-link">{link}</p>
                            <button class="btn btn--primary btn--block" style="margin-top: 12px;"
                                attr:data-testid="invite-copy"
                                on:click=move |_| { copy_to_clipboard(&to_copy); copied.set(true); }>
                                {move || if copied.get() { t("clients.copied") } else { t("clients.copy_link") }}
                            </button>
                        </div>
                        <button class="btn btn--danger btn--block" style="margin-top: 16px;"
                            prop:disabled=move || busy.get() on:click=remove>
                            {move || t("client.delete")}
                        </button>
                    }.into_view()
                }

                Some(_) => {
                    let r = report.get();
                    view! {
                        <div class="card" style="margin-bottom: 12px;">
                            <button class="btn btn--primary btn--block"
                                attr:data-testid="request-data"
                                prop:disabled=move || busy.get()
                                on:click=move |_| { error.set(None); asking.set(true); }>
                                {move || t("client.request")}
                            </button>
                            <p class="sub" style="margin-top: 10px; font-size: .82rem;">
                                {match (&r.report_at, &r.request_at) {
                                    (_, Some(at)) => t("client.waiting")
                                        .replace("{date}", at.get(0..10).unwrap_or("")),
                                    (Some(at), None) => t("client.report_at")
                                        .replace("{date}", at.get(0..10).unwrap_or("")),
                                    (None, None) => t("client.no_report").to_string(),
                                }}
                            </p>
                            {move || requested.get().then(|| view! {
                                <p class="sub" style="margin-top: 6px; color: var(--accent);"
                                    attr:data-testid="request-sent">{move || t("client.requested")}</p>
                            })}
                            {move || asking.get().then(|| view! {
                                <AskChoice has_report=report.get().report.is_some()
                                    ask=ask on_close=Callback::new(move |_| asking.set(false)) />
                            })}
                        </div>

                        <button class="btn btn--ghost btn--block" style="margin-bottom: 12px;"
                            attr:data-testid="client-chat"
                            on:click=move |_| view.set(View::Chat {
                                id: cid.get_value(), name: title.get_value(),
                            })>
                            {move || t("client.chat")}
                        </button>

                        {move || parse_error.get().map(|e| view! { <div class="banner">{e}</div> })}
                        {move || parsed.get().map(|rep| datashare::report::render(
                            &rep,
                            Callback::new(move |key: String| editing.set(Some(key))),
                        ))}
                        // Редактор открывается только по разобранному отчёту: из
                        // него он берёт и текущее число, и данные для расчёта.
                        {move || match (editing.get(), parsed.get()) {
                            (Some(key), Some(rep)) => view! {
                                <PlankaEditor
                                    client_id=cid.get_value()
                                    key=key
                                    report=rep
                                    on_close=Callback::new(move |changed: bool| {
                                        editing.set(None);
                                        if changed { reload(); }
                                    })/>
                            }.into_view(),
                            _ => ().into_view(),
                        }}

                        <button class="btn btn--danger btn--block" style="margin-top: 16px;"
                            attr:data-testid="client-unbind"
                            prop:disabled=move || busy.get() on:click=unbind>
                            {move || t("client.unbind")}
                        </button>
                        <button class="btn btn--ghost btn--block" style="margin-top: 8px;"
                            prop:disabled=move || busy.get() on:click=remove>
                            {move || t("client.delete")}
                        </button>
                    }.into_view()
                }
            }}
        </div>
    }
}

/// Правка одной планки — одно число.
///
/// Замка нет: сама привязка к куратору выключает автопересчёт, и пока он ведёт
/// человека, планки не двигаются без него. «Вернуть расчётную» тоже нет —
/// пересчитать планку по последним данным куратор может сам, а отправит он всё
/// равно ЧИСЛО.
///
/// Директива несёт число и вид. Текст, который человек увидит в чате и в письме,
/// собирается у него и на его языке — здесь его не составляют.
#[component]
fn PlankaEditor(
    client_id: String,
    key: String,
    report: datashare::report::Report,
    on_close: Callback<bool>,
) -> impl IntoView {
    let cid = store_value(client_id);
    let k = store_value(key.clone());
    let value = create_rw_signal(
        report.targets.value(&key).map(|v| format!("{v}")).unwrap_or_default(),
    );
    let busy = create_rw_signal(false);
    let error = create_rw_signal(None::<String>);

    // «Рассчитать» есть только у двух планок — у тех, что приложение ведёт само:
    // калории (недельный цикл) и белок (доля от них). Остальные десять — нормы, и
    // считать в них нечего: они и так стоят на нашем правиле, пока куратор его не
    // заменил своим числом.
    //
    // Число берётся из ОБЩЕГО кода: куратор видит ровно то, к чему пришло бы
    // приложение само. Дальше он вправе поправить его и отправить своё — но
    // отправит он в любом случае ЧИСЛО, а не «пересчитай».
    let suggested = store_value(match key.as_str() {
        "calories" => report.suggest().map(|s| s.calories),
        "protein" => report.suggest().and_then(|s| s.protein),
        _ => None,
    });
    let calculate = move |_| {
        if let Some(v) = suggested.get_value() {
            value.set(format!("{v:.0}"));
            error.set(None);
        }
    };

    let apply = move |_| {
        if busy.get_untracked() {
            return;
        }
        let raw = value.get_untracked();
        let Ok(amount) = raw.trim().replace(',', ".").parse::<f64>() else {
            error.set(Some("Не число".to_string()));
            return;
        };
        busy.set(true);
        error.set(None);
        spawn_local(async move {
            match api::set_planka(&cid.get_value(), &k.get_value(), amount).await {
                Ok(_) => on_close.call(true),
                Err(e) => {
                    error.set(Some(e.message().to_string()));
                    busy.set(false);
                }
            }
        });
    };

    view! {
        <div style="position: fixed; inset: 0; z-index: 60; background: rgba(0,0,0,.55); \
                    display: flex; align-items: flex-end;"
            attr:data-testid="planka-editor"
            on:click=move |_| on_close.call(false)>
            <div class="card" style="width: 100%; border-radius: 18px 18px 0 0; padding: 20px;"
                on:click=|ev| ev.stop_propagation()>
                <p style="font-weight: 640; font-size: 1.05rem;">{move || t("planka.edit")}</p>

                <p class="sub" style="margin: 14px 0 6px; font-size: .82rem;">
                    {move || t("planka.value")}
                </p>
                <input class="field" attr:data-testid="planka-value"
                    prop:value=move || value.get()
                    on:input=move |ev| value.set(event_target_value(&ev)) />

                {suggested.get_value().map(|_| view! {
                    <button class="btn btn--ghost btn--block" style="margin-top: 10px;"
                        attr:data-testid="planka-calc"
                        prop:disabled=move || busy.get() on:click=calculate>
                        {move || t("planka.calc")}
                    </button>
                    <p class="sub" style="margin: 6px 0 0; font-size: .78rem;">
                        {move || t("planka.calc_hint")}
                    </p>
                })}

                {move || error.get().map(|e| view! { <div class="banner">{e}</div> })}

                <button class="btn btn--primary btn--block" style="margin-top: 18px;"
                    attr:data-testid="planka-save"
                    prop:disabled=move || busy.get() on:click=apply>
                    {move || t("planka.save")}
                </button>
                <button class="btn btn--ghost btn--block" style="margin-top: 8px;"
                    on:click=move |_| on_close.call(false)>
                    {move || t("clients.cancel")}
                </button>
            </div>
        </div>
    }
}

/// Пауза перед повтором. Нужна ровно в одном месте — в петле длинного опроса:
/// без неё отказ воркера превращает её в холостую крутилку, которая долбит
/// сервер тем чаще, чем ему хуже.
async fn delay(ms: i32) {
    let promise = js_sys::Promise::new(&mut |resolve, _| {
        if let Some(w) = web_sys::window() {
            let _ = w.set_timeout_with_callback_and_timeout_and_arguments_0(&resolve, ms);
        }
    });
    let _ = wasm_bindgen_futures::JsFuture::from(promise).await;
}

/// Подтверждение действия. Отвязка и удаление необратимы для другой стороны —
/// спросить обязательно.
fn confirm(message: &str) -> bool {
    web_sys::window()
        .and_then(|w| w.confirm_with_message(message).ok())
        .unwrap_or(false)
}

/// Переписка с клиентом. Тред ЛИЧНЫЙ: это разговор куратора с этим человеком, и
/// ни другой куратор, ни админ его не видят.
#[component]
fn ChatScreen(id: String, name: String, view: RwSignal<View>) -> impl IntoView {
    use std::cell::Cell;
    use std::rc::Rc;

    let cid = store_value(id.clone());
    let title = store_value(name.clone());
    let messages = create_rw_signal(Vec::<api::Message>::new());
    let input = create_rw_signal(String::new());
    let sending = create_rw_signal(false);
    let error = create_rw_signal(None::<String>);
    let last_seq = create_rw_signal(0u64);

    // Опрос живёт, пока экран открыт. Сторож на месте по той же причине, что в
    // приложении худеющего: без него петли складываются друг на друга при
    // быстром уходе и возврате.
    let alive = Rc::new(Cell::new(true));
    on_cleanup({
        let a = alive.clone();
        move || a.set(false)
    });

    let apply = move |page: api::MessagesPage| {
        if page.messages.is_empty() {
            return;
        }
        // Курсор двигаем по ответу СЕРВЕРА, а не по максимальному seq на руках:
        // иначе следующий длинный опрос ушёл бы с устаревшего места и вернулся
        // бы мгновенно, превратив ожидание в крутилку.
        let newest = page.next_after_seq.max(page.messages.iter().map(|m| m.seq).max().unwrap_or(0));
        messages.update(|list| {
            for m in page.messages {
                if !list.iter().any(|x| x.seq == m.seq) {
                    list.push(m);
                }
            }
            list.sort_by_key(|m| m.seq);
        });
        if newest > last_seq.get_untracked() {
            last_seq.set(newest);
            // Отметка прочтения — по факту показа, и только вперёд.
            spawn_local(async move {
                if let Err(e) = api::mark_read(&cid.get_value(), newest).await {
                    leptos::logging::warn!("отметка прочтения: {e}");
                }
            });
        }
    };

    // Первая загрузка и дальше — длинный опрос: воркер держит запрос открытым до
    // 25 секунд и отвечает, как только приходит новое.
    create_effect({
        let alive = alive.clone();
        move |_| {
            let alive = alive.clone();
            spawn_local(async move {
                match api::messages(&cid.get_value(), 0).await {
                    Ok(page) => apply(page),
                    Err(e) => {
                        if e.is_auth() {
                            auth::logout();
                            view.set(View::Login);
                            return;
                        }
                        error.set(Some(e.message().to_string()));
                    }
                }
                while alive.get() {
                    let after = last_seq.get_untracked();
                    match api::messages_wait(&cid.get_value(), after, 25).await {
                        Ok(page) => {
                            if !alive.get() {
                                return;
                            }
                            apply(page);
                        }
                        Err(e) => {
                            if e.is_auth() {
                                auth::logout();
                                view.set(View::Login);
                                return;
                            }
                            // Сеть моргнула. Отступаем и пробуем снова: без паузы
                            // отказ воркера превратил бы ожидание в холостую
                            // крутилку, которая долбит сервер тем чаще, чем ему
                            // хуже.
                            leptos::logging::warn!("опрос переписки: {e}");
                            delay(2000).await;
                        }
                    }
                }
            });
        }
    });

    let send = move |_| {
        if sending.get_untracked() {
            return;
        }
        let text = input.get_untracked().trim().to_string();
        if text.is_empty() {
            return;
        }
        sending.set(true);
        error.set(None);
        spawn_local(async move {
            match api::reply(&cid.get_value(), &text).await {
                Ok(_) => {
                    input.set(String::new());
                    if let Ok(page) = api::messages(&cid.get_value(), last_seq.get_untracked()).await
                    {
                        apply(page);
                    }
                }
                Err(e) => error.set(Some(e.message().to_string())),
            }
            sending.set(false);
        });
    };

    view! {
        <div class="appbar">
            <button class="btn btn--icon btn--ghost" on:click=move |_| view.set(View::Client {
                id: cid.get_value(), name: title.get_value(),
            })>
                <svg viewBox="0 0 24 24"><polyline points="15 18 9 12 15 6"/></svg>
            </button>
            <div class="appbar__title">{title.get_value()}</div>
        </div>
        // Экран чата — ТОТ ЖЕ, что видит худеющий: обои, пузыри и плашки берутся
        // из общего крейта `chat-ui`. Разговор один, и выглядеть он обязан
        // одинаково с обоих концов. Отличается только обрамление: у худеющего
        // чат занимает весь экран и живёт во вкладке, здесь он под шапкой с
        // кнопкой «назад» — навигация к виду переписки не относится.
        <div class="screen screen--noflow"
            style=format!("display: flex; flex-direction: column; position: relative; {}", chat_ui::WALLPAPER)>
            {move || error.get().map(|e| view! { <div class="banner">{e}</div> })}
            <div style=chat_ui::SCROLL attr:data-testid="curator-chat">
                <div style=chat_ui::WRAP>
                    <div style=chat_ui::PATTERN></div>
                    <div style=format!("{} padding: 12px 12px 6.5rem;", chat_ui::LIST)>
                        {move || {
                            let list = messages.get();
                            if list.is_empty() {
                                return view! {
                                    <p style="font-size: 12px; color: #69748C; text-align: center; padding: 20px;">
                                        {t("chat.empty")}
                                    </p>
                                }.into_view();
                            }
                            list.into_iter().map(|m| {
                                // Директивы — не разговор: их текст человек
                                // собирает у себя, и пузырь здесь был бы пустым.
                                if m.kind != "text" {
                                    return view! { <chat_ui::Note text=directive_note(&m) /> }
                                        .into_view();
                                }
                                view! {
                                    <chat_ui::Bubble text=m.text mine=m.sender == "expert"
                                        sender_name=None />
                                }.into_view()
                            }).collect_view()
                        }}
                    </div>
                </div>
            </div>
            <div style=chat_ui::COMPOSER>
                <div style="display: flex; align-items: flex-end; gap: 0.5rem;">
                    <textarea rows="1" style=chat_ui::TEXTAREA
                        placeholder=move || t("chat.placeholder")
                        attr:data-testid="chat-input"
                        prop:value=move || input.get()
                        on:input=move |ev| input.set(event_target_value(&ev))></textarea>
                    <button class="btn btn--primary" attr:data-testid="chat-send"
                        prop:disabled=move || sending.get() on:click=send>
                        {move || t("chat.send")}
                    </button>
                </div>
            </div>
        </div>
    }
}

/// Что попросить у клиента. Двумя кнопками, а не полем «за сколько дней».
///
/// Куратор не знает, сколько дней у клиента накопилось: он знает «я это уже
/// видел» и «я не видел ничего». Число дней — перевод, который ему пришлось бы
/// делать в уме, и ошибаться в нём.
///
/// Пока отчёта от клиента нет ни одного, «только новое» не показывается: новое
/// относительно ЧЕГО — ответа нет, и выбор был бы ложным.
#[component]
fn AskChoice(
    has_report: bool,
    ask: impl Fn(datashare::report::Scope) + Copy + 'static,
    on_close: Callback<()>,
) -> impl IntoView {
    use datashare::report::Scope;
    view! {
        <div style="position: fixed; inset: 0; z-index: 60; background: rgba(0,0,0,.55); \
                    display: flex; align-items: flex-end;"
            attr:data-testid="request-choice"
            on:click=move |_| on_close.call(())>
            <div class="card" style="width: 100%; border-radius: 18px 18px 0 0; padding: 20px;"
                on:click=|ev| ev.stop_propagation()>
                <p style="font-weight: 640; font-size: 1.05rem;">{move || t("client.request_what")}</p>
                {has_report.then(|| view! {
                    <button class="btn btn--primary btn--block" style="margin-top: 16px;"
                        attr:data-testid="request-new"
                        on:click=move |_| ask(Scope::New)>
                        {move || t("client.request_new")}
                    </button>
                    <p class="sub" style="margin-top: 6px; font-size: .8rem;">
                        {move || t("client.request_new_hint")}
                    </p>
                })}
                <button class=move || if has_report { "btn btn--block" } else { "btn btn--primary btn--block" }
                    style="margin-top: 12px;" attr:data-testid="request-all"
                    on:click=move |_| ask(Scope::All)>
                    {move || t("client.request_all")}
                </button>
                <button class="btn btn--ghost btn--block" style="margin-top: 8px;"
                    on:click=move |_| on_close.call(())>
                    {move || t("clients.cancel")}
                </button>
            </div>
        </div>
    }
}

/// Короткая пометка о служебном сообщении. Куратору важно видеть, что директива
/// ушла; полный текст её человек прочтёт у себя и на своём языке.
fn directive_note(m: &api::Message) -> String {
    match m.kind.as_str() {
        "data_request" => "запрос данных".to_string(),
        "data_share" => "получен отчёт".to_string(),
        "set_planka_v2" | "set_planka" => "правка планки".to_string(),
        "open_week" => "открыта тема".to_string(),
        other => other.to_string(),
    }
}

/// Порядок в списке: сверху те, от кого дольше всего нет отчёта.
///
/// Список — рабочая очередь, а не картотека. Куратор открывает его, чтобы
/// увидеть, кем пора заняться, и первым должен стоять тот, о ком он дольше всего
/// ничего не знает.
///
/// Три ступени, и порядок между ними важнее сортировки внутри:
/// 1. Привязанные БЕЗ единого отчёта — человек согласился и молчит. Это самый
///    громкий случай, и никакая давность не должна его перебивать.
/// 2. Привязанные с отчётом — по давности, старые выше.
/// 3. Непривязанные слоты — приглашение ещё не приняли, отчёту взяться неоткуда,
///    и торопить тут некого.
fn sorted(mut list: Vec<api::Client>) -> Vec<api::Client> {
    list.sort_by(|a, b| {
        let rank = |c: &api::Client| match (c.bound, c.last_report_at.is_some()) {
            (true, false) => 0,
            (true, true) => 1,
            _ => 2,
        };
        rank(a)
            .cmp(&rank(b))
            // Даты в RFC3339 — сравнение строк совпадает со сравнением времени.
            .then_with(|| a.last_report_at.cmp(&b.last_report_at))
            // Устойчивость: без имени два одинаково молчащих клиента менялись бы
            // местами при каждом обновлении списка.
            .then_with(|| a.name.cmp(&b.name))
    });
    list
}

#[cfg(test)]
mod sort_tests {
    use super::*;

    fn c(name: &str, bound: bool, at: Option<&str>) -> api::Client {
        api::Client {
            id: name.into(),
            name: name.into(),
            invite_code: None,
            bound,
            bound_at: None,
            last_report_at: at.map(str::to_string),
            request_scope: None,
            request_at: None,
        }
    }

    #[test]
    fn molchashchie_vyshe_otchitavshihsya_a_slovy_v_konce() {
        let out = sorted(vec![
            c("свежий", true, Some("2026-08-27T10:00:00Z")),
            c("слот", false, None),
            c("давний", true, Some("2026-08-20T10:00:00Z")),
            c("молчит", true, None),
        ]);
        let names: Vec<&str> = out.iter().map(|x| x.name.as_str()).collect();
        assert_eq!(names, vec!["молчит", "давний", "свежий", "слот"]);
    }
}

/// Настройки куратора: имя и язык./// Настройки куратора: имя и язык.
///
/// Имя видят клиенты — в приглашении и под каждым его сообщением. Язык здесь
/// только про этот интерфейс: тексты, которые получает худеющий, собираются у
/// него и на ЕГО языке.
#[component]
fn Settings(view: RwSignal<View>) -> impl IntoView {
    let name = create_rw_signal(String::new());
    let lang = create_rw_signal(i18n::get());
    let busy = create_rw_signal(false);
    let saved = create_rw_signal(false);
    let error = create_rw_signal(None::<String>);
    // Уведомления. Подписка заводилась ТОЛЬКО при входе и молча: отказ в ней
    // намеренно не мешает войти. Значит куратор, у которого она не удалась,
    // узнавал об этом лишь по тому, что сообщения клиентов не приходят, и
    // починить не мог — включить её было негде.
    let push_on = create_rw_signal(crate::push::is_subscribed());
    let push_busy = create_rw_signal(false);
    let push_err = create_rw_signal(None::<String>);

    create_effect(move |_| {
        spawn_local(async move {
            match api::me().await {
                Ok(Some(c)) => {
                    name.set(c.name);
                    if c.lang == "en" {
                        lang.set(Lang::En);
                    }
                }
                Ok(None) => {}
                Err(e) => {
                    if e.is_auth() {
                        auth::logout();
                        view.set(View::Login);
                        return;
                    }
                    error.set(Some(e.message().to_string()));
                }
            }
        });
    });

    let save = move |_| {
        if busy.get_untracked() {
            return;
        }
        let n = name.get_untracked().trim().to_string();
        let l = lang.get_untracked();
        busy.set(true);
        error.set(None);
        spawn_local(async move {
            let code = if l == Lang::En { "en" } else { "ru" };
            match api::set_profile(&n, code).await {
                Ok(_) => {
                    // Язык интерфейса переключается тут же — ждать перезапуска
                    // ради собственной настройки незачем.
                    i18n::set(l);
                    saved.set(true);
                }
                Err(e) => error.set(Some(e.message().to_string())),
            }
            busy.set(false);
        });
    };

    view! {
        <div class="appbar">
            <button class="btn btn--icon btn--ghost" on:click=move |_| view.set(View::Clients)>
                <svg viewBox="0 0 24 24"><polyline points="15 18 9 12 15 6"/></svg>
            </button>
            <div class="appbar__title">{move || t("settings.title")}</div>
        </div>
        <div class="screen pad">
            {move || error.get().map(|e| view! { <div class="banner">{e}</div> })}
            <div class="card">
                <p class="sub" style="font-size: .82rem;">{move || t("settings.name")}</p>
                <input class="field" style="margin-top: 6px;" attr:data-testid="settings-name"
                    prop:value=move || name.get()
                    on:input=move |ev| name.set(event_target_value(&ev)) />
                <p class="sub" style="margin-top: 8px; font-size: .8rem;">
                    {move || t("settings.name_hint")}
                </p>
            </div>

            <div class="card" style="margin-top: 12px;">
                <p class="sub" style="font-size: .82rem;">{move || t("settings.lang")}</p>
                <div class="seg" style="margin-top: 8px;">
                    <button class=move || if lang.get() == Lang::Ru { "seg__btn seg__btn--on" } else { "seg__btn" }
                        attr:data-testid="settings-lang-ru"
                        on:click=move |_| lang.set(Lang::Ru)>"Русский"</button>
                    <button class=move || if lang.get() == Lang::En { "seg__btn seg__btn--on" } else { "seg__btn" }
                        attr:data-testid="settings-lang-en"
                        on:click=move |_| lang.set(Lang::En)>"English"</button>
                </div>
            </div>

            <div class="card" style="margin-top: 12px;">
                <p class="sub" style="font-size: .82rem;">{move || t("settings.push")}</p>
                {move || if !crate::push::is_supported() {
                    // На iOS `Notification` и `PushManager` существуют только в
                    // УСТАНОВЛЕННОМ приложении. Во вкладке кнопка была бы обманом.
                    view! {
                        <p class="sub" style="margin-top: 8px; font-size: .8rem;">
                            {move || t("settings.push_unsupported")}
                        </p>
                    }.into_view()
                } else {
                    view! {
                        <p class="sub" style="margin-top: 8px; font-size: .8rem;">
                            {move || if push_on.get() { t("settings.push_on") } else { t("settings.push_off") }}
                        </p>
                        <button class="btn btn--block" style="margin-top: 10px;"
                            attr:data-testid="settings-push"
                            prop:disabled=move || push_busy.get()
                            on:click=move |_| {
                                if push_busy.get_untracked() { return; }
                                push_busy.set(true);
                                push_err.set(None);
                                spawn_local(async move {
                                    match crate::push::subscribe().await {
                                        Ok(()) => push_on.set(true),
                                        Err(e) => {
                                            leptos::logging::error!("подписка на уведомления: {e}");
                                            push_err.set(Some(t("settings.push_failed").to_string()));
                                        }
                                    }
                                    push_busy.set(false);
                                });
                            }>
                            {move || if push_on.get() { t("settings.push_again") } else { t("settings.push_enable") }}
                        </button>
                        {move || push_err.get().map(|e| view! {
                            <p class="sub" style="margin-top: 8px; font-size: .8rem; color: var(--danger);">{e}</p>
                        })}
                    }.into_view()
                }}
            </div>

            <button class="btn btn--primary btn--block" style="margin-top: 16px;"
                attr:data-testid="settings-save"
                prop:disabled=move || busy.get() on:click=save>
                {move || t("settings.save")}
            </button>
            {move || saved.get().then(|| view! {
                <p class="sub" style="margin-top: 8px; color: var(--accent);"
                    attr:data-testid="settings-saved">{move || t("settings.saved")}</p>
            })}

            <button class="btn btn--danger btn--block" style="margin-top: 28px;"
                attr:data-testid="settings-logout"
                on:click=move |_| { auth::logout(); view.set(View::Login); }>
                {move || t("settings.logout")}
            </button>
        </div>
    }
}
