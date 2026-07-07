use leptos::*;

use crate::api::{self, ConversationSummary, Message};
use crate::auth;
use crate::datashare;

/// The six admin slash-commands: (command typed, dataset key, human menu label,
/// RU panel text sent as the message .text fallback).
const SLASH_COMMANDS: [(&str, &str, &str, &str); 6] = [
    ("/request-body-params", "body", "Параметры тела", "Куратор запрашивает у вас параметры тела"),
    ("/request-food-diary", "food", "Дневник питания", "Куратор запрашивает у вас ваш дневник питания"),
    ("/request-story-progress", "story", "Задания", "Куратор запрашивает у вас ваши текущие и выполненные задания"),
    ("/request-weight", "weight", "Дневник веса", "Куратор запрашивает у вас ваш дневник веса"),
    ("/request-steps", "steps", "Дневник шагов", "Куратор запрашивает у вас ваш дневник шагов"),
    ("/request-all", "all", "Все данные", "Куратор запрашивает у вас все ваши данные"),
];

/// Which screen is showing. Thread carries the selected user's id + display label.
#[derive(Clone, PartialEq)]
enum View {
    Login,
    /// Authenticated but NOT yet an approved expert: request a code and hand it
    /// to the operator out-of-band. Re-mounting this view re-checks /admin/me.
    RequestAccess,
    Queue,
    Thread { user_id: String, label: String },
    /// Operator worklist of paid-but-unbound payments (manual refund in lava).
    Payments,
}

#[component]
pub fn App() -> impl IntoView {
    // Treat an expired/missing token as logged-out so we never enter the authed
    // UI on a dead session and discover it via a raw 401 on the first poll.
    if auth::get_token().is_some() && !auth::has_live_session() {
        auth::logout();
    }
    // A live session is NOT proof of expert approval anymore: it may be an
    // unapproved candidate. Land on RequestAccess, whose on-mount /admin/me check
    // flips an approved expert straight to Queue and leaves a candidate on the
    // request-access screen (never silently stranded on Login).
    let view = create_rw_signal(if auth::has_live_session() { View::RequestAccess } else { View::Login });

    view! {
        <div class="app">
            {move || match view.get() {
                View::Login => view! { <Login view=view /> }.into_view(),
                View::RequestAccess => view! { <RequestAccess view=view /> }.into_view(),
                View::Queue => view! { <Queue view=view /> }.into_view(),
                View::Thread { user_id, label } => {
                    view! { <Thread view=view user_id=user_id label=label /> }.into_view()
                }
                View::Payments => view! { <Payments view=view /> }.into_view(),
            }}
        </div>
    }
}

/// Which main section a bottom-tab targets (for the active highlight).
#[derive(Clone, Copy, PartialEq)]
enum Section {
    Queue,
    Payments,
}

/// Persistent bottom navigation shared by the three main authed screens.
#[component]
fn TabBar(view: RwSignal<View>, active: Section) -> impl IntoView {
    let on = move |s: Section| if s == active { "tab tab--on" } else { "tab" };
    view! {
        <nav class="tabbar">
            <button class=move || on(Section::Queue) attr:data-testid="tab-queue"
                on:click=move |_| view.set(View::Queue)>
                <svg viewBox="0 0 24 24"><path d="M5 5h14a2 2 0 0 1 2 2v7a2 2 0 0 1-2 2H9l-4 4V7a2 2 0 0 1 0-2z"/></svg>
                "Очередь"
            </button>
            <button class=move || on(Section::Payments) attr:data-testid="tab-payments"
                on:click=move |_| view.set(View::Payments)>
                <svg viewBox="0 0 24 24"><rect x="3" y="6" width="18" height="12" rx="2.5"/><path d="M3 10.5h18"/></svg>
                "Платежи"
            </button>
        </nav>
    }
}

