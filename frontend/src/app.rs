use leptos::*;
use leptos_router::*;

use crate::pages;
use crate::services::i18n::t;
use crate::services::{auth, net, platform, subscription, update};

/// Приложение вписано в КОЛОНКУ шириной 480 px на любом экране.
///
/// Отдельного «десктопного вида» у нас нет и заводить его незачем: продукт
/// телефонный, и на мониторе он показывает ровно то же самое, посередине. Число
/// взято по самому широкому ходовому телефону (Pro Max ≈ 430 px) плюс поля —
/// колонка не должна оказаться уже, чем на настоящем устройстве.
///
/// Те же 480 px повторены в `dashboard::GRID` (сторона ячейки) и в ширине нижнего
/// меню: CSS-переменную сюда не завести — стили у нас строками в разметке.
const SHELL_COLUMN: &str = "max-width: 480px; margin: 0 auto; padding: 0.75rem;";

#[derive(Clone, Copy, PartialEq)]
enum AppState {
    PwaPrompt,
    Auth,
    /// Session exists and the cache doesn't yet confirm an active sub — we are
    /// fetching the LIVE status. Shows a spinner, NEVER the "no subscription"
    /// screen, so that message can't flash before the request resolves.
    Checking,
    /// Session exists but there is NO active subscription (e.g. the user closed
    /// the PWA mid-claim, or a terminal claim failure left a registered account
    /// with no sub). We must NEVER drop such a user into the app (locked decision:
    /// "never drop the user into the app unsubscribed"). Only ever reached AFTER a
    /// completed status fetch. This is a blocking screen, not the app.
    Locked,
    /// First-ever launch (no cached subscription) with NO network: we can't fetch
    /// the config/status, so we can't verify the subscription — but that's a
    /// NETWORK problem, not a "no subscription" one (a just-paid user almost
    /// always has connectivity). Show a "connect to continue" screen with retry,
    /// NOT Locked. Auto-advances to verification once the server is reachable.
    OfflineNoVerify,
    Ready,
}

/// Does the session currently hold an active subscription? Reads the cached status
/// (refreshed on every successful fetch). A never-claimed account has no cache →
/// false. Used to gate app entry so a registered-but-unclaimed session is blocked.
fn has_active_sub() -> bool {
    subscription::cached().map(|s| s.active).unwrap_or(false)
}

/// True when the current URL is an onboarding page (`/onboard`, `/onboard-tg`). These pages
/// drive their OWN auth flow (code-verify → passkey → PWA install), so they must render with
/// no session instead of the Auth/PwaPrompt overlays dropping over them.
fn is_onboard_entry() -> bool {
    let Some(loc) = web_sys::window().map(|w| w.location()) else { return false };
    let path = loc.pathname().unwrap_or_default();
    path == "/onboard" || path == "/onboard-tg"
}

/// Приглашение куратора. Экран сам разбирается, есть ли сессия: без неё он
/// отправляет человека регистрироваться обычным путём. Оверлей входа здесь
/// показал бы форму, которой пришедший по ссылке не ждал, — и увёл бы его от
/// вопроса, на который он пришёл ответить.
fn is_curator_invite() -> bool {
    let Some(loc) = web_sys::window().map(|w| w.location()) else { return false };
    loc.pathname().unwrap_or_default() == "/curator"
}

