use leptos::*;

use crate::api::{self, ConversationSummary, Message};
use crate::auth;

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
                    let cls = if is_expert { "bubble bubble--me" } else { "bubble bubble--them" };
                    view! {
                        <div attr:data-testid="msg" attr:data-sender=m.sender.clone() class=cls>
                            {m.text}
                        </div>
                    }
                }).collect_view()}
            </div>

            <div class="composer">
                <textarea attr:data-testid="reply-input" class="field" rows="1"
                    style="flex: 1; resize: none; max-height: 120px;" placeholder="Ответ…"
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
        </div>
    }
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

/// Operator worklist: paid-but-unbound payments. The server reconciles this list
/// against lava on load — contracts lava reports refunded/cancelled (terminatedAt) are
/// auto-voided and drop off here, so this shows only still-active unbound payments.
/// (No manual "mark voided" button anymore.)
#[component]
fn Payments(view: RwSignal<View>) -> impl IntoView {
    let items = create_rw_signal(Vec::<api::UnboundPayment>::new());
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
        </div>

        <TabBar view=view active=Section::Payments/>
    }
}