#[component]
fn Login(view: RwSignal<View>) -> impl IntoView {
    let name = create_rw_signal(String::new());
    let busy = create_rw_signal(false);
    let error = create_rw_signal(Option::<String>::None);

    let sign_in = move |_| {
        busy.set(true);
        error.set(None);
        spawn_local(async move {
            match auth::authenticate().await {
                // A signed-in user is not necessarily an approved expert. Send them
                // to RequestAccess, whose on-mount /admin/me check routes an approved
                // expert to Queue and a candidate to the request-access screen.
                Ok(_) => view.set(View::RequestAccess),
                Err(e) => error.set(Some(e)),
            }
            busy.set(false);
        });
    };

    let register = move |_| {
        let n = name.get().trim().to_string();
        if n.is_empty() {
            error.set(Some("Введите имя эксперта".to_string()));
            return;
        }
        busy.set(true);
        error.set(None);
        spawn_local(async move {
            match auth::register(&n).await {
                Ok(_uid) => {
                    // The new expert is not yet approved. They no longer need a manual
                    // EXPERT_IDS edit — after signing in they self-serve a request code
                    // on the request-access screen and hand it to the operator. Drop the
                    // freshly-created session so they sign in cleanly with the passkey.
                    auth::logout();
                    error.set(Some(
                        "Эксперт зарегистрирован. Войдите паскеем и запросите доступ.".to_string(),
                    ));
                }
                Err(e) => error.set(Some(e)),
            }
            busy.set(false);
        });
    };

    view! {
        <div class="center">
            <div class="brandmark"></div>
            <h1 class="h1">"re:Norma"</h1>
            <p class="sub">"Операторская консоль"</p>

            <button class="btn btn--primary btn--block" style="margin-bottom: 14px;"
                disabled=move || busy.get() on:click=sign_in>
                {move || if busy.get() { "…" } else { "Войти паскеем" }}
            </button>

            <details style="margin-top: 6px;">
                <summary style="color: var(--muted); cursor: pointer; font-size: .9rem; padding: 6px 0;">
                    "Первый вход на этом устройстве"
                </summary>
                <div style="margin-top: 12px; display: flex; gap: 8px;">
                    <input class="field" style="flex: 1;" placeholder="Имя эксперта"
                        prop:value=move || name.get()
                        on:input=move |e| name.set(event_target_value(&e)) />
                    <button class="btn" disabled=move || busy.get() on:click=register>
                        "Создать"
                    </button>
                </div>
            </details>

            {move || error.get().map(|e| view! {
                <p style="color: var(--danger); white-space: pre-wrap; margin-top: 16px;">{e}</p>
            })}
        </div>
    }
}

/// Screen for an authenticated-but-not-yet-approved candidate. On mount it
/// re-checks /admin/me: an approved expert is sent to Queue, otherwise the
/// candidate sees (or requests) their short access code to give the operator.
/// Re-mounting (the "Проверить доступ" button) re-runs the check, so once the
/// operator approves the code the candidate flips to Queue.
#[component]
fn RequestAccess(view: RwSignal<View>) -> impl IntoView {
    let code = create_rw_signal(Option::<String>::None);
    let busy = create_rw_signal(false);
    let error = create_rw_signal(Option::<String>::None);

    // Re-check approval via /admin/me. An approved expert flips to Queue; a
    // candidate stays here (showing their existing code, if any). Used both on
    // mount and by the "Проверить доступ" button — re-setting View::RequestAccess
    // would be a no-op (PartialEq) and wouldn't re-poll, so we call this directly.
    let recheck = move || {
        spawn_local(async move {
            match api::admin_me().await {
                Ok(me) if me.approved => view.set(View::Queue),
                // Not approved yet: show the existing code if one was already requested.
                Ok(me) => code.set(me.code),
                // A dead token (auth_user 401) means the session is gone → back to Login.
                Err(e) if e.is_auth() => {
                    auth::logout();
                    view.set(View::Login);
                }
                // Any other failure is surfaced, never silently swallowed.
                Err(e) => error.set(Some(e.message().to_string())),
            }
        });
    };

    // On mount: re-check approval. Handles both the returning-candidate case and
    // the just-approved case (operator approved the code → flip to Queue).
    recheck();

    let request = move |_| {
        busy.set(true);
        error.set(None);
        spawn_local(async move {
            match api::admin_request().await {
                Ok(c) => code.set(Some(c)),
                Err(e) if e.is_auth() => {
                    auth::logout();
                    view.set(View::Login);
                }
                Err(e) => error.set(Some(e.message().to_string())),
            }
            busy.set(false);
        });
    };

    view! {
        <div class="center">
            <div class="brandmark"></div>
            <h1 class="h1">"Доступ к консоли"</h1>
            <p class="sub">"Запросите код и передайте его оператору."</p>

            {move || match code.get() {
                None => view! {
                    <button class="btn btn--primary btn--block"
                        disabled=move || busy.get() on:click=request>
                        {move || if busy.get() { "…" } else { "Запросить доступ" }}
                    </button>
                }.into_view(),
                Some(c) => view! {
                    <div>
                        <p style="margin: 0 0 8px; color: var(--muted); font-size: .9rem;">"Ваш код доступа"</p>
                        <code class="code-box">{c}</code>
                        <p style="color: var(--muted); margin: 14px 0; font-size: .9rem;">
                            "Передайте этот код оператору. После одобрения нажмите «Проверить доступ»."
                        </p>
                        <button class="btn btn--primary btn--block" on:click=move |_| recheck()>
                            "Проверить доступ"
                        </button>
                    </div>
                }.into_view(),
            }}

            <button class="btn btn--ghost btn--block" style="margin-top: 14px;"
                on:click=move |_| { auth::logout(); view.set(View::Login); }>
                "Выйти"
            </button>

            {move || error.get().map(|e| view! {
                <p style="color: var(--danger); white-space: pre-wrap; margin-top: 16px;">{e}</p>
            })}
        </div>
    }
}

