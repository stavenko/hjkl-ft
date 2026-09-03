use leptos::*;

use crate::api::{self, ConversationSummary, Message};
use crate::auth;
use crate::datashare;

/// The admin slash-commands: (command typed, dataset key, human menu label,
/// RU panel text sent as the message .text fallback).
const SLASH_COMMANDS: [(&str, &str, &str, &str); 6] = [
    ("/request-body-params", "body", "Параметры тела", "Куратор запрашивает у вас параметры тела"),
    ("/request-food-diary", "food", "Дневник питания", "Куратор запрашивает у вас ваш дневник питания"),
    ("/request-weight", "weight", "Дневник веса", "Куратор запрашивает у вас ваш дневник веса"),
    ("/request-steps", "steps", "Дневник шагов", "Куратор запрашивает у вас ваш дневник шагов"),
    ("/request-system", "system", "Данные об устройстве", "Куратор запрашивает у вас данные об устройстве и браузере"),
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
    /// lava.top subscriptions/contracts NOT bound to any account in our DB.
    Subscriptions,
    /// Per-user AI-token consumption histogram (payment-worker UsageDO).
    Usage,
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
                View::Subscriptions => view! { <Subscriptions view=view /> }.into_view(),
                View::Usage => view! { <Usage view=view /> }.into_view(),
            }}
        </div>
    }
}

/// Which main section a bottom-tab targets (for the active highlight).
#[derive(Clone, Copy, PartialEq)]
enum Section {
    Queue,
    Payments,
    Subscriptions,
    Usage,
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
                "Пользователи"
            </button>
            <button class=move || on(Section::Subscriptions) attr:data-testid="tab-subscriptions"
                on:click=move |_| view.set(View::Subscriptions)>
                <svg viewBox="0 0 24 24"><path d="M21 12a9 9 0 1 1-2.6-6.4M21 4v5h-5"/><circle cx="12" cy="12" r="3.2"/></svg>
                "Подписки"
            </button>
            <button class=move || on(Section::Usage) attr:data-testid="tab-usage"
                on:click=move |_| view.set(View::Usage)>
                <svg viewBox="0 0 24 24"><path d="M4 20V10M10 20V4M16 20v-7M22 20H2"/></svg>
                "Токены"
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
    // The initial /admin/me is in flight. While an authorized admin is being
    // resolved (→ Queue), show a loader instead of flashing the access form.
    // Cleared only for outcomes that actually KEEP us on this screen (candidate,
    // or a non-auth error); approved/dead-token navigate away, so the loader
    // stays until the view switches.
    let checking = create_rw_signal(true);

