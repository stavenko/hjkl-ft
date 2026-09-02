//! Экраны приложения тренировок.
//!
//! Роутера нет — как в кураторском приложении и в админке: экранов мало, а
//! адресная строка здесь никому не нужна, приложение открывается с иконки.
//!
//! Порядок онбординга и причины, по которым он именно такой:
//!
//! 1. ТУПИКОВЫЙ БРАУЗЕР — раньше всего остального. В Яндекс.Браузере и Mi
//!    ключ не создать вовсе, и предлагать там вход значит вести человека в
//!    стену. Экран не закрывается: выхода из него, кроме перехода в Chrome, нет.
//! 2. ВХОД ключом. Главное действие — войти СУЩЕСТВУЮЩИМ (в проде это тот же
//!    ключ, что и на fit.renorma.app); завести новый спрятано за ссылкой.
//! 3. ПОДПИСКА. Проверяется живым запросом, если кэш её не подтверждает. Пока
//!    ответ не пришёл — спиннер, и ни в коем случае не экран «нужна подписка»:
//!    он мигнул бы у оплатившего человека. Сеть молчит — это «нет связи», а не
//!    «нет подписки».
//! 4. УСТАНОВКА на домашний экран — инструкция под конкретный браузер.
//! 5. Приложение: заглушка тренировок и нижнее меню.
//!
//! Меню — НИЖНЕЕ и с одной пока иконкой (настройки). Это не «кнопка, которую
//! потом переделаем в меню»: разделы у приложения будут (журнал подходов,
//! справочник, программы), и появляться они должны рядом с настройками, а не
//! вместо них. Красная точка на иконке — единственный способ узнать про
//! обновление, не заходя в настройки.

use leptos::*;

use crate::i18n::{t, Lang};
use crate::{auth, i18n, install, platform, settings, subscription, update};

#[derive(Clone, Copy, PartialEq)]
pub enum View {
    /// Браузер, в котором приложению делать нечего. Не закрывается.
    DeadEnd,
    Login,
    /// Сессия есть, кэш подписку не подтверждает — спрашиваем живой статус.
    Checking,
    /// Спросили и получили «нет». Экран блокирующий: внутрь без подписки нельзя.
    Locked,
    /// Спросить не удалось — сети нет. Это НЕ «нет подписки»: у только что
    /// оплатившего человека связь почти всегда есть, и обвинять его в неоплате
    /// из-за упавшего вайфая нельзя.
    Offline,
    Install,
    /// Приложение. `Tab` — открытый раздел; их пока два, и один из них заглушка.
    Ready(Tab),
}

/// Разделы приложения. Пока только «Тренировки» (заглушка) и «Настройки»;
/// иконка в меню сейчас одна — настроечная.
#[derive(Clone, Copy, PartialEq)]
pub enum Tab {
    Workouts,
    Settings,
}

fn initial_view() -> View {
    if platform::is_dead_end_browser() {
        return View::DeadEnd;
    }
    if !auth::has_live_session() {
        return View::Login;
    }
    match subscription::cached() {
        Some(s) if s.active => after_subscription(),
        Some(_) => View::Locked,
        None => View::Checking,
    }
}

/// Куда идти, когда подписка подтверждена: доставить приложение на домашний
/// экран, если его там ещё нет, иначе — внутрь.
fn after_subscription() -> View {
    if platform::needs_install_screen() {
        View::Install
    } else {
        View::Ready(Tab::Workouts)
    }
}