/// Relative "waiting" label from an RFC3339 timestamp.
fn waiting_label(since: &str) -> String {
    let Ok(t) = chrono::DateTime::parse_from_rfc3339(since) else {
        return String::new();
    };
    let secs = (chrono::Utc::now() - t.with_timezone(&chrono::Utc)).num_seconds().max(0);
    if secs < 60 {
        "ждёт <1 мин".to_string()
    } else if secs < 3600 {
        format!("ждёт {} мин", secs / 60)
    } else if secs < 86_400 {
        format!("ждёт {} ч", secs / 3600)
    } else {
        format!("ждёт {} дн", secs / 86_400)
    }
}

/// Which queue tab is active. Drives both the loader and the auto-poll target.
#[derive(Clone, Copy, PartialEq)]
enum Tab {
    Pending,
    Answered,
}

#[component]
fn Queue(view: RwSignal<View>) -> impl IntoView {
    let items = create_rw_signal(Vec::<ConversationSummary>::new());
    let error = create_rw_signal(Option::<String>::None);
    let loading = create_rw_signal(true);
    let tab = create_rw_signal(Tab::Pending);

    let load = move || {
        loading.set(true);
        let active = tab.get_untracked();
        spawn_local(async move {
            let result = match active {
                Tab::Pending => api::list_pending(None).await,
                Tab::Answered => api::list_answered(None).await,
            };
            match result {
                Ok(page) => {
                    items.set(page.conversations);
                    error.set(None);
                }
                // A dead session (401 expired / 403 not an expert) must not keep
                // polling: clear it and return to Login with a clear message.
                Err(e) if e.is_auth() => {
                    auth::logout();
                    view.set(View::Login);
                }
                Err(e) => error.set(Some(e.message().to_string())),
            }
            loading.set(false);
        });
    };

    load();

    // Switch tabs: set the active tab and immediately reload so we don't show the
    // previous tab's rows until the next poll tick.
    let switch = move |t: Tab| {
        if tab.get_untracked() != t {
            tab.set(t);
            items.set(Vec::new());
            load();
        }
    };

    // Auto-refresh the queue so the longest-waiting stays current without manual taps.
    // Fail loudly if the timer can't be registered rather than silently never refreshing.
    let handle = match set_interval_with_handle(move || load(), std::time::Duration::from_secs(5)) {
        Ok(h) => Some(h),
        Err(e) => {
            logging::error!("queue auto-refresh timer failed to start: {e:?}");
            error.set(Some("Авто-обновление очереди не запустилось".to_string()));
            None
        }
    };
    on_cleanup(move || { if let Some(h) = handle { h.clear(); } });

    view! {
        <header class="appbar">
            <div class="ring"></div>
            <div class="appbar__title">"Очередь"</div>
            <button class="btn btn--ghost btn--icon" attr:aria-label="Обновить" on:click=move |_| load()>
                <svg viewBox="0 0 24 24"><path d="M21 12a9 9 0 1 1-2.6-6.4M21 4v5h-5"/></svg>
            </button>
            <button class="btn btn--ghost btn--icon" attr:aria-label="Выйти"
                on:click=move |_| { auth::logout(); view.set(View::Login); }>
                <svg viewBox="0 0 24 24"><path d="M15 3h4a2 2 0 0 1 2 2v14a2 2 0 0 1-2 2h-4M10 17l-5-5 5-5M15 12H5"/></svg>
            </button>
        </header>

        <div class="screen">
            <div class="pad" style="padding-bottom: 4px;">
                <div class="seg">
                    <button class=move || if tab.get() == Tab::Pending { "seg__btn seg__btn--on" } else { "seg__btn" }
                        on:click=move |_| switch(Tab::Pending)>"Ожидают"</button>
                    <button class=move || if tab.get() == Tab::Answered { "seg__btn seg__btn--on" } else { "seg__btn" }
                        on:click=move |_| switch(Tab::Answered)>"Отвеченные"</button>
                </div>
            </div>

            {move || error.get().map(|e| view! { <div class="banner">{e}</div> })}

            {move || {
                let list = items.get();
                if list.is_empty() {
                    if loading.get() {
                        return view! { <div class="spinner"></div> }.into_view();
                    }
                    let empty = match tab.get() {
                        Tab::Pending => "Нет ожидающих обращений",
                        Tab::Answered => "Нет отвеченных обращений",
                    };
                    return view! {
                        <div class="empty"><div class="empty__ring"></div><p>{empty}</p></div>
                    }.into_view();
                }
                view! {
                    <div class="list">
                        {list.into_iter().enumerate().map(|(i, c)| {
                            let label = c.user_id.clone();
                            let uid = c.user_id.clone();
                            let label_for_click = label.clone();
                            let waiting = c.pending_since.as_deref().map(waiting_label).unwrap_or_default();
                            let has_wait = !waiting.is_empty();
                            view! {
                                <button attr:data-testid="conv" class="row reveal"
                                    style=format!("--i:{i}")
                                    on:click=move |_| view.set(View::Thread {
                                        user_id: uid.clone(), label: label_for_click.clone(),
                                    })>
                                    <div class="row__top">
                                        <span class="row__title">{label}</span>
                                        {has_wait.then(|| view! {
                                            <span class="badge badge--warn badge--plain">{waiting.clone()}</span>
                                        })}
                                    </div>
                                    <div class="row__sub">{c.preview}</div>
                                </button>
                            }
                        }).collect_view()}
                    </div>
                }.into_view()
            }}
        </div>

        <TabBar view=view active=Section::Queue/>
    }
}

