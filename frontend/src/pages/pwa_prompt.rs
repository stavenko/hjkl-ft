use leptos::*;
use wasm_bindgen::JsValue;

use crate::services::i18n::t;
use crate::services::platform;
/// Опознание браузера, заголовок и шаги переехали в общий крейт: кураторское
/// приложение ставится теми же способами, и держать два набора этих экранов
/// нельзя. Здесь остаётся всё, что есть только у приложения худеющего, —
/// спецэкраны Яндекса/Mi/неопознанного браузера, телеметрия и intent-хэндофф.
pub use pwa_prompt::{detect_platform, title_key};
use pwa_prompt::render_steps as shared_steps;

/// Язык для общих шагов. Слова у них свои, крейтовые, — приложению остаётся
/// сообщить, на каком языке их показать.
fn pwa_lang() -> pwa_prompt::Lang {
    match crate::services::i18n::get_lang() {
        crate::services::i18n::Lang::En => pwa_prompt::Lang::En,
        crate::services::i18n::Lang::Ru => pwa_prompt::Lang::Ru,
    }
}

fn render_steps(platform: &str) -> View {
    shared_steps(platform, pwa_lang)
}


/// Android intent URL that opens the app in Chrome SPECIFICALLY (package=), ignoring
/// the default browser. Carries `?u=<user_id>` so Chrome (fresh localStorage, no
/// session) can offer the Telegram-code login for this account. Falls back to the
/// plain https URL when Chrome is absent.
fn system_browser_intent_url() -> String {
    let win = web_sys::window().expect("no window");
    let loc = win.location();
    let host = loc.host().unwrap_or_default();

    // Путь СОХРАНЯЕТСЯ. Раньше здесь стоял корень, и это уводило человека не туда:
    // ссылка из Telegram ведёт на `/onboard?u=…`, а это особый вход — там
    // приложение не накрывает страницу оверлеями, а ведёт свой порядок (код,
    // ключ, и лишь в конце установка). На корне же первым делом показывается
    // экран установки, так что перешедший видел предложение поставить приложение
    // вместо регистрации, а зарегистрироваться ему было негде.
    let path = loc.pathname().unwrap_or_else(|_| "/".to_string());
    let params = loc
        .search()
        .ok()
        .and_then(|s| web_sys::UrlSearchParams::new_with_str(&s).ok())
        .unwrap_or_else(|| web_sys::UrlSearchParams::new().expect("UrlSearchParams"));

    // Account id: the signed-in session first, else the `?u=` this page was
    // opened with (unauthenticated launch of a `?u=`-carrying link).
    let url_u = params.get("u").filter(|s| !s.is_empty());
    if let Some(uid) = crate::services::auth::get_user_id().or(url_u) {
        params.set("u", &uid);
    }
    let query = String::from(params.to_string());
    let query = if query.is_empty() { String::new() } else { format!("?{query}") };

    let target = format!("https://{host}{path}{query}");
    let fallback = js_sys::encode_uri_component(&target);
    format!(
        "intent://{host}{path}{query}#Intent;scheme=https;package=com.android.chrome;S.browser_fallback_url={fallback};end"
    )
}

/// Кружок с номером шага. Экран для неопознанного браузера рисуется РАНЬШЕ общей
/// обвязки со стилями `.step-num`, поэтому оформление здесь своё, встроенное.
const STEP_NUM: &str = "flex-shrink: 0; display: inline-flex; align-items: center; \
     justify-content: center; width: 1.6rem; height: 1.6rem; border-radius: 50%; \
     background: var(--bulma-link); color: #fff; font-size: 0.85rem; font-weight: 700;";