fn initial_state() -> AppState {
    // Yandex Browser comes FIRST — before the onboarding bypass. The Telegram
    // link lands on `/onboard`, which is exactly where a Yandex user gets stuck
    // (no passkey there), so the «open it in Chrome» screen must cover that page
    // too.
    if platform::detect_platform_is_yandex() {
        return AppState::PwaPrompt;
    }
    // Mi Browser — по той же причине и на том же месте: ссылка из Telegram ведёт
    // на `/onboard`, а ключ там создать нечем.
    if platform::detect_platform_is_mi() {
        return AppState::PwaPrompt;
    }
    // Samsung Internet — там же и по тому же поводу: ссылка ведёт на `/onboard`,
    // а человека надо увести в Chrome ДО того, как он начнёт что-то делать здесь.
    if platform::detect_platform_is_samsung() {
        return AppState::PwaPrompt;
    }
    // Неопознанный браузер — тоже до онбординга: заводить аккаунт там, где мы не
    // ручаемся за работу, значит завести его наполовину.
    if platform::detect_platform_is_unknown() {
        return AppState::PwaPrompt;
    }
    // Onboarding drives its own flow; bypass all overlays so neither the Auth (login) nor the
    // PWA-install overlay covers it.
    if is_onboard_entry() || is_curator_invite() {
        return AppState::Ready;
    }
    if platform::needs_pwa_prompt() {
        AppState::PwaPrompt
    } else if !auth::session_valid_here() {
        // No usable session for this context. In the installed PWA a browser-onboarding token
        // does NOT count — the PWA requires its own login (passkey or Telegram code).
        AppState::Auth
    } else {
        // Offline-first subscription gate. Trust the cached status (stored in our
        // per-user DB) so a returning user enters INSTANTLY without a network wait;
        // a background daily re-check flips the gate if it changed.
        //   active cache   → Ready (enter now)
        //   inactive cache → Locked (completed check earlier said so)
        //   no cache yet   → Checking (first-ever launch: verify before deciding,
        //                    never flash "no subscription" from a cold cache)
        match subscription::cached() {
            Some(s) if s.active => AppState::Ready,
            Some(_) => AppState::Locked,
            None => AppState::Checking,
        }
    }
}