#[component]
pub fn App() -> impl IntoView {
    let view = create_rw_signal(initial_view());

    // Обновление ДО приложения применяется САМО. Единственная кнопка «Обновить»
    // живёт в настройках, а из инструкции по установке и с экрана входа туда не
    // попасть — человек со старой сборкой в кэше застрял бы на ней навсегда
    // ровно там, где свежая нужнее всего. Терять на этих экранах нечего.
    create_effect(move |_| {
        let before_app = matches!(view.get(), View::Login | View::Install | View::DeadEnd);
        if before_app && update::available().get() {
            update::apply_before_app();
        }
    });

    // Живая проверка подписки. Идёт ОДИН раз на вход в состояние `Checking`;
    // `verifying` держит её от повторного запуска, пока ответ не пришёл.
    let verifying = create_rw_signal(false);
    create_effect(move |_| {
        if view.get() != View::Checking || verifying.get_untracked() {
            return;
        }
        verifying.set(true);
        spawn_local(async move {
            let r = subscription::status().await;
            verifying.set(false);
            match r {
                Ok(s) if s.active => view.set(after_subscription()),
                Ok(_) => view.set(View::Locked),
                // Токен не приняли — сессии больше нет. Показывать здесь «нет
                // связи» значит отправить человека чинить вайфай вместо того,
                // чтобы просто войти заново.
                Err(subscription::StatusError::Unauthorized) => {
                    auth::logout();
                    subscription::forget();
                    view.set(View::Login);
                }
                // Ответа нет или он невнятный — это про связь, а не про оплату.
                // Никогда не `Locked` без внятного «не активна».
                Err(e) => {
                    leptos::logging::warn!("проверка подписки не удалась: {e}");
                    view.set(View::Offline);
                }
            }
        });
    });

    view! {
        <div class="app">
            {move || match view.get() {
                View::DeadEnd => view! {
                    // Выхода отсюда нет: `on_dismiss` не зовётся — эти экраны
                    // кнопки «продолжить» не показывают вовсе.
                    <install::InstallScreen on_dismiss=Callback::new(|_| {}) />
                }.into_view(),

                View::Login => view! {
                    // Вошли — дальше решает подписка, а не мы. Кэш при входе
                    // стёрт (аккаунт мог смениться), поэтому статус спрашивается
                    // живым запросом, а не берётся с прошлого раза.
                    <Login on_done=Callback::new(move |_| view.set(View::Checking)) />
                }.into_view(),

                View::Checking => view! {
                    <div class="screen screen--center" attr:data-testid="app-checking">
                        <div class="center">
                            <div class="spinner"></div>
                            <p class="sub" style="text-align: center;">{move || t("checking.title")}</p>
                        </div>
                    </div>
                }.into_view(),

                View::Locked => view! { <Locked view=view /> }.into_view(),
                View::Offline => view! { <Offline view=view /> }.into_view(),

                View::Install => view! {
                    // «Продолжить в браузере» есть ТОЛЬКО на десктопе — там
                    // ставить телефонное приложение незачем. Возврат к
                    // инструкции («иконки так и нет») экран разбирает сам.
                    <install::InstallScreen on_dismiss=Callback::new(move |_| view.set(View::Ready(Tab::Workouts))) />
                }.into_view(),

                View::Ready(tab) => view! { <Shell tab=tab view=view /> }.into_view(),
            }}
        </div>
    }
}