/// Значок Safari — компас: синий круг, белое кольцо и стрелка из двух половин.
/// Нарисован здесь же, как и хромовский: сторонних адресов мы не грузим.
#[component]
fn SafariMark() -> impl IntoView {
    view! {
        <svg viewBox="0 0 48 48" width="64" height="64" style="display: block; margin: 0 auto 18px;">
            <circle cx="24" cy="24" r="20" fill="#1EA0F0"/>
            <circle cx="24" cy="24" r="16" fill="none" stroke="#fff" stroke-width="1.5"/>
            // Стрелка: красная половина смотрит на северо-восток, светлая — назад.
            <path d="M33 15 L26 26 L22 22 Z" fill="#F5433B"/>
            <path d="M15 33 L22 22 L26 26 Z" fill="#F2F2F2"/>
        </svg>
    }
}

/// Логотип Chrome — нарисованный здесь же, а не картинкой со стороннего адреса:
/// три сектора, белая втулка и синяя середина.
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

/// Снимок браузера для наших логов: строка User-Agent и ответы на те вопросы,
/// от которых зависит, заработает ли приложение вообще. Одного UA мало — он
/// врёт и ничего не говорит о возможностях; а «ключи есть, сервис-воркера нет»
/// сразу показывает, чинить тут или уводить.
fn browser_report() -> String {
    let Some(win) = web_sys::window() else {
        return "нет window".to_string();
    };
    let nav = win.navigator();
    let ua = nav.user_agent().unwrap_or_default();
    let has = |obj: &JsValue, key: &str| {
        js_sys::Reflect::get(obj, &JsValue::from_str(key))
            .map(|v| !v.is_undefined() && !v.is_null())
            .unwrap_or(false)
    };
    let creds = js_sys::Reflect::get(nav.as_ref(), &JsValue::from_str("credentials")).ok();
    let creds_create = creds
        .as_ref()
        .and_then(|c| js_sys::Reflect::get(c, &JsValue::from_str("create")).ok())
        .map(|f| f.is_function())
        .unwrap_or(false);
    let standalone = win
        .match_media("(display-mode: standalone)")
        .ok()
        .flatten()
        .map(|m| m.matches())
        .unwrap_or(false);
    let lang = nav.language().unwrap_or_default();
    format!(
        "UA: {ua}\nplatform: unknown\nPublicKeyCredential: {}\nnavigator.credentials: {}\n\
         credentials.create: {}\nserviceWorker: {}\nstandalone: {standalone}\nlang: {lang}",
        has(win.as_ref(), "PublicKeyCredential"),
        creds.as_ref().map(|c| !c.is_undefined() && !c.is_null()).unwrap_or(false),
        creds_create,
        has(nav.as_ref(), "serviceWorker"),
    )
}

/// Адрес приложения, который человек унесёт в Chrome: тот же самый, что открыт
/// сейчас, вместе с `?u=` — иначе он вернётся без аккаунта.
fn current_app_url() -> String {
    let Some(loc) = web_sys::window().map(|w| w.location()) else {
        return String::new();
    };
    let origin = loc.origin().unwrap_or_default();
    let path = loc.pathname().unwrap_or_else(|_| "/".to_string());
    let search = loc.search().unwrap_or_default();
    format!("{origin}{path}{search}")
}

