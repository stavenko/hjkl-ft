//! Экраны установки приложения на домашний экран.
//!
//! Пошаговые инструкции берутся из общего крейта `pwa-prompt` — они выстраданы на
//! живых устройствах (у Яндекса на iPhone в листе «Поделиться» нет пункта «На
//! экран „Домой“»; Chrome на iPhone зовётся в UA «CriOS»; Mi Browser не умеет
//! ключи вовсе), и второй копии у них быть не должно.
//!
//! Здесь — только то, что общий крейт не покрывает: спецэкраны браузеров, в
//! которых приложению работать нечем. Их устройство повторяет приложение
//! худеющего, вместе с причинами:
//!
//! * Mi Browser и Samsung Internet — intent в Chrome ОТТУДА СРАБАТЫВАЕТ
//!   (проверено на устройствах), поэтому там одна кнопка, а не инструкция.
//! * Яндекс.Браузер — intent там НЕ работает: Chromium отказывается запускать
//!   intent, целящий в другой браузер, и молча уходит по `browser_fallback_url`,
//!   то есть переоткрывает страницу здесь же. Поэтому учим листу «Поделиться».
//! * Браузер не опознан — инструкции показывать нечего, мы не знаем ни его меню,
//!   ни его пунктов. Раньше в приложении худеющего таким молча подсовывали чужую,
//!   от Chrome. Вместо этого — сказать прямо и дать унести адрес руками.

use leptos::*;

use crate::i18n::{pwa_lang, t};
use crate::platform;

/// Кружок с номером шага. Экраны тупиковых браузеров рисуются БЕЗ обвязки общего
/// крейта (там своя разметка и свои `.step-num`), поэтому оформление тут своё.
const STEP_NUM: &str = "flex-shrink: 0; display: inline-flex; align-items: center; \
     justify-content: center; width: 1.6rem; height: 1.6rem; border-radius: 50%; \
     background: var(--accent); color: var(--accent-ink); font-size: 0.85rem; font-weight: 700;";

const STEP_ROW: &str = "display: flex; gap: 10px; margin-bottom: 10px; line-height: 1.5;";

/// Android-intent, открывающий страницу ИМЕННО в Chrome (`package=`), в обход
/// браузера по умолчанию. Если Chrome нет — уходит по обычному https-адресу.
fn chrome_intent_url() -> String {
    let Some(win) = web_sys::window() else { return String::new() };
    let loc = win.location();
    let host = loc.host().unwrap_or_default();
    let path = loc.pathname().unwrap_or_else(|_| "/".to_string());
    let query = loc.search().unwrap_or_default();

    let target = format!("https://{host}{path}{query}");
    let fallback = js_sys::encode_uri_component(&target);
    format!(
        "intent://{host}{path}{query}#Intent;scheme=https;package=com.android.chrome;\
         S.browser_fallback_url={fallback};end"
    )
}

/// Адрес приложения, который человек унесёт в другой браузер руками.
fn current_app_url() -> String {
    let Some(loc) = web_sys::window().map(|w| w.location()) else { return String::new() };
    let origin = loc.origin().unwrap_or_default();
    let path = loc.pathname().unwrap_or_else(|_| "/".to_string());
    let search = loc.search().unwrap_or_default();
    format!("{origin}{path}{search}")
}

/// Значок Safari — компас. Нарисован здесь же: сторонних адресов мы не грузим
/// (и CSP их не пустит).
#[component]
fn SafariMark() -> impl IntoView {
    view! {
        <svg viewBox="0 0 48 48" width="64" height="64" style="display: block; margin: 0 auto 18px;">
            <circle cx="24" cy="24" r="20" fill="#1EA0F0"/>
            <circle cx="24" cy="24" r="16" fill="none" stroke="#fff" stroke-width="1.5"/>
            <path d="M33 15 L26 26 L22 22 Z" fill="#F5433B"/>
            <path d="M15 33 L22 22 L26 26 Z" fill="#F2F2F2"/>
        </svg>
    }
}

/// Логотип Chrome — тоже нарисованный здесь, а не картинкой со стороннего адреса.
#[component]
fn ChromeMark() -> impl IntoView {
    view! {
        <svg viewBox="0 0 48 48" width="64" height="64" style="display: block; margin: 0 auto 18px;">
            <path fill="#EA4335" d="M6.68 14 A20 20 0 0 1 41.32 14 Z"/>
            <path fill="#FBBC05" d="M41.32 14 A20 20 0 0 1 24 44 Z"/>
            <path fill="#34A853" d="M24 44 A20 20 0 0 1 6.68 14 Z"/>
            <circle cx="24" cy="24" r="11" fill="#fff"/>
            <circle cx="24" cy="24" r="9" fill="#4285F4"/>
        </svg>
    }
}