/// Вход. Главное действие — ВОЙТИ существующим ключом: в проде это тот же
/// паскей, что и в приложении питания, и человек приходит сюда уже с ним.
///
/// Создание нового ключа спрятано за ссылкой намеренно. Поставь его кнопкой —
/// и пришедший из приложения питания заведёт вторым нажатием второй аккаунт:
/// пустой, без подписки, и с ним он упрётся в блокирующий экран, не поняв,
/// почему оплаченное не работает.
#[component]
fn Login(on_done: Callback<()>) -> impl IntoView {
    let busy = create_rw_signal(false);
    let error = create_rw_signal(None::<String>);
    let registering = create_rw_signal(false);
    let name = create_rw_signal(String::new());

    let sign_in = move |_| {
        if busy.get_untracked() {
            return;
        }
        busy.set(true);
        error.set(None);
        spawn_local(async move {
            match auth::authenticate().await {
                Ok(_) => {
                    // Аккаунт мог смениться — прошлый ответ о подписке к нему
                    // отношения не имеет.
                    subscription::forget();
                    on_done.call(());
                }
                Err(e) => {
                    error.set(Some(e));
                    busy.set(false);
                }
            }
        });
    };

    let create = move || {
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
                    subscription::forget();
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
    // отправляется, не убирая её.
    let submit = move |ev: leptos::ev::SubmitEvent| {
        ev.prevent_default();
        create();
    };

    view! {
        <div class="screen screen--center">
            <div class="center">
                <img src="/icons/icon-192.png" alt="" class="applogo" />
                <p class="h1">{move || t("login.title")}</p>
                <p class="sub">{move || t("login.sub")}</p>

                {move || error.get().map(|e| view! {
                    <div class="banner" attr:role="alert">{e}</div>
                })}

                {move || if registering.get() {
                    view! {
                        <form on:submit=submit>
                            <label class="label" attr:for="gym-name">{move || t("login.name")}</label>
                            <input class="field" attr:id="gym-name" attr:type="text"
                                attr:autocomplete="name" attr:autocapitalize="words"
                                attr:enterkeyhint="go" attr:spellcheck="false"
                                attr:data-testid="gym-name"
                                prop:value=move || name.get()
                                on:input=move |ev| name.set(event_target_value(&ev)) />
                            <p class="hint">{move || t("login.name_hint")}</p>
                            <button class="btn btn--primary btn--block" style="margin-top: 20px;"
                                attr:type="submit" prop:disabled=move || busy.get()
                                attr:data-testid="gym-register">
                                {move || t("login.create")}
                            </button>
                            <p class="alt">
                                <button class="linkbtn" attr:type="button"
                                    prop:disabled=move || busy.get()
                                    on:click=move |_| { error.set(None); registering.set(false); }>
                                    {move || t("login.back")}
                                </button>
                            </p>
                        </form>
                    }.into_view()
                } else {
                    view! {
                        <>
                            <button class="btn btn--primary btn--block"
                                prop:disabled=move || busy.get() on:click=sign_in
                                attr:data-testid="gym-login">
                                {move || if busy.get() { t("login.working") } else { t("login.enter") }}
                            </button>
                            <p class="alt">
                                {move || t("login.no_key")}
                                <button class="linkbtn" attr:type="button"
                                    prop:disabled=move || busy.get()
                                    on:click=move |_| { error.set(None); registering.set(true); }
                                    attr:data-testid="gym-go-register">
                                    {move || t("login.register")}
                                </button>
                            </p>
                        </>
                    }.into_view()
                }}

                <LangSwitch />
            </div>
        </div>
    }
}

/// Сессия есть, подписки нет. Экран блокирующий: внутрь неоплаченного не
/// пускаем. Купить отсюда нельзя намеренно — оплата живёт в приложении питания и
/// у бота, и второго кассового пути заводить не надо.
#[component]
fn Locked(view: RwSignal<View>) -> impl IntoView {
    view! {
        <div class="screen screen--center" attr:data-testid="app-locked">
            <div class="center">
                <img src="/icons/icon-192.png" alt="" class="applogo" />
                <p class="h1">{move || t("locked.title")}</p>
                <p class="sub">{move || t("locked.body")}</p>
                <button class="btn btn--block" attr:data-testid="locked-btn-relogin"
                    on:click=move |_| {
                        auth::logout();
                        subscription::forget();
                        view.set(View::Login);
                    }>
                    {move || t("locked.relogin")}
                </button>
            </div>
        </div>
    }
}

/// Проверить подписку не удалось. Это про связь: экран предлагает повторить, а
/// не объявляет человека неоплатившим.
#[component]
fn Offline(view: RwSignal<View>) -> impl IntoView {
    view! {
        <div class="screen screen--center" attr:data-testid="app-offline">
            <div class="center">
                <img src="/icons/icon-192.png" alt="" class="applogo" />
                <p class="h1">{move || t("offline.title")}</p>
                <p class="sub">{move || t("offline.body")}</p>
                <button class="btn btn--primary btn--block" attr:data-testid="offline-btn-retry"
                    on:click=move |_| view.set(View::Checking)>
                    {move || t("offline.retry")}
                </button>
            </div>
        </div>
    }
}

/// Оболочка приложения: раздел плюс нижнее меню.
#[component]
fn Shell(tab: Tab, view: RwSignal<View>) -> impl IntoView {
    view! {
        {match tab {
            Tab::Workouts => view! { <Stub /> }.into_view(),
            Tab::Settings => view! {
                <settings::Settings on_logout=Callback::new(move |_| view.set(View::Login)) />
            }.into_view(),
        }}
        <TabBar tab=tab view=view />
    }
}

/// Нижнее меню. Иконка пока одна — настроечная, и она РАБОТАЕТ КАК ПЕРЕКЛЮЧАТЕЛЬ:
/// нажал — настройки, нажал ещё — обратно к приложению. С одним пунктом иначе и
/// нельзя: обычная вкладка, будучи единственной, не оставила бы дороги назад.
/// Появятся разделы — станут обычными вкладками, а возврат перестанет быть нужен.
#[component]
fn TabBar(tab: Tab, view: RwSignal<View>) -> impl IntoView {
    let on_settings = tab == Tab::Settings;
    view! {
        <nav class="tabbar" attr:data-testid="tabbar">
            <button
                class=move || if on_settings { "tab tab--on" } else { "tab" }
                attr:data-testid="tab-settings"
                attr:aria-label=move || t("set.title")
                on:click=move |_| view.set(View::Ready(
                    if on_settings { Tab::Workouts } else { Tab::Settings }
                ))
            >
                <span class="tab__icon">
                    <svg viewBox="0 0 24 24" aria-hidden="true">
                        <circle cx="12" cy="12" r="3.2"/>
                        <path d="M19.4 13a1.7 1.7 0 0 0 .34 1.87l.06.06a2 2 0 1 1-2.83 2.83l-.06-.06a1.7 1.7 0 0 0-1.87-.34 1.7 1.7 0 0 0-1.03 1.56V21a2 2 0 1 1-4 0v-.09A1.7 1.7 0 0 0 8.9 19.3a1.7 1.7 0 0 0-1.87.34l-.06.06a2 2 0 1 1-2.83-2.83l.06-.06a1.7 1.7 0 0 0 .34-1.87 1.7 1.7 0 0 0-1.56-1.03H3a2 2 0 1 1 0-4h.09A1.7 1.7 0 0 0 4.7 8.9a1.7 1.7 0 0 0-.34-1.87l-.06-.06a2 2 0 1 1 2.83-2.83l.06.06a1.7 1.7 0 0 0 1.87.34H9.1A1.7 1.7 0 0 0 10.13 3V3a2 2 0 1 1 4 0v.09a1.7 1.7 0 0 0 1.03 1.56 1.7 1.7 0 0 0 1.87-.34l.06-.06a2 2 0 1 1 2.83 2.83l-.06.06a1.7 1.7 0 0 0-.34 1.87v.04a1.7 1.7 0 0 0 1.56 1.03H21a2 2 0 1 1 0 4h-.09a1.7 1.7 0 0 0-1.51 1.03z"/>
                    </svg>
                    // Красная точка — единственный способ узнать про обновление,
                    // не заходя в настройки. Сидит на иконке, а не на подписи:
                    // подпись может и не влезть, иконка есть всегда.
                    {move || update::available().get().then(|| view! {
                        <span class="tab__dot" attr:data-testid="tab-update-dot"></span>
                    })}
                </span>
                <span class="tab__label">{move || t("set.title")}</span>
            </button>
        </nav>
    }
}

/// Заглушка раздела тренировок. Онбординг пройден: ключ принят, подписка
/// подтверждена, приложение стоит на домашнем экране. Тренировок здесь ещё нет —
/// и экран говорит об этом прямо, а не притворяется пустым списком.
#[component]
fn Stub() -> impl IntoView {
    let user = auth::get_user_id().unwrap_or_default();
    // Показываем ХВОСТ идентификатора, а не его целиком: человеку нужно лишь
    // убедиться, что аккаунт тот самый, а не читать все тридцать шесть знаков.
    let short: String = user.chars().rev().take(6).collect::<Vec<_>>().into_iter().rev().collect();

    view! {
        <div class="screen screen--center" attr:data-testid="app-stub">
            <div class="center">
                <img src="/icons/icon-192.png" alt="" class="applogo" />
                <p class="h1">{move || t("stub.title")}</p>
                <p class="sub">{move || t("stub.body")}</p>

                <div class="card" style="text-align: left;">
                    <p class="label" style="margin-bottom: 4px;">{move || t("stub.signed_as")}</p>
                    <p class="mono" attr:data-testid="stub-user">{format!("…{short}")}</p>
                </div>
            </div>
        </div>
    }
}

/// Переключатель языка. Он здесь потому, что приложение ставится на домашний
/// экран раньше, чем в нём появляются настройки, — а человеку с нерусской
/// системой инструкция по установке нужна на его языке уже сейчас.
#[component]
fn LangSwitch() -> impl IntoView {
    view! {
        <div class="seg" style="margin-top: 26px;">
            <button class=move || if i18n::get() == Lang::Ru { "seg__btn seg__btn--on" } else { "seg__btn" }
                attr:data-testid="lang-ru"
                on:click=move |_| i18n::set(Lang::Ru)>"Рус"</button>
            <button class=move || if i18n::get() == Lang::En { "seg__btn seg__btn--on" } else { "seg__btn" }
                attr:data-testid="lang-en"
                on:click=move |_| i18n::set(Lang::En)>"Eng"</button>
        </div>
    }
}