#[component]
fn Thread(view: RwSignal<View>, user_id: String, label: String) -> impl IntoView {
    let messages = create_rw_signal(Vec::<Message>::new());
    let error = create_rw_signal(Option::<String>::None);
    let draft = create_rw_signal(String::new());
    let sending = create_rw_signal(false);
    // The dataset(s) whose shared payload is open in the modal (one modal at a time).
    let shared_open = create_rw_signal(Option::<datashare::Dataset>::None);
    // True while a list_messages fetch is outstanding, so the 4s poll and the
    // post-reply refresh don't race and clobber each other with stale data.
    let in_flight = create_rw_signal(false);
    // Highest seq we've already marked read, so we only POST /read when it advances
    // instead of every single poll tick.
    let read_seq = create_rw_signal(0u64);

    // `load` is a Callback (Copy) so it can be reused by both the initial fetch and
    // the post-reply refresh without moving it out of the FnMut click handler.
    let uid_load = user_id.clone();
    let load = Callback::new(move |_: ()| {
        if in_flight.get_untracked() {
            return;
        }
        in_flight.set(true);
        let uid = uid_load.clone();
        spawn_local(async move {
            match api::list_messages(&uid, 0).await {
                Ok(page) => {
                    if let Some(last) = page.messages.last() {
                        let seq = last.seq;
                        // Only advance the server-side read marker when there is
                        // genuinely a newer message than we last marked.
                        if seq > read_seq.get_untracked() {
                            let uid2 = uid.clone();
                            spawn_local(async move {
                                match api::mark_read(&uid2, seq).await {
                                    Ok(()) => read_seq.set(seq),
                                    Err(e) if e.is_auth() => {
                                        auth::logout();
                                        view.set(View::Login);
                                    }
                                    Err(e) => error.set(Some(format!("mark_read: {}", e.message()))),
                                }
                            });
                        }
                    }
                    messages.set(page.messages);
                    error.set(None);
                }
                Err(e) if e.is_auth() => {
                    auth::logout();
                    view.set(View::Login);
                }
                Err(e) => error.set(Some(e.message().to_string())),
            }
            in_flight.set(false);
        });
    });

    load.call(());

    // Poll the open thread for the user's new messages.
    // Fail loudly if the timer can't be registered rather than silently not polling.
    let handle = match set_interval_with_handle(move || load.call(()), std::time::Duration::from_secs(4)) {
        Ok(h) => Some(h),
        Err(e) => {
            logging::error!("thread poll timer failed to start: {e:?}");
            error.set(Some("Авто-обновление переписки не запустилось".to_string()));
            None
        }
    };
    on_cleanup(move || { if let Some(h) = handle { h.clear(); } });

    let uid_send = user_id.clone();
    let send = move |_| {
        let text = draft.get().trim().to_string();
        if text.is_empty() {
            return;
        }
        sending.set(true);
        let uid = uid_send.clone();
        spawn_local(async move {
            match api::reply(&uid, &text).await {
                Ok(_) => {
                    draft.set(String::new());
                    load.call(());
                }
                Err(e) if e.is_auth() => {
                    auth::logout();
                    view.set(View::Login);
                }
                Err(e) => error.set(Some(e.message().to_string())),
            }
            sending.set(false);
        });
    };

    // Fire a data_request for `dataset` with its RU panel text, then clear the draft.
    let uid_req = user_id.clone();
    let send_request = Callback::new(move |(dataset, text): (String, String)| {
        sending.set(true);
        let uid = uid_req.clone();
        spawn_local(async move {
            match api::reply_data_request(&uid, &dataset, &text).await {
                Ok(_) => {
                    draft.set(String::new());
                    load.call(());
                }
                Err(e) if e.is_auth() => {
                    auth::logout();
                    view.set(View::Login);
                }
                Err(e) => error.set(Some(e.message().to_string())),
            }
            sending.set(false);
        });
    });

    view! {
        <header class="appbar">
            <button class="btn btn--ghost btn--icon" attr:aria-label="Назад"
                on:click=move |_| view.set(View::Queue)>
                <svg viewBox="0 0 24 24"><path d="M15 18l-6-6 6-6"/></svg>
            </button>
            <div style="flex: 1; min-width: 0;">
                <div class="appbar__title mono">{label}</div>
                <div class="appbar__sub">"переписка · обновляется"</div>
            </div>
        </header>

        {move || error.get().map(|e| view! { <div class="banner">{e}</div> })}

        <div class="screen screen--noflow">
            <div class="msgs">
                {move || messages.get().into_iter().map(|m| {
                    let is_expert = m.sender == "expert";
                    let side_cls = if is_expert { "bubble--me" } else { "bubble--them" };
                    match m.kind.as_str() {
                        // The user shared data: render one labelled button per dataset;
                        // tap opens the modal. A broken payload surfaces loudly.
                        "data_share" => {
                            // payload is a RAW JSON STRING from the worker — parse it first.
                            let datasets = match m.payload.as_deref() {
                                Some(raw) => serde_json::from_str::<serde_json::Value>(raw)
                                    .map_err(|e| format!("payload не JSON: {e}"))
                                    .and_then(|v| datashare::datasets_from_payload(&v)),
                                None => Err("data_share без payload".to_string()),
                            };
                            match datasets {
                                Ok(list) => view! {
                                    <div attr:data-testid="msg" attr:data-sender=m.sender.clone()
                                         class=format!("bubble {side_cls}")
                                         style="display:flex; flex-direction:column; gap:6px; align-items:stretch;">
                                        {list.into_iter().map(|ds| {
                                            let label = ds.label();
                                            let ds2 = ds.clone();
                                            view! {
                                                <button attr:data-testid="data-share-btn"
                                                    class="btn btn--ghost"
                                                    style="justify-content:flex-start;"
                                                    on:click=move |_| shared_open.set(Some(ds2.clone()))>
                                                    {label}
                                                </button>
                                            }
                                        }).collect_view()}
                                    </div>
                                }.into_view(),
                                Err(e) => view! {
                                    <div class="bubble bubble--them"
                                         style="color:var(--danger);">
                                        {format!("Не удалось прочитать данные: {e}")}
                                    </div>
                                }.into_view(),
                            }
                        }
                        // A data_request the admin itself sent → compact "запрошено" chip.
                        "data_request" => {
                            let what = m.payload.as_deref()
                                .and_then(|raw| serde_json::from_str::<serde_json::Value>(raw).ok())
                                .and_then(|v| v.get("dataset").and_then(|d| d.as_str()).map(str::to_string))
                                .map(|d| dataset_ru(&d))
                                .unwrap_or_else(|| "данные".to_string());
                            view! {
                                <div attr:data-testid="msg" attr:data-sender=m.sender.clone()
                                     class=format!("bubble {side_cls}")
                                     style="opacity:.9; font-size:.9rem;">
                                    <span class="mono">"⤴ запрошено: "</span>{what}
                                </div>
                            }.into_view()
                        }
                        // Plain text (default / legacy).
                        _ => view! {
                            <div attr:data-testid="msg" attr:data-sender=m.sender.clone()
                                 class=format!("bubble {side_cls}")>
                                {m.text}
                            </div>
                        }.into_view(),
                    }
                }).collect_view()}
            </div>

            // Slash-command menu: shown when the draft starts with "/". Selecting a
            // command SENDS the corresponding data_request and clears the draft.
            {move || {
                let d = draft.get();
                if !d.starts_with('/') {
                    return ().into_view();
                }
                let q = d.to_lowercase();
                view! {
                    <div attr:data-testid="slash-menu"
                         style="position:sticky; bottom:0; margin:0 16px; background:var(--surface); \
                                border:1px solid var(--line); border-radius:var(--r); overflow:hidden; \
                                box-shadow:var(--shadow); z-index:25;">
                        {SLASH_COMMANDS.iter()
                            .filter(|(cmd, _, _, _)| cmd.starts_with(&q))
                            .map(|(cmd, dataset, label, panel_text)| {
                                let dataset = dataset.to_string();
                                let panel_text = panel_text.to_string();
                                view! {
                                    <button attr:data-testid="slash-item"
                                        style="display:flex; flex-direction:column; align-items:flex-start; gap:2px; \
                                               width:100%; text-align:left; padding:10px 14px; \
                                               border-bottom:1px solid var(--line-soft);"
                                        on:click=move |_| {
                                            send_request.call((dataset.clone(), panel_text.clone()));
                                        }>
                                        <span style="font-weight:600;">{*label}</span>
                                        <span class="mono row__meta">{*cmd}</span>
                                    </button>
                                }
                            }).collect_view()}
                    </div>
                }.into_view()
            }}

            <div class="composer">
                <textarea attr:data-testid="reply-input" class="field" rows="1"
                    style="flex: 1; resize: none; max-height: 120px;" placeholder="Ответ… (или / для запроса данных)"
                    prop:value=move || draft.get()
                    on:input=move |e| draft.set(event_target_value(&e)) />
                <button attr:data-testid="reply-send" class="btn btn--primary btn--icon"
                    attr:aria-label="Отправить" disabled=move || sending.get() on:click=send>
                    {move || if sending.get() {
                        view! { <span>"…"</span> }.into_view()
                    } else {
                        view! { <svg viewBox="0 0 24 24"><path d="M22 2L11 13M22 2l-7 20-4-9-9-4z"/></svg> }.into_view()
                    }}
                </button>
            </div>

            // Shared-data modal (reuses the receipt-detail modal pattern).
            {move || shared_open.get().map(|ds| {
                let title = ds.title();
                let body = datashare::render_dataset(&ds);
                view! {
                    <div on:click=move |_| shared_open.set(None)
                         style="position:fixed; inset:0; background:rgba(0,0,0,0.55); z-index:60; \
                                display:flex; align-items:center; justify-content:center; padding:16px;">
                        <div on:click=move |ev: leptos::ev::MouseEvent| ev.stop_propagation()
                             attr:data-testid="data-share-modal"
                             style="background:var(--surface); color:var(--text); max-width:660px; width:100%; \
                                    max-height:86vh; overflow:auto; border-radius:12px; border:1px solid var(--line);">
                            <div style="display:flex; justify-content:space-between; align-items:center; \
                                        padding:12px 16px; border-bottom:1px solid var(--line); \
                                        position:sticky; top:0; background:var(--surface);">
                                <b>{title}</b>
                                <button class="btn btn--ghost" on:click=move |_| shared_open.set(None)>"✕"</button>
                            </div>
                            <div style="padding:14px 16px;">{body}</div>
                        </div>
                    </div>
                }
            })}
        </div>
    }
}