/// Экран установки целиком: сам выбирает, что показать этому браузеру.
///
/// `on_dismiss` зовётся ТОЛЬКО с десктопной ветки: на телефоне выхода из
/// инструкции нет — приложение обязано стоять как PWA.
#[component]
pub fn InstallScreen(on_dismiss: Callback<()>) -> impl IntoView {
    let platform = pwa_prompt::detect_platform();

    // Тупиковые браузеры — прежде всего: там не установка, а уход в другой
    // браузер, и никакая отметка об установке этого не отменяет.
    if platform == "unknown" || platform == "ios_unknown" {
        return view! { <UnknownBrowserScreen ios={platform == "ios_unknown"} /> }.into_view();
    }
    if platform == "android_mi" || platform == "android_samsung" {
        return view! { <ChromeHandoffScreen /> }.into_view();
    }
    if platform == "android_yandex" {
        return view! { <YandexScreen /> }.into_view();
    }

    // Приложение уже поставлено, а мы всё ещё во вкладке — конечная: дальше
    // человек уходит с иконки на рабочем столе, а не отсюда.
    //
    // Сигнал — реактивное зеркало отметки в localStorage. Он нужен ровно затем,
    // чтобы кнопка «иконки так и нет» вернула к инструкции ЗДЕСЬ ЖЕ, не гоняя
    // состояние через всё приложение (а заодно и через проверку подписки).
    let installed = create_rw_signal(platform::pwa_installed());
    let back_to_steps = Callback::new(move |_| {
        platform::clear_pwa_installed();
        installed.set(false);
    });

    // Десктопная ветка ловит и ТЕЛЕФОН с включённой «Версией для ПК»: браузер в
    // этом режиме отдаёт десктопный User-Agent, и отличить его от компьютера мы
    // честно не можем. Поэтому там объясняются оба случая сразу.
    let is_desktop = !platform.starts_with("android") && !platform.starts_with("ios");
    let title = pwa_prompt::title_key(platform);

    // Шаги СТРОЯТСЯ на каждую отрисовку, а не запоминаются: `View` в CSR — это
    // живые узлы DOM, и один и тот же экземпляр, вставленный второй раз, не
    // копируется, а переезжает. Возврат к инструкции («иконки так и нет») —
    // ровно вторая отрисовка.
    let instructions = move || view! {
        <div class="screen screen--center"
             attr:data-testid=if is_desktop { "install-desktop" } else { "install-steps" }>
            <div class="center">
                <img src="/icons/icon-192.png" alt="" class="applogo" />
                {(!is_desktop).then(|| view! {
                    <>
                        <p class="h1">{move || t("install.title")}</p>
                        <p class="sub">{move || t("install.body")}</p>
                        <div class="card" style="text-align: left;">
                            <p style="font-weight: 640; margin: 0 0 14px;">{move || t(title)}</p>
                            {pwa_prompt::render_steps(platform, pwa_lang)}
                        </div>
                    </>
                })}
                {is_desktop.then(|| view! {
                    <>
                        <p class="h1">{move || t("install.title")}</p>
                        <div style="text-align: left; line-height: 1.6; margin-bottom: 22px;">
                            <p style="font-weight: 640; margin: 0 0 8px;">{move || t("install.desktop_lead")}</p>
                            <p class="sub" style="margin: 0 0 8px;">{move || t("install.desktop_phone")}</p>
                            <p class="sub" style="margin: 0;">{move || t("install.desktop_pc")}</p>
                        </div>
                        <button class="btn btn--block" attr:data-testid="install-btn-dismiss"
                            on:click=move |_| {
                                platform::dismiss_pwa_prompt();
                                on_dismiss.call(());
                            }>
                            {move || t("install.desktop_continue")}
                        </button>
                    </>
                })}
            </div>
        </div>
    };

    view! {
        {move || if installed.get() {
            view! { <InstalledScreen on_show_steps=back_to_steps /> }.into_view()
        } else {
            instructions().into_view()
        }}
    }
    .into_view()
}

/// Установка завершена.
#[component]
fn InstalledScreen(on_show_steps: Callback<()>) -> impl IntoView {
    view! {
        <div class="screen screen--center" attr:data-testid="install-installed">
            <div class="center">
                <img src="/icons/icon-192.png" alt="" class="applogo" />
                <p class="h1">{move || t("install.done_title")}</p>
                <p class="sub">{move || t("install.done_body")}</p>
                <div class="banner banner--warn" style="text-align: left;">
                    {move || t("install.done_wait")}
                </div>
                <p class="hint" style="text-align: left; margin-top: 18px;">
                    {move || t("install.done_missing")}
                </p>
                <button class="btn btn--block" style="margin-top: 10px;"
                    attr:data-testid="install-btn-show-steps"
                    on:click=move |_| {
                        platform::clear_pwa_installed();
                        on_show_steps.call(());
                    }>
                    {move || t("install.done_show")}
                </button>
            </div>
        </div>
    }
}