#[component]
pub fn PwaPrompt(on_dismiss: Callback<()>) -> impl IntoView {
    let platform = detect_platform();
    let title = title_key(platform);
    let steps = render_steps(platform);

    // Десктопная ветка: ни телефон, ни планшет. Она же ловит ТЕЛЕФОН с включённой
    // «Версией для ПК» — браузер в этом режиме отдаёт десктопный User-Agent, и
    // отличить его от компьютера мы честно не можем.
    let is_desktop = !platform.starts_with("android") && !platform.starts_with("ios");

    let dismiss = move |_| {
        platform::dismiss_pwa_prompt();
        on_dismiss.call(());
    };

    // Yandex Browser (and the Yandex app): a passkey cannot be created there
    // (`navigator.credentials` is absent) and a PWA cannot be installed, so the
    // user is walked into Chrome instead. An intent hand-off does NOT work —
    // Chromium refuses to launch an intent whose target is another browser and
    // silently follows `browser_fallback_url`, i.e. reopens the page HERE — so
    // the screen teaches the only route that does work: the browser's own share
    // button → pick Chrome. Blocking by design: there is no way past it.
    // iOS is untouched: this branch is Android-Yandex only.
    // Mi Browser. Отдельный экран, НЕ общий с яндексовским: там intent не
    // срабатывает (Chromium не запускает intent, целящий в другой браузер, и молча
    // уходит по browser_fallback_url — то есть переоткрывает страницу здесь же),
    // поэтому там учат листу «Поделиться». Здесь intent работает — проверено на
    // устройстве, — и человеку нужна ровно одна кнопка, а не инструкция.
    // Samsung Internet — сюда же: intent оттуда срабатывает (проверено на
    // устройстве, SamsungBrowser/30.0), значит лечение то же, что у Mi, — одна
    // кнопка в Chrome. Инструкцию по установке человек увидит уже в Chrome, а не
    // здесь: прежняя ветка «android_samsung» учила ставить из самого Samsung
    // Internet и до этого экрана теперь не доходит.
    // Браузер не опознан вовсе. Инструкции по установке тут показывать нечего:
    // мы не знаем ни его меню, ни его пунктов — и раньше молча подсовывали чужую,
    // от Chrome. Вместо этого говорим прямо и даём унести адрес в Chrome руками:
    // intent-кнопки тут нет намеренно, неизвестный браузер может её не понять.
    if platform == "unknown" || platform == "ios_unknown" {
        // На айфоне уводить некуда, кроме Safari: Chrome там — тот же WebKit, и
        // советовать его бессмысленно. Отсюда две редакции текста и два значка.
        let ios = platform == "ios_unknown";
        let url = current_app_url();
        // Сигнал нам: пока такие браузеры не попадали ни в какую статистику, и
        // узнать о них было неоткуда. Уходит один раз при показе экрана — вместе
        // со СНИМКОМ ВОЗМОЖНОСТЕЙ браузера, а не одним User-Agent'ом: чинить по
        // строке UA нечего, а по «есть ли ключи, есть ли сервис-воркер» — есть.
        crate::services::telemetry::report_internal("browser.unsupported", &url, &browser_report());
        let copied = create_rw_signal(false);
        let url_for_copy = url.clone();
        // «Скопировано» показываем только если буфер И ПРАВДА принял: обещание
        // может отклониться (нет прав, не тот контекст), и врать об этом нельзя —
        // человек уйдёт в Chrome с пустым буфером.
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
        return view! {
            <div attr:data-testid="pwa-unknown-screen"
                 style="min-height: 100vh; display: flex; flex-direction: column; align-items: center; \
                        justify-content: center; padding: 32px 24px; text-align: center; \
                        background: var(--bulma-scheme-main);">
                <div style="max-width: 24rem; width: 100%;">
                    {if ios { view! { <SafariMark /> } } else { view! { <ChromeMark /> } }}
                    <h1 class="title is-5" style="line-height: 1.35; margin-bottom: 14px;">
                        {move || t("pwa.unknown.title")}
                    </h1>
                    <p class="has-text-grey" style="line-height: 1.6; margin-bottom: 18px;">
                        {move || t("pwa.unknown.signal")}
                    </p>
                    <p style="line-height: 1.6; margin-bottom: 18px;">
                        {move || t(if ios { "pwa.unknown.safari" } else { "pwa.unknown.chrome" })}
                    </p>
                    // Порядок действий — пунктами и ПО ЛЕВОМУ КРАЮ: тремя фразами
                    // подряд по центру он читается как рассуждение, а не как то,
                    // что надо сделать по шагам.
                    <div style="text-align: left; margin-top: 4px;">
                        <div style="display: flex; gap: 10px; margin-bottom: 8px;">
                            <span style=STEP_NUM>"1"</span>
                            <div style="line-height: 1.5;">{move || t("pwa.unknown.step1")}</div>
                        </div>
                        // Адрес копируется тапом по нему же: набирать такое руками —
                        // отдельное мучение, а «выделите и скопируйте» на телефоне
                        // работает через раз.
                        <button
                            attr:data-testid="pwa-btn-copy-url"
                            class="button is-light is-fullwidth"
                            style="height: auto; padding: 12px; white-space: normal; word-break: break-all; \
                                   font-family: monospace; line-height: 1.45; margin-left: 0;"
                            on:click=copy
                        >
                            {url.clone()}
                        </button>
                        <p attr:data-testid="pwa-copied" class="has-text-success is-size-7"
                           style="min-height: 1.2em; margin: 6px 0 14px; text-align: center;">
                            {move || if copied.get() { t("pwa.unknown.copied") } else { "" }}
                        </p>
                        <div style="display: flex; gap: 10px; margin-bottom: 8px;">
                            <span style=STEP_NUM>"2"</span>
                            <div style="line-height: 1.5;">{move || t(if ios { "pwa.unknown.step2_safari" } else { "pwa.unknown.step2" })}</div>
                        </div>
                        <div style="display: flex; gap: 10px;">
                            <span style=STEP_NUM>"3"</span>
                            <div style="line-height: 1.5;">{move || t("pwa.unknown.step3")}</div>
                        </div>
                    </div>
                </div>
            </div>
        }
        .into_view();
    }

    if platform == "android_mi" || platform == "android_samsung" {
        let intent = system_browser_intent_url();
        return view! {
            <div attr:data-testid="pwa-mi-screen"
                 style="min-height: 100vh; display: flex; flex-direction: column; align-items: center; \
                        justify-content: center; padding: 32px 24px; text-align: center; \
                        background: var(--bulma-scheme-main);">
                <div style="max-width: 24rem; width: 100%;">
                    <img src="/icon-192.png" alt="re:Norma"
                         style="width: 80px; height: 80px; border-radius: 16px; margin-bottom: 20px;" />
                    <h1 class="title is-4" style="line-height: 1.3; margin-bottom: 28px;">
                        {move || t("pwa.mi.title")}
                    </h1>
                    <a
                        attr:data-testid="pwa-btn-open-chrome"
                        class="button is-link is-large is-fullwidth has-text-weight-semibold"
                        href=intent
                    >
                        {move || t("pwa.mi.open")}
                    </a>
                </div>
            </div>
        }
        .into_view();
    }

    if platform == "android_yandex" {
        return view! {
            <div attr:data-testid="pwa-yandex-screen"
                 style="min-height: 100vh; padding: 28px 20px 40px; text-align: center; \
                        background: var(--bulma-scheme-main); overflow-y: auto;">
                <div style="max-width: 26rem; margin: 0 auto;">
                    <img src="/icon-192.png" alt="re:Norma"
                         style="width: 72px; height: 72px; border-radius: 16px; margin-bottom: 18px;" />
                    <h1 class="title is-5" style="line-height: 1.3; margin-bottom: 10px;">
                        {move || t("pwa.yandex.title")}
                    </h1>
                    <p class="has-text-grey" style="line-height: 1.55; margin-bottom: 26px;">
                        {move || t("pwa.yandex.lead")}
                    </p>

                    <div style="text-align: left; margin-bottom: 22px;">
                        <p style="line-height: 1.5; margin-bottom: 10px;">
                            <span style="display: inline-flex; align-items: center; justify-content: center; width: 22px; height: 22px; border-radius: 50%; background: var(--bulma-text-strong); color: var(--bulma-scheme-main); font-size: 13px; font-weight: 700; margin-right: 8px; vertical-align: 1px;">"1"</span>
                            {move || t("pwa.yandex.step1")}
                        </p>
                        <img src="/onboard-img/hop-share.gif" alt=""
                             style="display: block; width: 100%; border-radius: 14px; border: 1px solid var(--bulma-border);" />
                    </div>

                    <div style="text-align: left;">
                        <p style="line-height: 1.5; margin-bottom: 10px;">
                            <span style="display: inline-flex; align-items: center; justify-content: center; width: 22px; height: 22px; border-radius: 50%; background: var(--bulma-text-strong); color: var(--bulma-scheme-main); font-size: 13px; font-weight: 700; margin-right: 8px; vertical-align: 1px;">"2"</span>
                            {move || t("pwa.yandex.step2")}
                        </p>
                        <img src="/onboard-img/hop-chrome.gif" alt=""
                             style="display: block; width: 100%; border-radius: 14px; border: 1px solid var(--bulma-border);" />
                    </div>
                </div>
            </div>
        }
        .into_view();
    }

    view! {
        <div style="min-height: 100vh; display: flex; flex-direction: column; align-items: center; justify-content: center; padding: 2rem; text-align: center; background: var(--bulma-scheme-main); overflow-y: auto;">
            <div style="max-width: 24rem;">
                <img src="/icon-192.png" alt="re:Norma" style="width: 80px; height: 80px; border-radius: 16px; margin-bottom: 1rem;" />
                <h1 class="title is-3" style="margin-bottom: 0.5rem;">"re:Norma"</h1>
                // На десктопе «поставьте на рабочий стол» — неверный совет: ставить
                // это приложение на компьютер незачем, оно телефонное. Там свой
                // текст, ниже.
                {(!is_desktop).then(|| view! {
                    <p class="has-text-grey mb-5" style="font-size: 1.05rem; line-height: 1.6;">
                        {move || t("pwa.description")}
                    </p>
                })}

                // Инструкции по установке — ТОЛЬКО для телефонов. На десктопе она
                // и бессмысленна («приложение мобильное» и тут же «ставим на Mac»),
                // и прямо вредна: эту же ветку видит ТЕЛЕФОН с включённой «Версией
                // для ПК», и человек ищет у себя значок установки в адресной строке
                // Safari, которого там нет.
                {(!is_desktop).then(|| view! {
                    <div class="box" style="text-align: left; margin-bottom: 2rem;">
                        <p class="has-text-weight-semibold mb-4">{t(title)}</p>
                        {steps}
                    </div>
                })}

                // «Продолжить в браузере» есть только на десктопе. На телефоне
                // приложение обязано стоять как PWA: браузерная вкладка на
                // Android открывается системным браузером (у части людей это
                // Яндекс, где приложение не работает), поэтому выхода из
                // инструкции нет — ставим PWA.
                //
                // Но десктопная ветка ловит и ТЕЛЕФОН с включённой «Версией для
                // ПК»: браузер в этом режиме отдаёт десктопный User-Agent, и мы
                // честно не можем отличить его от компьютера. Поэтому здесь
                // объясняем оба случая сразу — и человеку с телефона это самое
                // важное, потому что решается одной галочкой.
                {is_desktop.then(|| view! {
                    <div style="text-align: left; margin-bottom: 1.25rem; line-height: 1.6;">
                        <p class="has-text-weight-semibold" style="margin-bottom: 8px;">
                            {move || t("pwa.desktop.mobile_first")}
                        </p>
                        <p class="has-text-grey" style="margin-bottom: 8px;">
                            {move || t("pwa.desktop.if_phone")}
                        </p>
                        <p class="has-text-grey">
                            {move || t("pwa.desktop.if_desktop")}
                        </p>
                    </div>
                    <button
                        attr:data-testid="pwa-btn-dismiss"
                        class="button is-light is-fullwidth"
                        on:click=dismiss
                    >
                        {move || t("pwa.use_browser")}
                    </button>
                })}
            </div>
        </div>
    }
    .into_view()
}