#[component]
pub fn App() -> impl IntoView {
    let state = create_rw_signal(initial_state());

    // Onboarding step transitions: auth → app. Purchase is no longer an onboarding
    // step — it happens on the landing before the app is ever opened, and binding
    // the paid sub happens in the dedicated `/onboard` claim flow. Enabling push is
    // no longer an onboarding screen either — the Story (chapter 1) walks the user
    // through it instead.
    let after_auth = move || {
        // Gate on the subscription. If the cache already confirms an active sub,
        // enter immediately; otherwise verify the LIVE status (spinner) before
        // deciding — never flash the "no subscription" screen from a cold cache.
        if has_active_sub() {
            state.set(AppState::Ready);
        } else {
            state.set(AppState::Checking);
        }
    };

    // Миграции локальной базы: работающее приложение и обновление не ждёт.
    //
    // До обновления нельзя — старый код накатил бы старые преобразования на данные,
    // которых новая сборка ждёт другими, и та застала бы базу в промежуточном виде.
    //
    // Онбординг НЕ исключаем. Отличить его надёжно нечем: `Ready` выдаётся и ему,
    // а путь `/onboard` роутер переписывает на `/` быстрее, чем сюда доходит дело.
    // Вместо запрета — учёт: при входе база меняется на пользовательскую (гостевая
    // не копируется, `migrate_bootstrap = false`), и миграции проходят там заново.
    // Отметку о прогоне держит `run_for_current_db` — она привязана к ИМЕНИ БАЗЫ, а
    // не к сессии, поэтому смена базы честно вызывает повторный проход.
    // До приложения обновление применяется САМО. В Настройки — единственное место
    // с кнопкой «Обновить» — с экрана установки или входа не попасть, поэтому
    // застрявший на старой сборке иначе не обновится никогда.
    create_effect(move |_| {
        let before_app = matches!(state.get(), AppState::PwaPrompt | AppState::Auth);
        if before_app && crate::services::update::available().get() {
            crate::services::update::apply_before_app();
        }
    });

    create_effect(move |_| {
        let ready = matches!(state.get(), AppState::Ready);
        let update_pending = crate::services::update::available().get();
        if !ready || update_pending {
            return;
        }
        leptos::spawn_local(async move {
            let done = crate::services::migrations::run_for_current_db().await;
            if done > 0 {
                // Стёртое надо запросить заново — проход идёт сразу после миграций,
                // а не ждёт следующего запуска.
                crate::services::classify::sweep_unprocessed().await;
            }
            // Картинки разобранных записей живут неделю (`local::IMAGE_KEEP_DAYS`).
            // Уборка идёт раз за запуск: снимки тяжёлые, а места на телефоне мало.
            crate::services::local::sweep_images().await;
            // Нераспознанные записи разбираются фоном. Проход сам молчит, если
            // возможность выключена или сети нет, — звать его можно смело.
            crate::services::lazy_food::run_queue().await;
        });
    });

    // React to a subscription-status change detected by the background daily
    // re-check (subscription::maybe_recheck rewrites the cache + gate). Only ever
    // moves between the two subscription-driven states — it never clobbers the
    // Auth / PwaPrompt / Checking overlays. Skips its first (seed) run.
    create_effect(move |prev: Option<bool>| {
        let active = subscription::gate_signal().get();
        if prev.is_some() {
            match state.get_untracked() {
                AppState::Ready if !active => state.set(AppState::Locked),
                AppState::Locked if active => state.set(AppState::Ready),
                _ => {}
            }
        }
        active
    });

    // First-ever verification (no cached subscription), driven REACTIVELY by the
    // connectivity probe instead of a blind wait:
    //   - probe unknown (None)   → keep the spinner (Checking)
    //   - server reachable (true)→ fetch the live status → Ready / Locked
    //   - server down (false)    → OfflineNoVerify: it's a NETWORK problem, not a
    //     "no subscription" one; the screen offers retry and this same effect
    //     re-verifies once the probe reports reachable again.
    // `verifying` guards against a second concurrent status fetch.
    let verifying = create_rw_signal(false);

    // Сеть появилась — разбираем то, что накопилось. Записи, сделанные без сети,
    // иначе ждали бы следующего запуска приложения.
    create_effect(move |_| {
        if matches!(net::is_online().get(), Some(true)) {
            crate::services::lazy_food::run_queue_background();
        }
    });

    create_effect(move |_| {
        let online = net::is_online().get();
        match state.get() {
            AppState::Checking => match online {
                Some(true) => {
                    if !verifying.get_untracked() {
                        verifying.set(true);
                        spawn_local(async move {
                            let r = subscription::status().await;
                            verifying.set(false);
                            match r {
                                Ok(s) if s.active => state.set(AppState::Ready),
                                Ok(_) => state.set(AppState::Locked),
                                // Reachable-but-errored (e.g. payment worker down):
                                // can't verify → treat as a connectivity problem,
                                // never Locked without a completed "not active".
                                Err(_) => state.set(AppState::OfflineNoVerify),
                            }
                        });
                    }
                }
                Some(false) => state.set(AppState::OfflineNoVerify),
                None => {} // probe in flight — keep the spinner
            },
            // Network came back → re-enter verification.
            AppState::OfflineNoVerify if online == Some(true) => state.set(AppState::Checking),
            _ => {}
        }
    });

    // Пользователь дошёл до работающего приложения — сообщаем это серверу. По
    // этому признаку мини-апп в Telegram показывает «Открыть приложение» вместо
    // «Получить доступ к re:Norma»; без отметки кнопка остаётся неверной навсегда.
    create_effect(move |_| {
        if state.get() == AppState::Ready && auth::session_valid_here() {
            spawn_local(async { auth::report_entered().await });
        }
    });

    // Yandex Browser: render ONLY the «open it in Chrome» screen — nothing else
    // must run. The router is otherwise mounted UNDER the overlay, and /onboard
    // would burn the one-time login code from the Telegram link right there,
    // invisibly, leaving the link stale by the time the user reaches Chrome.
    if platform::detect_platform_is_yandex() {
        return view! {
            <div style="position: fixed; inset: 0; z-index: 100; background: var(--bulma-scheme-main); overflow-y: auto;">
                <pages::pwa_prompt::PwaPrompt on_dismiss=Callback::new(|_| {}) />
            </div>
        }
        .into_view();
    }
    // Mi Browser — то же самое и по той же причине: `/onboard` под оверлеем сожгла
    // бы одноразовый код из телеграм-ссылки раньше, чем человек дойдёт до Chrome.
    if platform::detect_platform_is_mi() {
        return view! {
            <div style="position: fixed; inset: 0; z-index: 100; background: var(--bulma-scheme-main); overflow-y: auto;">
                <pages::pwa_prompt::PwaPrompt on_dismiss=Callback::new(|_| {}) />
            </div>
        }
        .into_view();
    }

    view! {
        // Blocking migration page: while the one-time sync v2 migration uploads
        // day-batches, the app is not usable (local mutations would race the
        // upload) — cover everything and show the batch progress.
        {move || crate::services::sync::migration_progress().get().map(|(done, total): (u32, u32)| view! {
            <div attr:data-testid="migration-overlay" style="position: fixed; inset: 0; z-index: 110; background: var(--bulma-scheme-main); display: flex; flex-direction: column; align-items: center; justify-content: center; padding: 2rem; text-align: center;">
                <div style="max-width: 24rem; width: 100%;">
                    <img src="/icon-192.png" alt="re:Norma" style="width: 80px; height: 80px; border-radius: 16px; margin-bottom: 1rem;" />
                    <h1 class="title is-5" style="margin-bottom: 0.5rem;">"Делаем миграцию данных"</h1>
                    <p class="has-text-grey mb-4">"Не закрывайте приложение — это займёт немного времени."</p>
                    <progress class="progress is-primary" value=done.to_string() max=total.max(1).to_string()></progress>
                    <p class="has-text-grey is-size-7">{format!("{done} из {total}")}</p>
                </div>
            </div>
        })}

        // Overlays
        {move || match state.get() {
            AppState::PwaPrompt => Some(view! {
                <div style="position: fixed; inset: 0; z-index: 100; background: var(--bulma-scheme-main); overflow-y: auto;">
                    <pages::pwa_prompt::PwaPrompt on_dismiss=Callback::new(move |_| {
                        if auth::session_valid_here() {
                            after_auth();
                        } else {
                            state.set(AppState::Auth);
                        }
                    }) />
                </div>
            }.into_view()),

            AppState::Auth => Some(view! {
                <div style="position: fixed; inset: 0; z-index: 100; background: var(--bulma-scheme-main);">
                    <pages::auth_page::AuthPage on_authenticated=Callback::new(move |_| {
                        after_auth();
                    }) />
                </div>
            }.into_view()),

            // Verifying the live subscription status — spinner only, so the
            // "no subscription" screen can't appear before the request resolves.
            AppState::Checking => Some(view! {
                <div attr:data-testid="app-checking" style="position: fixed; inset: 0; z-index: 100; background: var(--bulma-scheme-main); display: flex; align-items: center; justify-content: center;">
                    <div class="ft-spinner"></div>
                </div>
            }.into_view()),

            // Session but no active subscription → blocking screen. The app behind
            // it is NOT reachable (this overlay covers it). The user can log into a
            // different account; a fresh sub is bought on the landing, not here.
            AppState::Locked => Some(view! {
                <div attr:data-testid="app-locked" style="position: fixed; inset: 0; z-index: 100; background: var(--bulma-scheme-main); display: flex; flex-direction: column; align-items: center; justify-content: center; padding: 2rem; text-align: center;">
                    <div style="max-width: 24rem; width: 100%;">
                        <img src="/icon-192.png" alt="re:Norma" style="width: 80px; height: 80px; border-radius: 16px; margin-bottom: 1rem;" />
                        <h1 class="title is-5" style="margin-bottom: 0.5rem;">{move || t("locked.title")}</h1>
                        <p class="has-text-grey mb-5" style="line-height: 1.6;">{move || t("locked.body")}</p>
                        <button
                            attr:data-testid="locked-btn-login"
                            class="button is-light is-fullwidth"
                            style="text-decoration: underline;"
                            on:click=move |_| {
                                // Switch account: drop the session and show the login dialog.
                                auth::logout();
                                state.set(AppState::Auth);
                            }
                        >
                            {move || t("auth.login_title")}
                        </button>
                    </div>
                </div>
            }.into_view()),

            // First-ever launch with no network → can't verify. This is a NETWORK
            // problem (not "no subscription"): tell the user to connect, offer
            // retry. Auto-advances once the probe reports the server reachable.
            AppState::OfflineNoVerify => Some(view! {
                <div attr:data-testid="app-offline-gate" style="position: fixed; inset: 0; z-index: 100; background: var(--bulma-scheme-main); display: flex; flex-direction: column; align-items: center; justify-content: center; padding: 2rem; text-align: center;">
                    <div style="max-width: 24rem; width: 100%;">
                        <span style="color: #E0A100; display: inline-flex; margin-bottom: 1rem;">
                            <svg xmlns="http://www.w3.org/2000/svg" width="56" height="56" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">
                                <path d="M10.29 3.86 1.82 18a2 2 0 0 0 1.71 3h16.94a2 2 0 0 0 1.71-3L13.71 3.86a2 2 0 0 0-3.42 0z"/>
                                <line x1="12" y1="9" x2="12" y2="13"/>
                                <line x1="12" y1="17" x2="12.01" y2="17"/>
                            </svg>
                        </span>
                        <h1 class="title is-5" style="margin-bottom: 0.5rem;">{move || t("offline_gate.title")}</h1>
                        <p class="has-text-grey mb-5" style="line-height: 1.6;">{move || t("offline_gate.body")}</p>
                        <button
                            attr:data-testid="offline-gate-retry"
                            class="button is-link is-fullwidth"
                            on:click=move |_| net::probe_background()
                        >
                            {move || t("offline_gate.retry")}
                        </button>
                    </div>
                </div>
            }.into_view()),

            AppState::Ready => None,
        }}

        // Router always mounted
        <Router>
            // Pinned app-shell: the document itself never scrolls (position:
            // fixed, inset:0), so the fixed bottom nav can't float with iOS's
            // visual viewport after a resume (the "phantom keyboard" bug — the
            // nav slid up as if a keyboard were open). Only the inner container
            // scrolls; it opts into the resume scroll re-arm (data-ios-scroll).
            <div style="position: fixed; inset: 0; overflow: hidden; background: var(--bulma-background);">
                <div attr:data-ios-scroll="1"
                     style="position: absolute; inset: 0; overflow-y: auto; -webkit-overflow-scrolling: touch; padding-bottom: 4.5rem;">
                    // Приложение живёт КОЛОНКОЙ шириной с телефон, где бы его ни
                    // открыли. Растянутое на весь монитор, оно разъезжалось:
                    // квадратные ячейки сетки считаются от ширины окна и на 1440
                    // раздувались вчетверо (плитка 46 → 172 px), а шрифты, значки
                    // и нижнее меню заданы в пикселях и не росли — огромные пустые
                    // карточки с крошечными подписями, кнопки не попадали под
                    // палец. Одна колонка снимает это целиком: и вид, и пропорции
                    // те же самые, что на телефоне.
                    <div style=SHELL_COLUMN>
                    <Routes>
                        <Route path="/" view=DashboardGate />
                        // Согласие на куратора. Маршрут открытый: приглашение
                        // приходит ссылкой, и экран сам решает, вести ли человека
                        // на регистрацию или спрашивать согласие.
                        <Route path="/curator" view=pages::curator_invite::CuratorInvitePage />
                        <Route path="/help/food" view=pages::help::HelpFoodPage />
                        <Route path="/help/:id" view=pages::help::HelpArticlePage />
                        <Route path="/onboard" view=pages::onboard::OnboardPage />
                        <Route path="/onboard-tg" view=pages::onboard_tg::OnboardTgPage />
                        <Route path="/progress" view=pages::progress::ProgressPage />
                        <Route path="/diary" view=pages::diary::DiaryPage />
                        <Route path="/diary/add" view=pages::diary_add::DiaryAddPage />
                        <Route path="/foods" view=pages::foods::FoodsPage />
                        <Route path="/recipes" view=pages::recipes::RecipesPage />
                        <Route path="/recipes/:id" view=pages::recipe_detail::RecipeDetailPage />
                        <Route path="/recipes/:id/add" view=pages::recipe_add::RecipeAddPage />
                        <Route path="/settings" view=pages::settings::SettingsPage />
                        <Route path="/settings/privacy" view=pages::privacy::PrivacyPage />
                        <Route path="/settings/subscription" view=pages::subscription::SubscriptionPage />
                        <Route path="/settings/backup" view=pages::backup::BackupPage />
                        <Route path="/weight" view=pages::weight::WeightPage />
                        <Route path="/steps" view=pages::steps::StepsPage />
                        <Route path="/chat" view=pages::chat::ChatPage />
                    </Routes>
                    </div>
                </div>
            </div>

            // Hidden on /onboard: that pre-account page forces AppState::Ready to
            // bypass the auth overlays, which would otherwise surface this app-shell
            // nav before the user has registered.
            <nav style:display=move || { let p = use_location().pathname.get(); if p == "/onboard" || p == "/onboard-tg" { "none" } else { "flex" } } style="position: fixed; bottom: 0.75rem; left: 50%; transform: translateX(-50%); z-index: 40; background: var(--bulma-scheme-main); display: flex; justify-content: space-around; align-items: center; height: 3.5rem; width: min(26rem, calc(100% - 2rem), calc(480px - 2rem)); border-radius: 1rem; box-shadow: 0 4px 24px rgba(0,0,0,0.15);">
                // Пункт меню ведёт туда, где человек, возможно, уже стоит, и роутер
                // тогда не делает ничего. Поэтому он ещё и СОБЫТИЕ: дашборд по нему
                // закрывает раскрытую панель, дневник возвращается на сегодня.
                <a attr:data-testid="nav-dashboard" href="/"
                    on:click=|_| crate::services::nav::tap_home()
                    style="display: flex; flex-direction: column; align-items: center; justify-content: center; flex: 1; height: 100%; color: var(--bulma-text); text-decoration: none;">
                    <svg xmlns="http://www.w3.org/2000/svg" width="28" height="28" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">
                        <rect x="3" y="3" width="7" height="7" rx="1.5" />
                        <rect x="14" y="3" width="7" height="7" rx="1.5" />
                        <rect x="3" y="14" width="7" height="7" rx="1.5" />
                        <rect x="14" y="14" width="7" height="7" rx="1.5" />
                    </svg>
                    <span style="font-size: 0.6rem; margin-top: 2px;">{move || t("nav.dashboard")}</span>
                </a>
                <a attr:data-testid="nav-diary" href="/diary"
                    on:click=|_| crate::services::nav::tap_diary()
                    style="display: flex; flex-direction: column; align-items: center; justify-content: center; flex: 1; height: 100%; color: var(--bulma-text); text-decoration: none;">
                    <svg xmlns="http://www.w3.org/2000/svg" width="28" height="28" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">
                        <rect x="3" y="4" width="18" height="18" rx="2" />
                        <line x1="3" y1="10" x2="21" y2="10" />
                        <line x1="8" y1="2" x2="8" y2="6" />
                        <line x1="16" y1="2" x2="16" y2="6" />
                    </svg>
                    <span style="font-size: 0.6rem; margin-top: 2px;">{move || t("nav.diary")}</span>
                </a>
                <a attr:data-testid="nav-recipes" href="/recipes" style="display: flex; flex-direction: column; align-items: center; justify-content: center; flex: 1; height: 100%; color: var(--bulma-text); text-decoration: none;">
                    <svg xmlns="http://www.w3.org/2000/svg" width="28" height="28" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">
                        <path d="M14 2H6a2 2 0 0 0-2 2v16a2 2 0 0 0 2 2h12a2 2 0 0 0 2-2V8z" />
                        <polyline points="14 2 14 8 20 8" />
                        <line x1="16" y1="13" x2="8" y2="13" />
                        <line x1="16" y1="17" x2="8" y2="17" />
                        <polyline points="10 9 9 9 8 9" />
                    </svg>
                    <span style="font-size: 0.6rem; margin-top: 2px;">{move || t("nav.recipes")}</span>
                </a>
                <a attr:data-testid="nav-settings" href="/settings" style="display: flex; flex-direction: column; align-items: center; justify-content: center; flex: 1; height: 100%; color: var(--bulma-text); text-decoration: none;">
                    <span style="position: relative; display: inline-flex;">
                    <svg xmlns="http://www.w3.org/2000/svg" width="28" height="28" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">
                        <circle cx="12" cy="12" r="3" />
                        <path d="M19.4 15a1.65 1.65 0 0 0 .33 1.82l.06.06a2 2 0 0 1 0 2.83 2 2 0 0 1-2.83 0l-.06-.06a1.65 1.65 0 0 0-1.82-.33 1.65 1.65 0 0 0-1 1.51V21a2 2 0 0 1-2 2 2 2 0 0 1-2-2v-.09A1.65 1.65 0 0 0 9 19.4a1.65 1.65 0 0 0-1.82.33l-.06.06a2 2 0 0 1-2.83 0 2 2 0 0 1 0-2.83l.06-.06A1.65 1.65 0 0 0 4.68 15a1.65 1.65 0 0 0-1.51-1H3a2 2 0 0 1-2-2 2 2 0 0 1 2-2h.09A1.65 1.65 0 0 0 4.6 9a1.65 1.65 0 0 0-.33-1.82l-.06-.06a2 2 0 0 1 0-2.83 2 2 0 0 1 2.83 0l.06.06A1.65 1.65 0 0 0 9 4.68a1.65 1.65 0 0 0 1-1.51V3a2 2 0 0 1 2-2 2 2 0 0 1 2 2v.09a1.65 1.65 0 0 0 1 1.51 1.65 1.65 0 0 0 1.82-.33l.06-.06a2 2 0 0 1 2.83 0 2 2 0 0 1 0 2.83l-.06.06A1.65 1.65 0 0 0 19.4 9a1.65 1.65 0 0 0 1.51 1H21a2 2 0 0 1 2 2 2 2 0 0 1-2 2h-.09a1.65 1.65 0 0 0-1.51 1z" />
                    </svg>
                    {move || update::available().get().then(|| view! {
                        <span attr:data-testid="nav-settings-update-dot"
                            style="position: absolute; top: -1px; right: -2px; width: 9px; height: 9px; border-radius: 50%; background: var(--bulma-danger); border: 1.5px solid var(--bulma-scheme-main);"></span>
                    })}
                    </span>
                    <span style="font-size: 0.6rem; margin-top: 2px;">{move || t("nav.settings")}</span>
                </a>
                <a attr:data-testid="nav-support" href="/chat" style="display: flex; flex-direction: column; align-items: center; justify-content: center; flex: 1; height: 100%; color: var(--bulma-text); text-decoration: none;">
                    <svg xmlns="http://www.w3.org/2000/svg" width="28" height="28" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">
                        <path d="M21 15a2 2 0 0 1-2 2H7l-4 4V5a2 2 0 0 1 2-2h14a2 2 0 0 1 2 2z" />
                    </svg>
                    <span style="font-size: 0.6rem; margin-top: 2px;">{move || t("nav.support")}</span>
                </a>
            </nav>
        </Router>

        // Fullscreen story viewer (Portal to <body>) — mounted here so a story can
        // open over any screen, including the first-launch persona editor.
        <crate::components::story_tray::StoryViewerHost/>
    }
    .into_view()
}

/// Дашборд не строится, пока не открыта база пользователя.
///
/// Роутер смонтирован ПОД экранами установки и входа, поэтому дашборд оживал
/// раньше базы: его ресурсы считали индикаторы по пустоте и пытались положить
/// результат туда, где хранить некому. Расчёт был заведомо бессмысленным (данных
/// нет по определению), запись — заведомо потерянной, а человек получал
/// три сообщения об ошибке, с которыми ничего сделать не мог.
///
/// Ждать надо ИМЕННО не построив компонент: ресурсы Leptos стартуют при создании,
/// и спрятать готовый дашборд под условие в разметке было бы поздно.
#[component]
fn DashboardGate() -> impl IntoView {
    let ready = crate::services::db::ready_signal();
    move || {
        ready
            .get()
            .then(|| view! { <pages::dashboard::DashboardPage/> })
    }
}