/// Human RU name for a dataset key (for the compact "запрошено" chip).
fn dataset_ru(key: &str) -> String {
    match key {
        "body" => "параметры тела",
        "food" => "дневник питания",
        "story" => "задания",
        "weight" => "дневник веса",
        "steps" => "дневник шагов",
        "all" => "все данные",
        other => other,
    }
    .to_string()
}

/// ms-epoch → coarse "N назад" label for the payments worklist.
fn since_label(ms: i64) -> String {
    if ms <= 0 {
        return String::new();
    }
    let now = js_sys::Date::now();
    let secs = ((now - ms as f64) / 1000.0).max(0.0) as i64;
    if secs < 60 {
        "только что".to_string()
    } else if secs < 3600 {
        format!("{} мин назад", secs / 60)
    } else if secs < 86_400 {
        format!("{} ч назад", secs / 3600)
    } else {
        format!("{} дн назад", secs / 86_400)
    }
}

/// Format a minor-unit (×100) amount as major units + currency, e.g. 5000/"RUB" → "50 RUB".
fn fmt_money(amount: Option<i64>, currency: Option<&str>) -> String {
    match amount {
        Some(a) => {
            let cur = currency.unwrap_or("");
            let s = if a % 100 == 0 {
                format!("{} {}", a / 100, cur)
            } else {
                format!("{}.{:02} {}", a / 100, (a % 100).abs(), cur)
            };
            s.trim().to_string()
        }
        None => "—".into(),
    }
}