/// Mi Browser / Samsung Internet: одна кнопка в Chrome. Инструкцию по установке
/// человек увидит уже там.
#[component]
fn ChromeHandoffScreen() -> impl IntoView {
    let intent = chrome_intent_url();
    view! {
        <div class="screen screen--center" attr:data-testid="install-chrome-handoff">
            <div class="center">
                <img src="/icons/icon-192.png" alt="" class="applogo" />
                <p class="h1" style="margin-bottom: 24px;">{move || t("dead.mi.title")}</p>
                <a class="btn btn--primary btn--block" attr:data-testid="install-btn-open-chrome" href=intent>
                    {move || t("dead.mi.open")}
                </a>
            </div>
        </div>
    }
}

/// Яндекс.Браузер: intent не сработает, поэтому учим листу «Поделиться».
/// Гифки — общие, из крейта `pwa-prompt` (те же, что показывает приложение
/// худеющего на этом же экране).
#[component]
fn YandexScreen() -> impl IntoView {
    view! {
        <div class="screen screen--center" attr:data-testid="install-yandex">
            <div class="center">
                <img src="/icons/icon-192.png" alt="" class="applogo" />
                <p class="h1">{move || t("dead.yandex.title")}</p>
                <p class="sub">{move || t("dead.yandex.lead")}</p>
                <div style="text-align: left; margin-bottom: 20px;">
                    <div style=STEP_ROW>
                        <span style=STEP_NUM>"1"</span>
                        <div>{move || t("dead.yandex.step1")}</div>
                    </div>
                    <img src="/onboard-img/hop-share.gif" alt="" class="shot" />
                </div>
                <div style="text-align: left;">
                    <div style=STEP_ROW>
                        <span style=STEP_NUM>"2"</span>
                        <div>{move || t("dead.yandex.step2")}</div>
                    </div>
                    <img src="/onboard-img/hop-chrome.gif" alt="" class="shot" />
                </div>
            </div>
        </div>
    }
}

/// Браузер не опознан. Инструкции нет — есть адрес, который можно унести.
#[component]
fn UnknownBrowserScreen(ios: bool) -> impl IntoView {
    let url = current_app_url();
    let copied = create_rw_signal(false);
    let url_for_copy = url.clone();
    // «Скопировано» показываем, только если буфер И ПРАВДА принял: обещание может
    // отклониться (нет прав, не тот контекст), и врать об этом нельзя — человек
    // уйдёт в Chrome с пустым буфером.
    let copy = move |_| {
        let Some(win) = web_sys::window() else { return };
        let promise = win.navigator().clipboard().write_text(&url_for_copy);
        leptos::spawn_local(async move {
            match wasm_bindgen_futures::JsFuture::from(promise).await {
                Ok(_) => copied.set(true),
                Err(e) => leptos::logging::warn!("буфер обмена отказал: {e:?}"),
            }
        });
    };
    view! {
        <div class="screen screen--center" attr:data-testid="install-unknown">
            <div class="center">
                // На айфоне уводить некуда, кроме Safari: Chrome там — тот же
                // WebKit, и советовать его бессмысленно. Отсюда два значка и две
                // редакции текста.
                {if ios { view! { <SafariMark /> } } else { view! { <ChromeMark /> } }}
                <p class="h1">{move || t("dead.unknown.title")}</p>
                <p class="sub">{move || t("dead.unknown.signal")}</p>
                <p style="line-height: 1.6; margin-bottom: 18px;">
                    {move || t(if ios { "dead.unknown.safari" } else { "dead.unknown.chrome" })}
                </p>
                // Порядок действий — пунктами и ПО ЛЕВОМУ КРАЮ: тремя фразами
                // подряд по центру он читается как рассуждение, а не как то, что
                // надо сделать по шагам.
                <div style="text-align: left;">
                    <div style=STEP_ROW>
                        <span style=STEP_NUM>"1"</span>
                        <div>{move || t("dead.unknown.step1")}</div>
                    </div>
                    // Адрес копируется тапом по нему же: набирать такое руками —
                    // отдельное мучение, а «выделите и скопируйте» на телефоне
                    // работает через раз.
                    <button class="btn btn--block mono" attr:data-testid="install-btn-copy-url"
                        style="height: auto; padding: 12px; white-space: normal; word-break: break-all; \
                               line-height: 1.45;"
                        on:click=copy>
                        {url.clone()}
                    </button>
                    <p attr:data-testid="install-copied"
                       style="min-height: 1.2em; margin: 6px 0 14px; text-align: center; \
                              color: var(--accent); font-size: .82rem;">
                        {move || if copied.get() { t("dead.unknown.copied") } else { "" }}
                    </p>
                    <div style=STEP_ROW>
                        <span style=STEP_NUM>"2"</span>
                        <div>{move || t(if ios { "dead.unknown.step2_safari" } else { "dead.unknown.step2" })}</div>
                    </div>
                    <div style=STEP_ROW>
                        <span style=STEP_NUM>"3"</span>
                        <div>{move || t("dead.unknown.step3")}</div>
                    </div>
                </div>
            </div>
        </div>
    }
}
