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
                    <input class="field" style="margin-top: 12px;"
                        placeholder=move || t("login.name")
                        attr:data-testid="curator-name"
                        prop:value=move || name.get()
                        on:input=move |ev| name.set(event_target_value(&ev)) />
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
                    clients.set(list);
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
    let days = create_rw_signal(String::from("1"));
    let requested = create_rw_signal(false);
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

    let ask = move |_| {
        if busy.get_untracked() {
            return;
        }
        let n = days.get_untracked().trim().parse::<u32>().unwrap_or(1);
        if n == 0 || n > 366 {
            error.set(Some("Срок — от 1 до 366 дней".to_string()));
            return;
        }
        busy.set(true);
        error.set(None);
        spawn_local(async move {
            match api::request_data(&cid.get_value(), n).await {
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
                            <div style="display: flex; gap: 8px; align-items: center;">
                                <input class="field" type="number" min="1" max="366"
                                    style="max-width: 92px;" attr:data-testid="request-days"
                                    prop:value=move || days.get()
                                    on:input=move |ev| days.set(event_target_value(&ev)) />
                                <span class="sub" style="font-size: .8rem;">{move || t("client.request_days")}</span>
                                <button class="btn btn--primary" style="margin-left: auto;"
                                    attr:data-testid="request-data"
                                    prop:disabled=move || busy.get() on:click=ask>
                                    {move || t("client.request")}
                                </button>
                            </div>
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
                        {move || editing.get().map(|key| view! {
                            <PlankaEditor
                                client_id=cid.get_value()
                                key=key
                                targets=parsed.get().map(|r| r.targets).unwrap_or_default()
                                on_close=Callback::new(move |changed: bool| {
                                    editing.set(None);
                                    if changed { reload(); }
                                })/>
                        })}

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

/// Индикаторы, у которых автопересчёт вообще есть, — только им нужен замок.
/// У констант пересчитывать нечего, и переключатель там был бы враньём.
const RECOMPUTED: &[&str] = &["calories", "steps", "protein", "veg_fruit", "iron", "fiber"];

/// Правка одной планки: значение и запрет автопересчёта.
///
/// Директива несёт ЧИСЛО. Текст, который человек увидит в чате и в письме,
/// собирается у него и на его языке — здесь его не составляют.
#[component]
fn PlankaEditor(
    client_id: String,
    key: String,
    targets: datashare::report::Targets,
    on_close: Callback<bool>,
) -> impl IntoView {
    let cid = store_value(client_id);
    let k = store_value(key.clone());
    let current = targets.value(&key);
    let curated = targets.by_curator(&key).cloned();
    let recomputed = RECOMPUTED.contains(&key.as_str());

    let value = create_rw_signal(
        curated
            .as_ref()
            .and_then(|c| c.amount)
            .or(current)
            .map(|v| format!("{v}"))
            .unwrap_or_default(),
    );
    let locked = create_rw_signal(curated.as_ref().map(|c| c.locked).unwrap_or(false));
    let busy = create_rw_signal(false);
    let error = create_rw_signal(None::<String>);

    let apply = move |_| {
        if busy.get_untracked() {
            return;
        }
        let raw = value.get_untracked();
        let amount = raw.trim().replace(',', ".").parse::<f64>().ok();
        if !raw.trim().is_empty() && amount.is_none() {
            error.set(Some("Не число".to_string()));
            return;
        }
        busy.set(true);
        error.set(None);
        spawn_local(async move {
            match api::set_planka(&cid.get_value(), &k.get_value(), amount, locked.get_untracked())
                .await
            {
                Ok(_) => on_close.call(true),
                Err(e) => {
                    error.set(Some(e.message().to_string()));
                    busy.set(false);
                }
            }
        });
    };

    // «Вернуть расчётную» — это директива БЕЗ значения и без замка: запись
    // куратора перестаёт перекрывать наше правило.
    let reset = move |_| {
        if busy.get_untracked() {
            return;
        }
        busy.set(true);
        spawn_local(async move {
            match api::set_planka(&cid.get_value(), &k.get_value(), None, false).await {
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

                {recomputed.then(|| view! {
                    <label style="display: flex; gap: 10px; align-items: flex-start; margin-top: 16px;">
                        <input type="checkbox" attr:data-testid="planka-lock"
                            prop:checked=move || locked.get()
                            on:change=move |ev| locked.set(event_target_checked(&ev)) />
                        <span>
                            <span style="font-weight: 600;">{move || t("planka.lock")}</span>
                            <span class="sub" style="display: block; font-size: .8rem;">
                                {move || t("planka.lock_hint")}
                            </span>
                        </span>
                    </label>
                })}

                {move || error.get().map(|e| view! { <div class="banner">{e}</div> })}

                <button class="btn btn--primary btn--block" style="margin-top: 18px;"
                    attr:data-testid="planka-save"
                    prop:disabled=move || busy.get() on:click=apply>
                    {move || t("planka.save")}
                </button>
                {curated.is_some().then(|| view! {
                    <button class="btn btn--ghost btn--block" style="margin-top: 8px;"
                        attr:data-testid="planka-reset"
                        prop:disabled=move || busy.get() on:click=reset>
                        {move || t("planka.reset")}
                    </button>
                })}
                <button class="btn btn--ghost btn--block" style="margin-top: 8px;"
                    on:click=move |_| on_close.call(false)>
                    {move || t("clients.cancel")}
                </button>
            </div>
        </div>
    }
}

/// Подтверждение действия. Отвязка и удаление необратимы для другой стороны —
/// спросить обязательно.
fn confirm(message: &str) -> bool {
    web_sys::window()
        .and_then(|w| w.confirm_with_message(message).ok())
        .unwrap_or(false)
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