/// Operator worklist: paid-but-unbound payments. The server reconciles this list
/// against lava on load — contracts lava reports refunded/cancelled (terminatedAt) are
/// auto-voided and drop off here, so this shows only still-active unbound payments.
/// (No manual "mark voided" button anymore.)
#[component]
fn Payments(view: RwSignal<View>) -> impl IntoView {
    let items = create_rw_signal(Vec::<api::UnboundPayment>::new());
    let refunds = create_rw_signal(Vec::<api::RefundRequest>::new());
    let no_access = create_rw_signal(Vec::<api::PaidNoAccess>::new());
    let receipts = create_rw_signal(Vec::<api::Receipt>::new());
    // The receipt whose full body is open in the modal (fetched on demand).
    let selected = create_rw_signal(Option::<api::ReceiptFull>::None);
    let error = create_rw_signal(Option::<String>::None);
    let loading = create_rw_signal(true);

    let load = Callback::new(move |_: ()| {
        loading.set(true);
        spawn_local(async move {
            match api::unbound_payments().await {
                Ok(list) => {
                    items.set(list);
                    error.set(None);
                }
                Err(e) if e.is_auth() => {
                    auth::logout();
                    view.set(View::Login);
                }
                Err(e) => error.set(Some(e.message().to_string())),
            }
            // Refund requests — best-effort; a failure here shouldn't blank the page.
            match api::refund_requests().await {
                Ok(list) => refunds.set(list),
                Err(e) if e.is_auth() => {
                    auth::logout();
                    view.set(View::Login);
                }
                Err(e) => error.set(Some(e.message().to_string())),
            }
            // Paid-but-no-access — best-effort.
            match api::paid_no_access().await {
                Ok(list) => no_access.set(list),
                Err(e) if e.is_auth() => {
                    auth::logout();
                    view.set(View::Login);
                }
                Err(_) => {}
            }
            // Caught receipts — best-effort.
            match api::receipts().await {
                Ok(list) => receipts.set(list),
                Err(e) if e.is_auth() => {
                    auth::logout();
                    view.set(View::Login);
                }
                Err(_) => {}
            }
            loading.set(false);
        });
    });
    load.call(());

    view! {
        <header class="appbar">
            <div class="ring"></div>
            <div style="flex: 1; min-width: 0;">
                <div class="appbar__title">"Платежи"</div>
                <div class="appbar__sub">"непривязанные · сверено с lava"</div>
            </div>
            <button class="btn btn--ghost btn--icon" attr:aria-label="Обновить" on:click=move |_| load.call(())>
                <svg viewBox="0 0 24 24"><path d="M21 12a9 9 0 1 1-2.6-6.4M21 4v5h-5"/></svg>
            </button>
        </header>

        <div class="screen">
            {move || error.get().map(|e| view! { <div class="banner">{e}</div> })}

            // Paid but no access yet (no passkey) — nudge them to finish onboarding.
            {move || {
                let list = no_access.get();
                (!list.is_empty()).then(|| view! {
                    <div style="padding: 16px 16px 2px;">
                        <span class="badge badge--danger">{format!("Оплатили, нет доступа · {}", list.len())}</span>
                    </div>
                    <div class="list">
                        {list.into_iter().enumerate().map(|(i, r)| {
                            let who = r.tg_username.clone()
                                .map(|u| format!("@{u}"))
                                .or_else(|| r.tg_user_id.map(|id| format!("tg:{id}")))
                                .or_else(|| r.user_id.clone())
                                .unwrap_or_else(|| "—".into());
                            let amount = match (r.amount, r.currency.clone()) {
                                (Some(a), Some(c)) => format!("{a} {c}"),
                                _ => "—".into(),
                            };
                            let when = r.paid_at.or(r.created_at).map(since_label).unwrap_or_default();
                            view! {
                                <div attr:data-testid="no-access-row" class="row reveal" style=format!("--i:{i}")>
                                    <div class="row__top">
                                        <span class="row__title">{who}</span>
                                        <span class="badge badge--danger">"нет ключа"</span>
                                    </div>
                                    <div class="row__sub mono">{amount}</div>
                                    <div class="row__meta">{when}</div>
                                </div>
                            }
                        }).collect_view()}
                    </div>
                })
            }}

            // Refund requests: client asked for a refund, access already revoked.
            // Process each manually in lava (using the contract id / email).
            {move || {
                let list = refunds.get();
                (!list.is_empty()).then(|| view! {
                    <div style="padding: 16px 16px 2px;">
                        <span class="badge badge--danger">{format!("Запросы на возврат · {}", list.len())}</span>
                    </div>
                    <div class="list">
                        {list.into_iter().enumerate().map(|(i, r)| {
                            let cur = if r.currency.is_empty() { "RUB".to_string() } else { r.currency.clone() };
                            let amount = format!("{} {}", r.amount, cur);
                            let email = r.email.clone().unwrap_or_else(|| r.user_id.clone());
                            let contract = r.contract_id.clone().unwrap_or_else(|| "—".to_string());
                            let mut meta = String::new();
                            if let Some(d) = r.days_left { meta.push_str(&format!("остаток {d} дн.")); }
                            if let Some(c) = r.created_at {
                                if !meta.is_empty() { meta.push_str(" · "); }
                                meta.push_str(&since_label(c));
                            }
                            view! {
                                <div attr:data-testid="refund-row" class="row reveal" style=format!("--i:{i}")>
                                    <div class="row__top">
                                        <span class="row__title mono">{amount}</span>
                                        <span class="badge badge--danger">"возврат"</span>
                                    </div>
                                    <div class="row__sub">{email}</div>
                                    <div class="row__meta">"lava: "<span class="mono">{contract}</span></div>
                                    <div class="row__meta">{meta}</div>
                                </div>
                            }
                        }).collect_view()}
                    </div>
                })
            }}

            // Caught receipts (bound to a payment). Tap a row → full text in a modal.
            {move || {
                let list = receipts.get();
                (!list.is_empty()).then(|| view! {
                    <div style="padding: 16px 16px 2px;">
                        <span class="badge">{format!("Чеки · {}", list.len())}</span>
                    </div>
                    <div class="list">
                        {list.into_iter().enumerate().map(|(i, r)| {
                            let who = r.tg_username.clone().map(|u| format!("@{u}"))
                                .or_else(|| r.tg_user_id.map(|id| format!("tg:{id}")))
                                .or_else(|| r.user_id.clone())
                                .or_else(|| r.email.clone())
                                .unwrap_or_else(|| "—".into());
                            let amount = fmt_money(r.amount, r.currency.as_deref());
                            let when = r.received_at.map(since_label).unwrap_or_default();
                            let id = r.id.clone();
                            let open = move |_| {
                                let id = id.clone();
                                spawn_local(async move {
                                    if let Ok(Some(full)) = api::receipt_detail(&id).await {
                                        selected.set(Some(full));
                                    }
                                });
                            };
                            view! {
                                <div attr:data-testid="receipt-row" class="row reveal"
                                     style=format!("--i:{i}; cursor:pointer;") on:click=open>
                                    <div class="row__top">
                                        <span class="row__title mono">{amount}</span>
                                        <span class="badge">"чек"</span>
                                    </div>
                                    <div class="row__sub">{who}</div>
                                    <div class="row__meta">{when}</div>
                                </div>
                            }
                        }).collect_view()}
                    </div>
                })
            }}

            {move || {
                let list = items.get();
                if list.is_empty() {
                    if loading.get() {
                        return view! { <div class="spinner"></div> }.into_view();
                    }
                    return view! {
                        <div class="empty"><div class="empty__ring"></div>
                            <p>"Нет непривязанных платежей"</p></div>
                    }.into_view();
                }
                view! {
                    <div class="list">
                        {list.into_iter().enumerate().map(|(i, p)| {
                            let amount = match (p.amount, p.currency.clone()) {
                                (Some(a), Some(c)) => format!("{a} {c}"),
                                (Some(a), None) => a.to_string(),
                                _ => "—".to_string(),
                            };
                            let email = p.email.clone().unwrap_or_else(|| "—".to_string());
                            let contract = p.contract_id.clone().unwrap_or_else(|| "—".to_string());
                            let waited = p.paid_at.map(since_label).unwrap_or_default();
                            let has_wait = !waited.is_empty();
                            view! {
                                <div attr:data-testid="payment-row" class="row reveal" style=format!("--i:{i}")>
                                    <div class="row__top">
                                        <span class="row__title mono">{amount}</span>
                                        {has_wait.then(|| view! {
                                            <span class="badge badge--warn badge--plain">{waited.clone()}</span>
                                        })}
                                    </div>
                                    <div class="row__sub">{email}</div>
                                    <div class="row__meta">"lava: "<span class="mono">{contract}</span></div>
                                </div>
                            }
                        }).collect_view()}
                    </div>
                }.into_view()
            }}

            // Receipt detail: the full rendered receipt body.
            {move || selected.get().map(|full| {
                let body = full.body_text.clone().unwrap_or_default();
                let amount = fmt_money(full.amount, full.currency.as_deref());
                let when = full.received_at.map(since_label).unwrap_or_default();
                view! {
                    <div on:click=move |_| selected.set(None)
                         style="position:fixed; inset:0; background:rgba(0,0,0,0.55); z-index:60; \
                                display:flex; align-items:center; justify-content:center; padding:16px;">
                        <div on:click=move |ev: leptos::ev::MouseEvent| ev.stop_propagation()
                             style="background:#fff; color:#111; max-width:660px; width:100%; \
                                    max-height:86vh; overflow:auto; border-radius:12px;">
                            <div style="display:flex; justify-content:space-between; align-items:center; \
                                        padding:12px 16px; border-bottom:1px solid #eee; position:sticky; top:0; background:#fff;">
                                <div><b>"Чек"</b>" · "<span class="mono">{amount}</span>" · "{when}</div>
                                <button class="btn btn--ghost" on:click=move |_| selected.set(None)>"✕"</button>
                            </div>
                            <div inner_html=body style="padding:12px 16px;"></div>
                        </div>
                    </div>
                }
            })}
        </div>

        <TabBar view=view active=Section::Payments/>
    }
}