    // Re-check approval via /admin/me. An approved expert flips to Queue; a
    // candidate stays here (showing their existing code, if any). Used both on
    // mount and by the "Проверить доступ" button — re-setting View::RequestAccess
    // would be a no-op (PartialEq) and wouldn't re-poll, so we call this directly.
    let recheck = move || {
        spawn_local(async move {
            match api::admin_me().await {
                Ok(me) if me.approved => view.set(View::Queue),
                // Not approved yet: show the existing code if one was already requested.
                Ok(me) => {
                    code.set(me.code);
                    checking.set(false);
                }
                // A dead token (auth_user 401) means the session is gone → back to Login.
                Err(e) if e.is_auth() => {
                    auth::logout();
                    view.set(View::Login);
                }
                // Any other failure is surfaced, never silently swallowed.
                Err(e) => {
                    error.set(Some(e.message().to_string()));
                    checking.set(false);
                }
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
            {move || if checking.get() {
                // Authorized session being resolved — loader, not the access form.
                return view! { <div class="spinner"></div> }.into_view();
            } else {
                ().into_view()
            }}

            {move || (!checking.get()).then(|| view! {
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

/// Await `ms` milliseconds (setTimeout-backed) — used to back off the long-poll
/// loop after a transient error without busy-spinning.
async fn worker_delay(ms: i32) {
    let promise = js_sys::Promise::new(&mut |resolve, _| {
        if let Some(w) = web_sys::window() {
            let _ = w.set_timeout_with_callback_and_timeout_and_arguments_0(&resolve, ms);
        }
    });
    let _ = wasm_bindgen_futures::JsFuture::from(promise).await;
}

#[component]
fn Thread(view: RwSignal<View>, user_id: String, label: String) -> impl IntoView {
    let messages = create_rw_signal(Vec::<Message>::new());
    let error = create_rw_signal(Option::<String>::None);
    // Positive feedback (e.g. planka set) — shown in a green banner, not the red one.
    let notice = create_rw_signal(Option::<String>::None);
    let draft = create_rw_signal(String::new());
    let sending = create_rw_signal(false);
    // The dataset(s) whose shared payload is open in the modal (one modal at a time).
    let shared_open = create_rw_signal(Option::<datashare::Dataset>::None);
    // Карточка пользователя прямо из переписки. Разговор почти всегда про самого
    // человека («не заходит», «сбросьте мне онбординг»), а его id у треда уже есть —
    // ходить за ним в список пользователей и искать там строку незачем.
    let card_open = create_rw_signal(false);
    let uid_card = store_value(user_id.clone());
    // Auto-scroll to the newest message (same as the client chat): the thread opens
    // pinned to the bottom and follows new messages, but a user who scrolled up to
    // read history isn't yanked down until they return near the bottom.
    let msgs_ref = create_node_ref::<leptos::html::Div>();
    let stick_bottom = create_rw_signal(true);
    let scroll_to_bottom = move || {
        if let Some(el) = msgs_ref.get() {
            el.set_scroll_top(el.scroll_height());
        }
    };
    let on_msgs_scroll = move |_| {
        if let Some(el) = msgs_ref.get() {
            let dist = el.scroll_height() - el.scroll_top() - el.client_height();
            stick_bottom.set(dist < 120);
        }
    };
    create_effect(move |_| {
        messages.get();
        if stick_bottom.get_untracked() {
            request_animation_frame(scroll_to_bottom);
        }
    });
    // True while a list_messages fetch is outstanding, so the 4s poll and the
    // post-reply refresh don't race and clobber each other with stale data.
    let in_flight = create_rw_signal(false);
    // Highest seq we've already marked read, so we only POST /read when it advances
    // instead of every single poll tick.
    let read_seq = create_rw_signal(0u64);
    // Highest seq currently shown — the `after_seq` the long-poll waits past. Kept
    // in sync by `load`; the change-detector loop advances it before refreshing so
    // it can never long-poll from 0 (which returns instantly and would busy-loop).
    let last_seq = create_rw_signal(0u64);

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
                    // Track the max seq shown so the long-poll waits past it.
                    last_seq.set(page.messages.last().map(|m| m.seq).unwrap_or(0));
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

    // Watch the open thread via LONG-POLL instead of a fixed interval: the worker
    // holds each request open (~25s) and returns the moment a newer message lands,
    // so new messages are near-instant AND we make ~1 request / 25s (was every 4s),
    // which also collapses the CORS preflight rate. Sequential loop (no interval);
    // a stop flag flipped on cleanup ends it when the thread closes.
    let uid_poll = user_id.clone();
    let stop = std::rc::Rc::new(std::cell::Cell::new(false));
    let stop_cleanup = stop.clone();
    spawn_local(async move {
        loop {
            if stop.get() {
                break;
            }
            let after = last_seq.get_untracked();
            match api::list_messages_wait(&uid_poll, after, 25).await {
                Ok(page) => {
                    if stop.get() {
                        break;
                    }
                    if !page.messages.is_empty() {
                        // Advance BEFORE refreshing so the next long-poll can't fire
                        // from a stale `after` and spin. `load` re-renders + marks read.
                        last_seq.set(page.next_after_seq);
                        load.call(());
                    }
                    // Empty = the wait window elapsed with no new message → loop.
                }
                Err(e) if e.is_auth() => {
                    auth::logout();
                    view.set(View::Login);
                    break;
                }
                Err(_) => {
                    // Transient (network / worker hiccup): back off, then retry.
                    worker_delay(2000).await;
                }
            }
        }
    });
    on_cleanup(move || stop_cleanup.set(true));

    let uid_send = user_id.clone();
    let send = move |_| {
        let text = draft.get().trim().to_string();
        if text.is_empty() {
            return;
        }
        // Slash ACTION: `/set-calorie-limit <kcal>` sets the client's calorie planka
        // (not a chat message). Parse the amount and call the planka endpoint.
        if let Some(rest) = text.strip_prefix("/set-calorie-limit") {
            let amount = rest.trim().replace(',', ".").parse::<f64>().ok();
            match amount {
                Some(a) if a > 0.0 && a < 20000.0 => {
                    sending.set(true);
                    let uid = uid_send.clone();
                    spawn_local(async move {
                        match api::set_planka(&uid, a).await {
                            Ok(_seq) => {
                                draft.set(String::new());
                                error.set(None);
                                // The app applies it on its side — this is a directive,
                                // not an immediate server-side write.
                                notice.set(Some(format!("✓ Планка отправлена клиенту: {a:.0} ккал")));
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
                }
                _ => error.set(Some(
                    "Укажите калорийность числом, например: /set-calorie-limit 2600".to_string(),
                )),
            }
            return;
        }
        // Slash ACTION: `/open-week <номер>` открывает клиенту тему — те же номера,
        // что у историй в ленте. Нужна, когда гейт не может открыть её сам.
        if let Some(rest) = text.strip_prefix("/open-week") {
            match rest.trim().parse::<u32>().ok().filter(|w| (3..=7).contains(w)) {
                Some(week) => {
                    sending.set(true);
                    let uid = uid_send.clone();
                    spawn_local(async move {
                        match api::open_week(&uid, week).await {
                            Ok(_seq) => {
                                draft.set(String::new());
                                error.set(None);
                                notice.set(Some(format!("✓ Тема №{week} отправлена клиенту")));
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
                }
                None => error.set(Some(
                    "Номер темы от 3 до 7: 3 активность, 4 кальций, 5 железо, 6 жиры, \
                     7 красное мясо. Например: /open-week 7"
                        .to_string(),
                )),
            }
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

    // Fire a data_request for `dataset` with its RU panel text.
    let uid_req = user_id.clone();
    let send_request = Callback::new(move |(dataset, text): (String, String)| {
        // Close the menu + clear the input IMMEDIATELY on tap: the menu is bound to
        // the draft starting with "/", so clearing `draft` synchronously (before the
        // round-trip) hides it and empties the textarea, making the tap feel done.
        draft.set(String::new());
        sending.set(true);
        let uid = uid_req.clone();
        spawn_local(async move {
            match api::reply_data_request(&uid, &dataset, &text).await {
                // The sent request shows up as a "⤴ запрошено: …" chip on refresh.
                Ok(_) => load.call(()),
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
            <button attr:data-testid="thread-user-card" class="btn btn--ghost"
                attr:aria-label="Карточка пользователя"
                on:click=move |_| card_open.set(true)>
                "Карточка"
            </button>
        </header>

        {move || error.get().map(|e| view! { <div class="banner">{e}</div> })}
        {move || notice.get().map(|n| view! { <div class="banner banner--ok">{n}</div> })}

        <div class="screen screen--noflow" node_ref=msgs_ref on:scroll=on_msgs_scroll>
            <div class="msgs">
                {move || messages.get().into_iter().map(|m| {
                    let is_expert = m.sender == "expert";
                    let side_cls = if is_expert { "bubble--me" } else { "bubble--them" };
                    // Время сообщения — строкой под пузырём. Эксперту важно, КОГДА
                    // человек написал: на «час назад» и «в прошлый вторник» отвечают
                    // по-разному, а в ленте это было ниоткуда не видно.
                    let row_cls = if is_expert { "msg-row msg-row--me" } else { "msg-row msg-row--them" };
                    let stamp = fmt_msg_time(&m.created_at);
                    let bubble = match m.kind.as_str() {
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
                        // A set_planka directive the admin sent → compact chip. The
                        // client app applies it on its side.
                        "set_planka" => {
                            let amt = m.payload.as_deref()
                                .and_then(|raw| serde_json::from_str::<serde_json::Value>(raw).ok())
                                .and_then(|v| v.get("amount").and_then(|a| a.as_f64()))
                                .map(|a| format!("{a:.0} ккал"))
                                .unwrap_or_else(|| "—".to_string());
                            view! {
                                <div attr:data-testid="msg" attr:data-sender=m.sender.clone()
                                     class=format!("bubble {side_cls}")
                                     style="opacity:.9; font-size:.9rem;">
                                    <span class="mono">"⤴ планка: "</span>{amt}
                                </div>
                            }.into_view()
                        }
                        // Директива открытия темы — тоже компактной плашкой:
                        // применяет её само приложение.
                        "open_week" => {
                            let week = m.payload.as_deref()
                                .and_then(|raw| serde_json::from_str::<serde_json::Value>(raw).ok())
                                .and_then(|v| v.get("week").and_then(|w| w.as_u64()))
                                .map(|w| format!("№{w}"))
                                .unwrap_or_else(|| "—".to_string());
                            view! {
                                <div attr:data-testid="msg" attr:data-sender=m.sender.clone()
                                     class=format!("bubble {side_cls}")
                                     style="opacity:.9; font-size:.9rem;">
                                    <span class="mono">"⤴ открыта тема: "</span>{week}
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
                    };
                    view! {
                        <div class=row_cls>
                            {bubble}
                            <span attr:data-testid="msg-time" class="msg-time">{stamp}</span>
                        </div>
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
                // First token only, so the planka item stays visible while the admin
                // types its argument ("/set-calorie-limit 2600").
                let cmd_token = q.split_whitespace().next().unwrap_or("").to_string();
                let show_planka = "/set-calorie-limit".starts_with(&cmd_token)
                    || cmd_token.starts_with("/set-calorie-limit");
                // То же и для открытия темы: пункт остаётся видимым, пока админ
                // дописывает номер («/open-week 7»).
                let show_open_week = "/open-week".starts_with(&cmd_token)
                    || cmd_token.starts_with("/open-week");
                view! {
                    <div attr:data-testid="slash-menu"
                         style="position:sticky; bottom:0; margin:0 16px; background:var(--surface); \
                                border:1px solid var(--line); border-radius:var(--r); overflow:hidden; \
                                box-shadow:var(--shadow); z-index:25;">
                        {show_planka.then(|| view! {
                            <button attr:data-testid="slash-item"
                                style="display:flex; flex-direction:column; align-items:flex-start; gap:2px; \
                                       width:100%; text-align:left; padding:10px 14px; \
                                       border-bottom:1px solid var(--line-soft);"
                                // Prime the draft; the admin types the kcal and hits send.
                                on:click=move |_| draft.set("/set-calorie-limit ".to_string())>
                                <span style="font-weight:600;">"Установить планку калорий"</span>
                                <span class="mono row__meta">"/set-calorie-limit <ккал>"</span>
                            </button>
                        })}
                        {show_open_week.then(|| view! {
                            <button attr:data-testid="slash-item"
                                style="display:flex; flex-direction:column; align-items:flex-start; gap:2px; \
                                       width:100%; text-align:left; padding:10px 14px; \
                                       border-bottom:1px solid var(--line-soft);"
                                on:click=move |_| draft.set("/open-week ".to_string())>
                                <span style="font-weight:600;">"Открыть тему"</span>
                                <span class="mono row__meta">
                                    "/open-week <3 активность · 4 кальций · 5 железо · 6 жиры · 7 красное мясо>"
                                </span>
                            </button>
                        })}
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

            // Та же карточка, что открывается строкой в списке пользователей:
            // подписка, ключи, чеки, «Сбросить доступ (онбординг заново)» и
            // обнуление. Компонент один — расходиться этим двум местам нечем.
            {move || card_open.get().then(|| view! {
                <UserModal user_id=uid_card.get_value()
                    on_close=Callback::new(move |_| card_open.set(false)) />
            })}
        </div>
    }
}

/// Карточка пользователя с обнулением. Открывается по строке в списке
/// пользователей. Кнопка обнуления заряжается 10 быстрыми нажатиями подряд
/// (интервал < 500 мс) — случайно её не нажать; затем подтверждение.
#[component]
fn UserModal(user_id: String, on_close: Callback<()>) -> impl IntoView {
    let card = create_rw_signal(Option::<api::UserCard>::None);
    let error = create_rw_signal(Option::<String>::None);
    let loading = create_rw_signal(true);
    // Взвод кнопки: сколько нажатий подряд и когда было последнее (мс).
    let taps = create_rw_signal(0u32);
    let last_tap = create_rw_signal(0.0f64);
    let confirming = create_rw_signal(false);
    // Сброс доступа — отдельное, куда менее разрушительное действие: деньги и
    // личные данные остаются, поэтому взвода на 10 нажатий тут нет, только
    // подтверждение.
    let confirming_reset = create_rw_signal(false);
    let wiping = create_rw_signal(false);
    let report = create_rw_signal(Option::<api::WipeReport>::None);
    // Открытый чек — письмо от lava целиком.
    let receipt_open = create_rw_signal(Option::<api::ReceiptFull>::None);

    let uid_load = user_id.clone();
    spawn_local(async move {
        match api::user_card(&uid_load).await {
            Ok(c) => card.set(Some(c)),
            Err(e) => error.set(Some(e.message().to_string())),
        }
        loading.set(false);
    });

    const NEEDED: u32 = 10;
    const MAX_GAP_MS: f64 = 500.0;

    let on_arm = move |_| {
        let now = js_sys::Date::now();
        let prev = last_tap.get_untracked();
        let n = if prev > 0.0 && now - prev < MAX_GAP_MS { taps.get_untracked() + 1 } else { 1 };
        last_tap.set(now);
        taps.set(n);
        if n >= NEEDED {
            taps.set(0);
            last_tap.set(0.0);
            confirming.set(true);
        }
    };

    // store_value: замыкание подтверждения вызывается многократно (Fn), поэтому
    // id нельзя «съедать» перемещением.
    let uid_wipe = store_value(user_id.clone());
    let on_confirm = move |_| {
        let uid = uid_wipe.get_value();
        confirming.set(false);
        wiping.set(true);
        error.set(None);
        spawn_local(async move {
            match api::user_wipe(&uid).await {
                Ok(r) => {
                    if !r.ok {
                        // Частичный провал — показать целиком, не молчать.
                        error.set(Some(r.error.clone().unwrap_or_else(||
                            "обнуление прошло не полностью — см. шаги".to_string())));
                    }
                    report.set(Some(r));
                }
                Err(e) => error.set(Some(e.message().to_string())),
            }
            wiping.set(false);
        });
    };

    let uid_reset = store_value(user_id.clone());
    let on_reset = move |_| {
        let uid = uid_reset.get_value();
        confirming_reset.set(false);
        wiping.set(true);
        error.set(None);
        spawn_local(async move {
            match api::user_reset(&uid).await {
                Ok(r) => {
                    if !r.ok {
                        error.set(Some(r.error.clone().unwrap_or_else(||
                            "сброс прошёл не полностью — см. шаги".to_string())));
                    }
                    report.set(Some(r));
                }
                Err(e) => error.set(Some(e.message().to_string())),
            }
            wiping.set(false);
        });
    };

    let uid_title = user_id.clone();
    view! {
        <div on:click=move |_| on_close.call(())
             style="position:fixed; inset:0; background:rgba(0,0,0,0.55); z-index:70; \
                    display:flex; align-items:center; justify-content:center; padding:16px;">
            <div on:click=move |ev: leptos::ev::MouseEvent| ev.stop_propagation()
                 attr:data-testid="user-modal"
                 style="background:var(--surface); color:var(--text); max-width:560px; width:100%; \
                        max-height:86vh; overflow:auto; border-radius:12px; border:1px solid var(--line);">
                <div style="display:flex; justify-content:space-between; align-items:center; \
                            padding:12px 16px; border-bottom:1px solid var(--line); \
                            position:sticky; top:0; background:var(--surface);">
                    <b class="mono" style="font-size:0.85rem; word-break:break-all;">{uid_title}</b>
                    <button class="btn btn--ghost" on:click=move |_| on_close.call(())>"✕"</button>
                </div>

                <div style="padding:14px 16px;">
                    {move || error.get().map(|e| view! {
                        <div attr:data-testid="user-modal-error" class="banner">{e}</div>
                    })}
                    {move || loading.get().then(|| view! { <div class="row__meta">"Загружаем…"</div> })}

                    {move || card.get().map(|c| {
                        let auth = c.auth.clone();
                        let sub = c.subscription.clone();
                        let claims = c.claims.clone();
                        let receipts = c.receipts.clone();
                        view! {
                            {c.auth_error.clone().map(|e| view! {
                                <div class="banner">{format!("auth-worker недоступен: {e}")}</div>
                            })}

                            <div style="font-weight:650; margin:4px 0 6px;">"Аккаунт"</div>
                            <div class="row__meta">
                                {match auth.as_ref().and_then(|a| a.created_at) {
                                    Some(ms) => format!("создан {}", fmt_ts(ms)),
                                    None => "создан — нет данных".to_string(),
                                }}
                            </div>
                            {auth.as_ref().and_then(|a| a.identity.clone()).map(|i| view! {
                                <div class="row__meta">
                                    {format!("личность: {} {}{}", i.provider, i.provider_uid,
                                             i.username.map(|u| format!(" (@{u})")).unwrap_or_default())}
                                </div>
                            })}
                            {auth.as_ref().map(|a| a.has_phrase).unwrap_or(false).then(|| view! {
                                <div class="row__meta">"есть парольная фраза"</div>
                            })}

                            <div style="font-weight:650; margin:12px 0 6px;">
                                {format!("Ключи · {}", auth.as_ref().map(|a| a.passkeys.len()).unwrap_or(0))}
                            </div>
                            {auth.as_ref().map(|a| a.passkeys.clone()).unwrap_or_default().into_iter()
                                .map(|k| view! {
                                    <div class="row__meta">
                                        {format!("{} · создан {} · использован {}",
                                                 k.name.unwrap_or_else(|| "без имени".into()),
                                                 fmt_ts(k.created_at), fmt_ts(k.last_used_at))}
                                    </div>
                                }).collect_view()}

                            <div style="font-weight:650; margin:12px 0 6px;">
                                {format!("Токены · {}", auth.as_ref().map(|a| a.tokens.len()).unwrap_or(0))}
                            </div>
                            {auth.as_ref().map(|a| a.tokens.clone()).unwrap_or_default().into_iter()
                                .map(|t| view! {
                                    <div class="row__meta mono" style="font-size:0.72rem;">
                                        // created_at токенов — В СЕКУНДАХ.
                                        {format!("{}… · создан {} · использован {}",
                                                 t.token_id.chars().take(10).collect::<String>(),
                                                 fmt_ts(t.created_at * 1000), fmt_ts(t.last_used_at * 1000))}
                                    </div>
                                }).collect_view()}

                            <div style="font-weight:650; margin:12px 0 6px;">"Подписка"</div>
                            {sub.map(|s| view! {
                                <div class="row__meta">
                                    {format!("{}{} · до {}", s.status,
                                             if s.active { ", активна" } else { "" }, fmt_ts(s.end))}
                                </div>
                            })}

                            // Оплаченное и висящее разделено: по инвойсам без оплаты
                            // и аннулированным видно, что у человека пошло не так.
                            {
                                let paid: Vec<api::ClaimCard> = claims.iter()
                                    .filter(|c| c.status == "paid" || c.status == "claimed")
                                    .cloned().collect();
                                let invoices: Vec<api::ClaimCard> = claims.iter()
                                    .filter(|c| c.status != "paid" && c.status != "claimed")
                                    .cloned().collect();
                                view! {
                                    <div style="font-weight:650; margin:12px 0 6px;">
                                        {format!("Платежи · {}", paid.len())}
                                    </div>
                                    {if paid.is_empty() {
                                        view! { <div class="row__meta">"оплат нет"</div> }.into_view()
                                    } else {
                                        paid.into_iter().map(|c| view! {
                                            <div class="row__meta">
                                                {format!("{} · оплачен {}",
                                                    fmt_money(c.amount, c.currency.as_deref()),
                                                    c.paid_at.or(c.created_at).map(fmt_ts)
                                                        .unwrap_or_else(|| "—".into()))}
                                            </div>
                                        }).collect_view()
                                    }}

                                    <div style="font-weight:650; margin:12px 0 6px;">
                                        {format!("Инвойсы без оплаты · {}", invoices.len())}
                                    </div>
                                    {if invoices.is_empty() {
                                        view! { <div class="row__meta">"нет"</div> }.into_view()
                                    } else {
                                        invoices.into_iter().map(|c| view! {
                                            <div class="row__meta">
                                                {format!("{} · {} · выставлен {}",
                                                    match c.status.as_str() {
                                                        "pending" => "не оплачен",
                                                        "void" => "аннулирован",
                                                        other => other,
                                                    },
                                                    fmt_money(c.amount, c.currency.as_deref()),
                                                    c.created_at.map(fmt_ts).unwrap_or_else(|| "—".into()))}
                                            </div>
                                        }).collect_view()
                                    }}
                                }
                            }

                            <div style="font-weight:650; margin:12px 0 6px;">
                                {format!("Чеки · {}", receipts.len())}
                            </div>
                            // Строка чека ОТКРЫВАЕТСЯ: письмо от lava — это не только сумма.
                            // Про сорванное продление и отмену подписки провайдер сообщает
                            // только письмом, и разобрать, что там написано, нужно уметь
                            // прямо отсюда. Кнопка, а не <a>: на iOS клик по ссылке без href
                            // не доходит (см. reference_ios_leptos_click_delegation).
                            {if receipts.is_empty() {
                                view! { <div class="row__meta">"нет"</div> }.into_view()
                            } else {
                                receipts.into_iter().map(|r| {
                                    let id = r.id.clone();
                                    let open = move |_| {
                                        let id = id.clone();
                                        spawn_local(async move {
                                            match api::receipt_detail(&id).await {
                                                Ok(Some(full)) => receipt_open.set(Some(full)),
                                                Ok(None) => error.set(Some("чек не найден".into())),
                                                Err(e) => error.set(Some(e.message().to_string())),
                                            }
                                        });
                                    };
                                    view! {
                                        <button attr:data-testid="user-receipt-row" class="row__meta"
                                                style="display:block; width:100%; text-align:left; \
                                                       background:none; border:none; padding:2px 0; \
                                                       color:inherit; cursor:pointer; text-decoration:underline dotted;"
                                                on:click=open>
                                            {format!("{} · получен {}",
                                                fmt_money(r.amount, r.currency.as_deref()),
                                                r.received_at.map(fmt_ts).unwrap_or_else(|| "—".into()))}
                                        </button>
                                    }
                                }).collect_view()
                            }}
                        }
                    })}

                    // ── Обнуление ──
                    <div style="margin-top:18px; padding-top:14px; border-top:1px solid var(--line);">
                        {move || report.get().map(|r| view! {
                            <div attr:data-testid="wipe-report" style="margin-bottom:10px;">
                                {r.steps.into_iter().map(|st| view! {
                                    <div class="row__meta">
                                        {format!("{} {}{}", if st.ok { "✓" } else { "✗" }, st.step,
                                                 st.error.map(|e| format!(" — {e}")).unwrap_or_default())}
                                    </div>
                                }).collect_view()}
                            </div>
                        })}
                        <button
                            attr:data-testid="user-reset"
                            class="btn btn--ghost"
                            style="width:100%; margin-bottom:8px;"
                            disabled=move || wiping.get()
                            on:click=move |_| confirming_reset.set(true)
                        >
                            "Сбросить доступ (онбординг заново)"
                        </button>
                        <button
                            attr:data-testid="user-wipe-arm"
                            class="btn btn--ghost"
                            style="width:100%; color:var(--danger, #e0304f);"
                            disabled=move || wiping.get()
                            on:click=on_arm
                        >
                            {move || if wiping.get() {
                                "Обнуляем…".to_string()
                            } else if taps.get() > 0 {
                                format!("Обнулить пользователя ({}/{})", taps.get(), NEEDED)
                            } else {
                                "Обнулить пользователя".to_string()
                            }}
                        </button>
                    </div>
                </div>
            </div>

            // Подтверждение сброса доступа.
            {move || confirming_reset.get().then(|| view! {
                <div on:click=move |ev: leptos::ev::MouseEvent| ev.stop_propagation()
                     attr:data-testid="reset-confirm"
                     style="position:fixed; inset:0; background:rgba(0,0,0,0.7); z-index:80; \
                            display:flex; align-items:center; justify-content:center; padding:16px;">
                    <div style="background:var(--surface); color:var(--text); max-width:440px; width:100%; \
                                border-radius:12px; border:1px solid var(--line); padding:16px;">
                        <div style="font-weight:700; margin-bottom:8px;">"Сбросить доступ?"</div>
                        <div class="row__meta" style="line-height:1.5;">
                            "У пользователя будут сняты ключи, токены, выданные коды и отметки \
                             открытых глав — он вернётся в состояние сразу после оплаты, и в \
                             мини-аппе снова появится «Получить доступ к re:Norma», чтобы пройти \
                             онбординг заново. Платежи, подписка и чеки останутся, как и все его \
                             личные данные: дневник, прогресс историй, переписка."
                        </div>
                        <div style="display:flex; gap:8px; margin-top:14px;">
                            <button class="btn btn--ghost" style="flex:1;"
                                    on:click=move |_| confirming_reset.set(false)>"Отмена"</button>
                            <button attr:data-testid="reset-confirm-yes" class="btn" style="flex:1;"
                                    on:click=on_reset>"Да, сбросить"</button>
                        </div>
                    </div>
                </div>
            })}

            // Подтверждение — отдельным слоем поверх карточки.
            {move || confirming.get().then(|| view! {
                <div on:click=move |ev: leptos::ev::MouseEvent| ev.stop_propagation()
                     attr:data-testid="wipe-confirm"
                     style="position:fixed; inset:0; background:rgba(0,0,0,0.7); z-index:80; \
                            display:flex; align-items:center; justify-content:center; padding:16px;">
                    <div style="background:var(--surface); color:var(--text); max-width:440px; width:100%; \
                                border-radius:12px; border:1px solid var(--line); padding:16px;">
                        <div style="font-weight:700; margin-bottom:8px;">"Обнулить пользователя?"</div>
                        <div class="row__meta" style="line-height:1.5;">
                            "Этот пользователь будет удалён: аккаунт, ключи и токены, дневник, \
                             переписка с поддержкой, уведомления, платежи и чеки. Его подписка \
                             будет отменена, чтобы больше не списывалась. Так, как будто этого \
                             пользователя никогда не существовало. Деньги за уже прошедший \
                             платёж не возвращаются. Вы уверены, что хотите сделать так?"
                        </div>
                        <div style="display:flex; gap:8px; margin-top:14px;">
                            <button class="btn btn--ghost" style="flex:1;"
                                    on:click=move |_| confirming.set(false)>"Отмена"</button>
                            <button attr:data-testid="wipe-confirm-yes" class="btn" style="flex:1;"
                                    on:click=on_confirm>"Да, обнулить"</button>
                        </div>
                    </div>
                </div>
            })}

            // Тело чека — то же окно, что и на экране платежей. Письмо приходит
            // размеченным, поэтому показывается как есть.
            {move || receipt_open.get().map(|full| {
                let body = full.body_text.clone().unwrap_or_default();
                let amount = fmt_money(full.amount, full.currency.as_deref());
                let when = full.received_at.map(fmt_ts).unwrap_or_default();
                view! {
                    <div on:click=move |_| receipt_open.set(None)
                         attr:data-testid="user-receipt-body"
                         style="position:fixed; inset:0; background:rgba(0,0,0,0.7); z-index:90; \
                                display:flex; align-items:center; justify-content:center; padding:16px;">
                        <div on:click=move |ev: leptos::ev::MouseEvent| ev.stop_propagation()
                             style="background:#fff; color:#111; max-width:660px; width:100%; \
                                    max-height:86vh; overflow:auto; border-radius:12px;">
                            <div style="display:flex; justify-content:space-between; align-items:center; \
                                        padding:12px 16px; border-bottom:1px solid #eee; \
                                        position:sticky; top:0; background:#fff;">
                                <div><b>"Чек"</b>" · "<span class="mono">{amount}</span>" · "{when}</div>
                                <button class="btn btn--ghost" on:click=move |_| receipt_open.set(None)>"✕"</button>
                            </div>
                            <div inner_html=body style="padding:12px 16px;"></div>
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
        "weight" => "дневник веса",
        "steps" => "дневник шагов",
        "system" => "данные об устройстве",
        "all" => "все данные",
        other => other,
    }
    .to_string()
}

/// ms-epoch → coarse "N назад" label for the payments worklist.
/// Absolute local date-time `DD.MM.YYYY HH:MM` — for fields that can be in the FUTURE
/// (e.g. a subscription's `period_end`), where the relative `since_label` is wrong.
/// Время сообщения под пузырём: часы и минуты, а для не-сегодняшнего ещё и дата.
///
/// `created_at` приходит строкой ISO из воркера; разбирает её сам браузер, он же
/// переводит в местный пояс. Непонятную строку показываем как есть, а не прячем:
/// пустое место на её месте читалось бы как «времени нет».
fn fmt_msg_time(iso: &str) -> String {
    let d = js_sys::Date::new(&wasm_bindgen::JsValue::from_str(iso));
    if d.get_time().is_nan() {
        return iso.to_string();
    }
    let two = |n: u32| if n < 10 { format!("0{n}") } else { n.to_string() };
    let hm = format!("{}:{}", two(d.get_hours()), two(d.get_minutes()));
    let now = js_sys::Date::new_0();
    let same_day = d.get_full_year() == now.get_full_year()
        && d.get_month() == now.get_month()
        && d.get_date() == now.get_date();
    if same_day {
        hm
    } else {
        format!("{}.{} {hm}", two(d.get_date()), two(d.get_month() + 1))
    }
}

fn fmt_ts(ms: i64) -> String {
    if ms <= 0 {
        return "—".to_string();
    }
    let d = js_sys::Date::new(&wasm_bindgen::JsValue::from_f64(ms as f64));
    let two = |n: u32| if n < 10 { format!("0{n}") } else { n.to_string() };
    format!(
        "{}.{}.{} {}:{}",
        two(d.get_date()),
        two(d.get_month() + 1),
        d.get_full_year(),
        two(d.get_hours()),
        two(d.get_minutes()),
    )
}

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
    let users = create_rw_signal(Vec::<api::UserRow>::new());
    // Открытая карточка пользователя (обнуление живёт внутри неё).
    let user_open = create_rw_signal(Option::<String>::None);
    let receipts = create_rw_signal(Vec::<api::Receipt>::new());
    // The receipt whose full body is open in the modal (fetched on demand).
    let selected = create_rw_signal(Option::<api::ReceiptFull>::None);
    // The unbound payment whose detail (+ cancel action) is open in the modal.
    let selected_payment = create_rw_signal(Option::<api::UnboundPayment>::None);
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
            // Уникальные пользователи. Ошибку показываем, а не проглатываем.
            match api::users().await {
                Ok(list) => users.set(list),
                Err(e) if e.is_auth() => {
                    auth::logout();
                    view.set(View::Login);
                }
                Err(e) => error.set(Some(format!("список пользователей: {}", e.message()))),
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
                <div class="appbar__title">"Пользователи"</div>
                <div class="appbar__sub">"по одному на человека · нажмите для карточки"</div>
            </div>
            <button class="btn btn--ghost btn--icon" attr:aria-label="Обновить" on:click=move |_| load.call(())>
                <svg viewBox="0 0 24 24"><path d="M21 12a9 9 0 1 1-2.6-6.4M21 4v5h-5"/></svg>
            </button>
        </header>

        <div class="screen">
            {move || error.get().map(|e| view! { <div class="banner">{e}</div> })}

            // Список ПОЛЬЗОВАТЕЛЕЙ: ровно одна строка на человека, сколько бы у
            // него ни было платежей и инвойсов. Что именно пошло не так — видно
            // в карточке по клику.
            {move || {
                let list = users.get();
                (!list.is_empty()).then(|| view! {
                    <div style="padding: 16px 16px 2px;">
                        <span class="badge">{format!("Пользователи · {}", list.len())}</span>
                    </div>
                    <div class="list">
                        {list.into_iter().enumerate().map(|(i, u)| {
                            let who = u.tg_username.clone().map(|n| format!("@{n}"))
                                .or_else(|| u.tg_user_id.map(|id| format!("tg:{id}")))
                                .or_else(|| u.email.clone())
                                .unwrap_or_else(|| short_uid(&u.user_id));
                            let uid = u.user_id.clone();
                            let when = u.last_at.map(since_label).unwrap_or_default();
                            let mut facts: Vec<String> = Vec::new();
                            if u.paid_count > 0 { facts.push(format!("оплат: {}", u.paid_count)); }
                            if u.pending_count > 0 { facts.push(format!("неоплаченных счетов: {}", u.pending_count)); }
                            if u.void_count > 0 { facts.push(format!("аннулировано: {}", u.void_count)); }
                            let facts = facts.join(" · ");
                            let no_key = u.has_credentials == Some(false);
                            let key_unknown = u.has_credentials.is_none();
                            view! {
                                <button attr:data-testid="user-row" class="row reveal"
                                     attr:data-user-id=uid.clone()
                                     style=format!("--i:{i}")
                                     on:click=move |_| user_open.set(Some(uid.clone()))>
                                    <div class="row__top">
                                        <span class="row__title">{who}</span>
                                        {no_key.then(|| view! {
                                            <span class="badge badge--danger">"нет ключа"</span>
                                        })}
                                        {key_unknown.then(|| view! {
                                            <span class="badge badge--warn badge--plain">"ключ: неизвестно"</span>
                                        })}
                                    </div>
                                    <div class="row__sub">{facts}</div>
                                    <div class="row__meta">{when}</div>
                                </button>
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

            // Чеки, которые НЕ удалось привязать к пользователю. Привязанные видны
            // в карточке своего пользователя — здесь они только дублировали бы его.
            {move || {
                let list: Vec<api::Receipt> = receipts.get().into_iter()
                    .filter(|r| r.user_id.is_none())
                    .collect();
                (!list.is_empty()).then(|| view! {
                    <div style="padding: 16px 16px 2px;">
                        <span class="badge">{format!("Чеки без пользователя · {}", list.len())}</span>
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
                                <button attr:data-testid="receipt-row" class="row reveal"
                                     style=format!("--i:{i}") on:click=open>
                                    <div class="row__top">
                                        <span class="row__title mono">{amount}</span>
                                        <span class="badge">"чек"</span>
                                    </div>
                                    <div class="row__sub">{who}</div>
                                    <div class="row__meta">{when}</div>
                                </button>
                            }
                        }).collect_view()}
                    </div>
                })
            }}

            // Платежи, которые прошли, но не привязаны НИ К КАКОМУ аккаунту:
            // человек заплатил и не завершил онбординг. Пользователя у них нет,
            // поэтому в списке выше их быть не может — но и потерять их нельзя.
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
                    <div style="padding: 16px 16px 2px;">
                        <span class="badge badge--warn badge--plain">
                            {format!("Платежи без пользователя · {}", list.len())}
                        </span>
                    </div>
                    <div class="list">
                        {list.into_iter().enumerate().map(|(i, p)| {
                            // Сумма в минорных единицах, как и везде: печатаем
                            // тем же форматтером, иначе 990 читается как «990 ₽».
                            let amount = fmt_money(p.amount, p.currency.as_deref());
                            let email = p.email.clone().unwrap_or_else(|| "—".to_string());
                            let contract = p.contract_id.clone().unwrap_or_else(|| "—".to_string());
                            let waited = p.paid_at.map(since_label).unwrap_or_default();
                            let has_wait = !waited.is_empty();
                            // iOS click-delegation: a real <button>, never a bare <a on:click>.
                            let row = p.clone();
                            view! {
                                <button attr:data-testid="payment-row" class="row reveal"
                                        style=format!("--i:{i}; width:100%; text-align:left;")
                                        on:click=move |_| selected_payment.set(Some(row.clone()))>
                                    <div class="row__top">
                                        <span class="row__title mono">{amount}</span>
                                        {has_wait.then(|| view! {
                                            <span class="badge badge--warn badge--plain">{waited.clone()}</span>
                                        })}
                                    </div>
                                    <div class="row__sub">{email}</div>
                                    <div class="row__meta">"lava: "<span class="mono">{contract}</span></div>
                                </button>
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

            // Unbound-payment detail + «отменить подписку» (renewal only, NO refund).
            {move || selected_payment.get().map(|p| {
                let amount = match (p.amount, p.currency.clone()) {
                    (Some(a), Some(c)) => format!("{a} {c}"),
                    (Some(a), None) => a.to_string(),
                    _ => "—".to_string(),
                };
                let email = p.email.clone().unwrap_or_else(|| "—".to_string());
                let contract = p.contract_id.clone().unwrap_or_else(|| "—".to_string());
                let paid = p.paid_at.map(fmt_ts).unwrap_or_else(|| "—".to_string());
                let until = p.period_end.map(fmt_ts).unwrap_or_else(|| "—".to_string());
                let contract_for_cancel = p.contract_id.clone().unwrap_or_default();
                let email_for_cancel = p.email.clone().unwrap_or_default();
                let cancelling = create_rw_signal(false);
                let on_cancel = move |_| {
                    let cid = contract_for_cancel.clone();
                    let em = email_for_cancel.clone();
                    if cid.is_empty() {
                        error.set(Some("нет contractId — нечего отменять".to_string()));
                        return;
                    }
                    cancelling.set(true);
                    spawn_local(async move {
                        match api::cancel_subscription(&cid, &em).await {
                            Ok(()) => {
                                selected_payment.set(None);
                                error.set(None);
                                load.call(());
                            }
                            Err(e) if e.is_auth() => {
                                auth::logout();
                                view.set(View::Login);
                            }
                            Err(e) => {
                                cancelling.set(false);
                                error.set(Some(e.message().to_string()));
                            }
                        }
                    });
                };
                view! {
                    <div on:click=move |_| selected_payment.set(None)
                         style="position:fixed; inset:0; background:rgba(0,0,0,0.55); z-index:60; \
                                display:flex; align-items:center; justify-content:center; padding:16px;">
                        <div on:click=move |ev: leptos::ev::MouseEvent| ev.stop_propagation()
                             style="background:#fff; color:#111; max-width:520px; width:100%; \
                                    max-height:86vh; overflow:auto; border-radius:12px;">
                            <div style="display:flex; justify-content:space-between; align-items:center; \
                                        padding:12px 16px; border-bottom:1px solid #eee;">
                                <div><b>"Платёж"</b>" · "<span class="mono">{amount}</span></div>
                                <button class="btn btn--ghost" on:click=move |_| selected_payment.set(None)>"✕"</button>
                            </div>
                            <div style="padding:12px 16px; display:flex; flex-direction:column; gap:8px;">
                                <div class="row__sub">{email}</div>
                                <div class="row__meta">"lava: "<span class="mono">{contract}</span></div>
                                <div class="row__meta">{format!("Оплачен: {paid}")}</div>
                                <div class="row__meta">{format!("Действует до: {until}")}</div>
                                <button class="btn btn--danger"
                                        attr:data-testid="cancel-subscription"
                                        prop:disabled=move || cancelling.get()
                                        style="margin-top:8px;"
                                        on:click=on_cancel>
                                    {move || if cancelling.get() { "Отменяю…" } else { "Отменить подписку" }}
                                </button>
                                <div class="row__meta" style="color:#a00;">"Только остановит продление — без возврата денег."</div>
                            </div>
                        </div>
                    </div>
                }
            })}
        </div>

        // Карточка пользователя: открывается по строке списка.
        {move || user_open.get().map(|uid| view! {
            <UserModal user_id=uid on_close=Callback::new(move |_| user_open.set(None)) />
        })}

        <TabBar view=view active=Section::Payments/>
    }
}

/// Feature A: lava.top subscriptions/contracts NOT bound to any account in our DB.
/// Each row can be cancelled (stops renewal only — lava has NO refund API).
#[component]
fn Subscriptions(view: RwSignal<View>) -> impl IntoView {
    let items = create_rw_signal(Vec::<api::LavaSub>::new());
    let error = create_rw_signal(Option::<String>::None);
    let loading = create_rw_signal(true);

    let load = Callback::new(move |_: ()| {
        loading.set(true);
        spawn_local(async move {
            match api::lava_subscriptions().await {
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
            loading.set(false);
        });
    });
    load.call(());

    view! {
        <header class="appbar">
            <div class="ring"></div>
            <div style="flex: 1; min-width: 0;">
                <div class="appbar__title">"Подписки"</div>
                <div class="appbar__sub">"lava · активные, без привязки к аккаунту"</div>
            </div>
            <button class="btn btn--ghost btn--icon" attr:aria-label="Обновить" on:click=move |_| load.call(())>
                <svg viewBox="0 0 24 24"><path d="M21 12a9 9 0 1 1-2.6-6.4M21 4v5h-5"/></svg>
            </button>
        </header>

        <div class="screen">
            {move || error.get().map(|e| view! { <div class="banner">{e}</div> })}

            {move || {
                let list = items.get();
                if list.is_empty() {
                    if loading.get() {
                        return view! { <div class="spinner"></div> }.into_view();
                    }
                    return view! {
                        <div class="empty"><div class="empty__ring"></div>
                            <p>"Нет активных непривязанных подписок"</p></div>
                    }.into_view();
                }
                view! {
                    <div class="list">
                        {list.into_iter().enumerate().map(|(i, s)| {
                            let amount = match (s.amount, s.currency.clone()) {
                                (Some(a), Some(c)) => format!("{a} {c}"),
                                (Some(a), None) => a.to_string(),
                                _ => "—".to_string(),
                            };
                            let email = s.email.clone().unwrap_or_else(|| "—".to_string());
                            let status = s.status.clone().unwrap_or_else(|| "—".to_string());
                            let dt = s.datetime.clone().unwrap_or_default();
                            let contract = s.contract_id.clone();
                            let email_for_cancel = s.email.clone().unwrap_or_default();
                            let cancelling = create_rw_signal(false);
                            // Cancel failure is shown ON THIS ROW (not the global banner).
                            let row_error = create_rw_signal(None::<String>);
                            let on_cancel = move |_| {
                                let cid = contract.clone();
                                let em = email_for_cancel.clone();
                                cancelling.set(true);
                                row_error.set(None);
                                spawn_local(async move {
                                    match api::cancel_subscription(&cid, &em).await {
                                        // ONLY a confirmed success (worker 2xx after lava
                                        // accepted the cancel) removes the row from the list.
                                        Ok(()) => {
                                            items.update(|v| v.retain(|x| x.contract_id != cid));
                                        }
                                        Err(e) if e.is_auth() => {
                                            auth::logout();
                                            view.set(View::Login);
                                        }
                                        Err(e) => {
                                            cancelling.set(false);
                                            row_error.set(Some(e.message().to_string()));
                                        }
                                    }
                                });
                            };
                            view! {
                                <div attr:data-testid="subscription-row" class="row reveal" style=format!("--i:{i}")>
                                    <div class="row__top">
                                        <span class="row__title mono">{amount}</span>
                                        <span class="badge badge--plain">{status}</span>
                                    </div>
                                    <div class="row__sub">{email}</div>
                                    <div class="row__meta">"lava: "<span class="mono">{s.contract_id.clone()}</span></div>
                                    {(!dt.is_empty()).then(|| view! { <div class="row__meta">{dt.clone()}</div> })}
                                    {s.next_charge_at.clone().map(|n| view! {
                                        <div class="row__meta">{format!("Следующее списание: {n}")}</div>
                                    })}
                                    <button class="btn btn--danger"
                                            attr:data-testid="subscription-cancel"
                                            prop:disabled=move || cancelling.get()
                                            style="margin-top:8px;"
                                            on:click=on_cancel>
                                        {move || if cancelling.get() {
                                            view! { <span class="spinner spinner--btn"></span> }.into_view()
                                        } else {
                                            "Отменить".into_view()
                                        }}
                                    </button>
                                    {move || row_error.get().map(|e| view! {
                                        <div attr:data-testid="subscription-cancel-error"
                                             class="row__meta" style="color:#a00; margin-top:6px; word-break: break-all;">{e}</div>
                                    })}
                                </div>
                            }
                        }).collect_view()}
                    </div>
                    <div class="pad row__meta" style="color:#a00;">
                        "Отмена останавливает только продление — без возврата денег."
                    </div>
                }.into_view()
            }}
        </div>

        <TabBar view=view active=Section::Subscriptions/>
    }
}

/// A short user-id label for a bar (first / last chars, to keep bars readable).
fn short_uid(uid: &str) -> String {
    let chars: Vec<char> = uid.chars().collect();
    if chars.len() <= 10 {
        uid.to_string()
    } else {
        let head: String = chars[..6].iter().collect();
        let tail: String = chars[chars.len() - 3..].iter().collect();
        format!("{head}…{tail}")
    }
}

/// USD for a real neuron count at the given $/1000-neurons tariff.
fn usd_of(neurons: f64, price_per_1k: f64) -> f64 {
    neurons / 1000.0 * price_per_1k
}

/// Adaptive USD formatting — test amounts are tiny, so keep precision when small.
fn fmt_usd(usd: f64) -> String {
    if usd <= 0.0 {
        "$0".to_string()
    } else if usd >= 1.0 {
        format!("${usd:.2}")
    } else if usd >= 0.01 {
        format!("${usd:.3}")
    } else {
        format!("${usd:.5}")
    }
}

/// Inline SVG BAR HISTOGRAM: one bar per user, height ∝ NEURONS, labelled with the
/// COST (₽/$ tariff). DESC by neurons (as the API returns). Vision has no neurons,
/// so this is the Cloudflare-billable spend per user this week.
fn usage_histogram(users: &[api::UserUsage], price_per_1k: f64) -> leptos::View {
    if users.is_empty() {
        return view! { <div class="row__meta">"Нет данных для графика"</div> }.into_view();
    }
    const MAX_BARS: usize = 40;
    let shown: Vec<api::UserUsage> = users.iter().take(MAX_BARS).cloned().collect();
    let n = shown.len();
    let max_neurons = shown.iter().map(|u| u.neurons()).fold(0.0_f64, f64::max).max(1.0);

    let (w, h) = (600.0_f64, 240.0_f64);
    let (pad_l, pad_r, pad_t, pad_b) = (6.0_f64, 6.0_f64, 22.0_f64, 42.0_f64);
    let plot_w = w - pad_l - pad_r;
    let plot_h = h - pad_t - pad_b;
    let slot = plot_w / n as f64;
    let bar_w = (slot * 0.66).min(46.0);

    let bars = shown
        .iter()
        .enumerate()
        .map(|(i, u)| {
            let cx = pad_l + slot * (i as f64 + 0.5);
            let bh = (u.neurons() / max_neurons) * plot_h;
            let x = cx - bar_w / 2.0;
            let y = pad_t + (plot_h - bh);
            let cost = fmt_usd(usd_of(u.neurons(), price_per_1k));
            let uid = short_uid(&u.user_id);
            view! {
                <g>
                    <rect x=format!("{x:.1}") y=format!("{y:.1}")
                          width=format!("{bar_w:.1}") height=format!("{:.1}", bh.max(0.0))
                          rx="3" fill="var(--accent)"/>
                    <text x=format!("{cx:.1}") y=format!("{:.1}", y - 5.0)
                          text-anchor="middle" font-size="10" fill="var(--text)"
                          font-weight="600">{cost}</text>
                    <text x=format!("{cx:.1}") y=format!("{:.1}", h - pad_b + 12.0)
                          text-anchor="end" font-size="10" fill="var(--muted)"
                          transform=format!("rotate(-35 {cx:.1} {:.1})", h - pad_b + 12.0)>
                        {uid}
                    </text>
                </g>
            }
        })
        .collect_view();

    let baseline_y = pad_t + plot_h;
    view! {
        <svg viewBox=format!("0 0 {w} {h}")
             style="width:100%; height:240px; display:block; background:var(--surface-2); border-radius:10px;">
            <line x1=format!("{pad_l:.1}") y1=format!("{baseline_y:.1}")
                  x2=format!("{:.1}", w - pad_r) y2=format!("{baseline_y:.1}")
                  stroke="var(--line)" stroke-width="1"/>
            {bars}
        </svg>
        {(users.len() > MAX_BARS).then(|| view! {
            <div class="row__meta" style="margin-top:6px;">
                {format!("показаны топ-{MAX_BARS} из {} пользователей", users.len())}
            </div>
        })}
    }
    .into_view()
}

/// Long-term "average week": per-week total cost bars from the weekly rollup table,
/// plus the mean weekly cost across all stored weeks.
fn usage_weekly(weekly: &[api::WeeklyUsage], price_per_1k: f64) -> leptos::View {
    if weekly.is_empty() {
        return view! { <div class="row__meta">"Недельная агрегация появится после первого воскресенья"</div> }
            .into_view();
    }
    // Sum neurons per week_start (rows are per user).
    let mut weeks: Vec<(String, f64)> = Vec::new();
    for r in weekly {
        match weeks.iter_mut().find(|(w, _)| *w == r.week_start) {
            Some((_, n)) => *n += r.neurons(),
            None => weeks.push((r.week_start.clone(), r.neurons())),
        }
    }
    let avg_usd = usd_of(weeks.iter().map(|(_, n)| *n).sum::<f64>() / weeks.len() as f64, price_per_1k);
    let max_n = weeks.iter().map(|(_, n)| *n).fold(0.0_f64, f64::max).max(1.0);
    view! {
        <div class="row__meta" style="margin-bottom:8px;">
            "Средняя неделя: "<b style="color:var(--text);">{fmt_usd(avg_usd)}</b>
            {format!(" (по {} нед.)", weeks.len())}
        </div>
        <div style="display:flex; flex-direction:column; gap:6px;">
            {weeks.into_iter().map(|(week, neurons)| {
                let pct = (neurons / max_n * 100.0).clamp(0.0, 100.0);
                let cost = fmt_usd(usd_of(neurons, price_per_1k));
                view! {
                    <div style="display:flex; align-items:center; gap:10px;">
                        <span class="mono" style="width:92px; flex:none; color:var(--muted); font-size:.82rem;">
                            {week}
                        </span>
                        <div style="flex:1; height:14px; background:var(--surface-2); border-radius:7px; overflow:hidden;">
                            <div style=format!("height:100%; width:{pct:.1}%; background:var(--accent); border-radius:7px;")></div>
                        </div>
                        <span class="mono" style="width:72px; flex:none; text-align:right; font-weight:600;">
                            {cost}
                        </span>
                    </div>
                }
            }).collect_view()}
        </div>
    }
    .into_view()
}

/// РАСХОД ПО МОДЕЛЯМ — токены и деньги за всё, что хранится.
///
/// Здесь и живёт ответ на вопрос «сколько мы тратим у провайдера»: у стороннего
/// счёт идёт по токенам конкретной модели, и нейроны Cloudflare к нему отношения не
/// имеют. Модель без тарифа честно помечается «тариф не задан» — ноль в этом месте
/// читался бы как «бесплатно».
fn usage_by_model(rows: &[api::ModelUsage]) -> leptos::View {
    if rows.is_empty() {
        return view! { <div class="row__meta">"Расход по моделям появится после первых запросов"</div> }
            .into_view();
    }
    let fmt_tokens = |n: i64| {
        if n >= 1_000_000 {
            format!("{:.2}M", n as f64 / 1e6)
        } else if n >= 1_000 {
            format!("{:.1}k", n as f64 / 1e3)
        } else {
            n.to_string()
        }
    };
    view! {
        <div style="display:flex; flex-direction:column; gap:6px;">
            {rows.iter().map(|r| {
                // Пустое имя — строки, накопленные до того, как модель стали писать.
                let name = if r.model.is_empty() {
                    format!("{} (без модели)", r.source)
                } else {
                    r.model.clone()
                };
                let mut tokens = format!("{} ↓ · {} ↑", fmt_tokens(r.in_tokens), fmt_tokens(r.out_tokens));
                if r.neurons > 0.0 {
                    tokens.push_str(&format!(" · {:.0} нейронов", r.neurons));
                }
                let cost = match r.usd {
                    Some(u) => fmt_usd(u),
                    None => "тариф не задан".to_string(),
                };
                let cost_color = if r.usd.is_some() { "var(--text)" } else { "var(--muted)" };
                // Две строки, а не три колонки: на 430 px имя модели, токены и цена
                // в один ряд не влезают — цена уезжала за край экрана.
                view! {
                    <div style="display:flex; flex-direction:column; gap:2px; padding:6px 0; \
                                border-bottom:1px solid var(--line);">
                        <div style="display:flex; align-items:baseline; gap:8px;">
                            <span style="flex:1; min-width:0; overflow:hidden; text-overflow:ellipsis; \
                                         white-space:nowrap;">
                                {name}
                            </span>
                            <span class="mono" style=format!("flex:none; font-weight:600; \
                                                              color:{cost_color}; font-size:.82rem;")>
                                {cost}
                            </span>
                        </div>
                        <span class="mono" style="color:var(--muted); font-size:.78rem;">{tokens}</span>
                    </div>
                }
            }).collect_view()}
        </div>
    }
    .into_view()
}

/// Token-usage view: fetches /admin/usage on mount (+ refresh button) and renders
/// a headline (total / users / average), a per-user bar histogram, and per-day totals.
#[component]
fn Usage(view: RwSignal<View>) -> impl IntoView {
    let report = create_rw_signal(Option::<api::UsageReport>::None);
    let error = create_rw_signal(Option::<String>::None);
    let loading = create_rw_signal(true);

    let load = Callback::new(move |_: ()| {
        loading.set(true);
        spawn_local(async move {
            match api::admin_usage().await {
                Ok(r) => {
                    report.set(Some(r));
                    error.set(None);
                }
                Err(e) if e.is_auth() => {
                    auth::logout();
                    view.set(View::Login);
                }
                Err(e) => error.set(Some(e.message().to_string())),
            }
            loading.set(false);
        });
    });
    load.call(());

    view! {
        <header class="appbar">
            <div class="ring"></div>
            <div style="flex: 1; min-width: 0;">
                <div class="appbar__title">"Нейроны"</div>
                <div class="appbar__sub">"расход ИИ по пользователям и стоимость"</div>
            </div>
            <button class="btn btn--ghost btn--icon" attr:aria-label="Обновить" on:click=move |_| load.call(())>
                <svg viewBox="0 0 24 24"><path d="M21 12a9 9 0 1 1-2.6-6.4M21 4v5h-5"/></svg>
            </button>
        </header>

        <div class="screen">
            {move || error.get().map(|e| view! { <div class="banner">{e}</div> })}

            {move || {
                let Some(r) = report.get() else {
                    if loading.get() {
                        return view! { <div class="spinner"></div> }.into_view();
                    }
                    return ().into_view();
                };

                if r.week.is_empty() && r.weekly.is_empty() && r.by_model.is_empty() {
                    return view! {
                        <div class="empty"><div class="empty__ring"></div><p>"Пока нет данных"</p></div>
                    }.into_view();
                }

                let price = r.price_usd_per_1k_neurons;
                let user_count = r.week.len();
                let total_neurons: f64 = r.week.iter().map(|u| u.neurons()).sum();
                let total_usd = usd_of(total_neurons, price);
                let avg_usd = if user_count > 0 { total_usd / user_count as f64 } else { 0.0 };
                let hist = usage_histogram(&r.week, price);
                let weekly = usage_weekly(&r.weekly, price);
                let by_model = usage_by_model(&r.by_model);
                // Деньги СТОРОННЕГО провайдера — отдельной строкой: нейроны Workers AI
                // с ними не складываются, это разные счета.
                let thirdparty_usd: f64 = r.by_model.iter().filter_map(|m| m.usd).sum();
                let has_week = !r.week.is_empty();
                let week_start = r.week_start.clone();

                view! {
                    <div class="pad">
                        // Headline: this-week cost · users · average per user (this week).
                        <div style="display:flex; gap:10px; flex-wrap:wrap; margin-bottom:14px;">
                            <div style="flex:1; min-width:120px; padding:12px; background:var(--surface-2); \
                                        border:1px solid var(--line); border-radius:10px;">
                                <div class="row__meta">"Эта неделя, ₽/$"</div>
                                <div class="mono" style="font-size:1.25rem; font-weight:700;">
                                    {fmt_usd(total_usd)}
                                </div>
                            </div>
                            <div style="flex:1; min-width:100px; padding:12px; background:var(--surface-2); \
                                        border:1px solid var(--line); border-radius:10px;">
                                <div class="row__meta">"Пользователей"</div>
                                <div class="mono" style="font-size:1.25rem; font-weight:700;">
                                    {user_count.to_string()}
                                </div>
                            </div>
                            <div style="flex:1; min-width:120px; padding:12px; background:var(--surface-2); \
                                        border:1px solid var(--line); border-radius:10px;">
                                <div class="row__meta">"В среднем на пользователя"</div>
                                <div class="mono" style="font-size:1.25rem; font-weight:700;">
                                    {fmt_usd(avg_usd)}
                                </div>
                            </div>
                        </div>

                        <div class="row__meta" style="margin-bottom:12px;">
                            {format!("тариф ${price}/1000 нейронов · неделя с {week_start} · \
                                      всего {:.0} нейронов", total_neurons)}
                        </div>

                        // Per-user histogram (the "how much each tester eats" view).
                        {has_week.then(|| view! {
                            <div style="font-weight:650; margin:0 0 8px;">"По пользователям (эта неделя)"</div>
                            {hist.clone()}
                        })}

                        // Long-term weekly rollup ("average week").
                        <div style="font-weight:650; margin:18px 0 8px;">"По неделям"</div>
                        {weekly}

                        // Сторонние модели: токены и их собственные деньги.
                        <div style="font-weight:650; margin:18px 0 8px;">"По моделям (всё время)"</div>
                        <div class="row__meta" style="margin-bottom:8px;">
                            "У стороннего провайдера: "
                            <b style="color:var(--text);">{fmt_usd(thirdparty_usd)}</b>
                            " — счёт отдельный от нейронов Cloudflare"
                        </div>
                        {by_model}
                    </div>
                }.into_view()
            }}
        </div>

        <TabBar view=view active=Section::Usage/>
    }
}
