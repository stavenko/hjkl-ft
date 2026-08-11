use leptos::*;
use wasm_bindgen::JsValue;

use crate::components::pwa_icons::*;
use crate::services::i18n::t;
use crate::services::platform;

pub fn detect_platform() -> &'static str {
    let ua = web_sys::window()
        .and_then(|w| w.navigator().user_agent().ok())
        .unwrap_or_default()
        .to_lowercase();

    let is_ios = ua.contains("iphone") || ua.contains("ipad") || ua.contains("ipod");
    let is_android = ua.contains("android");
    let is_mac = ua.contains("macintosh") || ua.contains("mac os");

    // Chrome на iPhone зовётся в UA «CriOS», слова «chrome» там НЕТ вовсе. Без
    // этого он не опознавался как Chrome, зато подходил под правило Safari (в его
    // UA есть и «safari», и «version/») — и человек получал чужую инструкцию:
    // «кнопка „Поделиться“ внизу экрана», которой у Chrome нет, значок стоит в
    // адресной строке. Поэтому «crios» и добавляется в Chrome, и вычитается из Safari.
    let is_crios = ua.contains("crios");
    let is_chrome = (ua.contains("chrome") || is_crios) && !ua.contains("edg") && !ua.contains("opr");
    let is_firefox = ua.contains("firefox");
    let is_edge = ua.contains("edg/");
    let is_safari =
        ua.contains("safari") && !ua.contains("chrome") && !ua.contains("chromium") && !is_crios;
    let is_samsung = ua.contains("samsungbrowser");
    // Yandex Browser (yabrowser) AND the Yandex app's built-in browser (yasearchbrowser /
    // yaapp_android) — the latter has NO "yabrowser" in its UA but does contain "chrome",
    // so without this it would fall through to the Chrome instructions.
    let is_yandex = ua.contains("yabrowser") || ua.contains("yasearchbrowser") || ua.contains("yaapp_android");
    // Mi Browser (Xiaomi). В UA есть "chrome", поэтому без своей проверки он
    // уходил бы в ветку Chrome — а ключи он не умеет: `PublicKeyCredential` там
    // отсутствует вовсе (замерено пробником на Redmi, Android 15, MiuiBrowser
    // 14.60). Зеркальный случай Яндекса: там был интерфейс без `credentials`,
    // здесь — `credentials.create` без интерфейса.
    let is_mi = ua.contains("miuibrowser");

    // Яндекс.Браузер на iPhone проверяется ПЕРВЫМ: его UA содержит и "safari", и
    // "version/", поэтому иначе он уходил бы в ветку Safari — а там инструкция
    // «Поделиться → На экран Домой», и пункта «На экран Домой» в его листе
    // «Поделиться» нет. Ставится он иначе: ⋮ → «Добавить ярлык на телефон».
    if is_ios && is_yandex { return "ios_yandex"; }
    if is_ios && is_safari { return "ios_safari"; }
    if is_ios && is_chrome { return "ios_chrome"; }
    if is_ios && is_firefox { return "ios_firefox"; }
    if is_ios { return "ios_safari"; }

    if is_android && is_samsung { return "android_samsung"; }
    if is_android && is_yandex { return "android_yandex"; }
    // ПОСЛЕ яндексовской строки намеренно: если UA когда-нибудь понесёт оба
    // маркера, победит прежний, выстраданный путь, а не новый.
    if is_android && is_mi { return "android_mi"; }
    if is_android && is_firefox { return "android_firefox"; }
    if is_android && is_chrome { return "android_chrome"; }
    // Android, но ни один известный браузер не опознан. Раньше такой человек молча
    // получал инструкцию для Chrome — и не находил у себя ни того меню, ни тех
    // пунктов. Честнее сказать прямо, что этот браузер мы не умеем.
    if is_android { return "unknown"; }

    if is_mac && is_safari { return "macos_safari"; }
    if is_mac && is_chrome { return "macos_chrome"; }
    if is_mac && is_edge { return "macos_edge"; }
    if is_mac && is_firefox { return "macos_firefox"; }
    if is_mac { return "macos_chrome"; }

    if is_chrome { return "desktop_chrome"; }
    if is_edge { return "desktop_edge"; }
    if is_firefox { return "desktop_firefox"; }

    // Ни система, ни браузер не опознаны — тот же честный ответ, что и на Android.
    "unknown"
}

fn title_key(platform: &str) -> &'static str {
    match platform {
        s if s.starts_with("ios") => "pwa.title.ios",
        s if s.starts_with("android") => "pwa.title.android",
        s if s.starts_with("macos") => "pwa.title.macos",
        _ => "pwa.title.desktop",
    }
}

fn render_steps(platform: &str) -> View {
    match platform {
        // Safari на iPhone. Раньше шаги были на словах и с нарисованными значками;
        // теперь — снимки живого интерфейса с мигающей подсказкой, куда нажимать
        // (scripts/shot-ios-install-gifs.mjs). Значок «Поделиться» у Safari стоит
        // ПОСЕРЕДИНЕ нижней панели, а не в адресной строке, как у Chrome.
        "ios_safari" => view! {
            <div class="steps">
                <div class="step">
                    <span class="step-num">"1"</span>
                    <div class="step-body">
                        {move || t("pwa.inst.ios_safari.1")}
                        <img src="/onboard-img/ios-safari-share.gif" alt="" class="step-shot" />
                    </div>
                </div>
                <div class="step">
                    <span class="step-num">"2"</span>
                    <div class="step-body">
                        {move || t("pwa.inst.ios_safari.2")}
                        <img src="/onboard-img/ios-safari-home.gif" alt="" class="step-shot" />
                    </div>
                </div>
                <div class="step">
                    <span class="step-num">"3"</span>
                    <div class="step-body">
                        {move || t("pwa.inst.ios_safari.3")}
                        // Последний диалог — СИСТЕМНЫЙ, один на весь iOS независимо
                        // от браузера, поэтому берётся уже снятый в яндексовском
                        // прогоне кадр, а не заводится копия того же самого.
                        <img src="/onboard-img/ios-ya-add.gif" alt="" class="step-shot" />
                    </div>
                </div>
            </div>
        }.into_view(),

        // Яндекс.Браузер на iPhone. Путь другой, чем в Safari: значок на домашний
        // экран кладётся не из системного листа «Поделиться», а через собственное
        // меню браузера, которое лишь потом открывает системный лист.
        // Снимки живого интерфейса с мигающей подсказкой — scripts/shot-ios-yandex-gifs.mjs.
        "ios_yandex" => view! {
            <div class="steps">
                <div class="step">
                    <span class="step-num">"1"</span>
                    <div class="step-body">
                        {move || t("pwa.inst.ios_yandex.1")}
                        <img src="/onboard-img/ios-ya-menu.gif" alt="" class="step-shot" />
                    </div>
                </div>
                <div class="step">
                    <span class="step-num">"2"</span>
                    <div class="step-body">
                        {move || t("pwa.inst.ios_yandex.2")}
                        <img src="/onboard-img/ios-ya-shortcut.gif" alt="" class="step-shot" />
                    </div>
                </div>
                <div class="step">
                    <span class="step-num">"3"</span>
                    <div class="step-body">
                        {move || t("pwa.inst.ios_yandex.3")}
                        <img src="/onboard-img/ios-ya-home.gif" alt="" class="step-shot" />
                    </div>
                </div>
                <div class="step">
                    <span class="step-num">"4"</span>
                    <div class="step-body">
                        {move || t("pwa.inst.ios_yandex.4")}
                        <img src="/onboard-img/ios-ya-add.gif" alt="" class="step-shot" />
                    </div>
                </div>
                <div class="step">
                    <span class="step-num">"5"</span>
                    <div class="step-body">{move || t("pwa.inst.ios_yandex.5")}</div>
                </div>
            </div>
        }.into_view(),

        // Chrome на iPhone. Прежде здесь висело «установка работает только в
        // Safari» — неправда: ярлык ставится и отсюда, просто путь свой. Кнопки
        // «Поделиться» внизу экрана, как в Safari, нет — значок стоит в правом
        // краю адресной строки, а пункт «На экран „Домой“» лежит в СОБСТВЕННОМ
        // меню Chrome, а не в системном листе. Снимки — с живого экрана.
        "ios_chrome" => view! {
            <div class="steps">
                <div class="step">
                    <span class="step-num">"1"</span>
                    <div class="step-body">
                        {move || t("pwa.inst.ios_chrome.1")}
                        <img src="/onboard-img/ios-chrome-share.gif" alt="" class="step-shot" />
                    </div>
                </div>
                <div class="step">
                    <span class="step-num">"2"</span>
                    <div class="step-body">
                        {move || t("pwa.inst.ios_chrome.2")}
                        <img src="/onboard-img/ios-chrome-add.gif" alt="" class="step-shot" />
                    </div>
                </div>
                <div class="step">
                    <span class="step-num">"3"</span>
                    <div class="step-body">
                        {move || t("pwa.inst.ios_chrome.3")}
                        // Тот же системный диалог iOS, что и в Safari.
                        <img src="/onboard-img/ios-ya-add.gif" alt="" class="step-shot" />
                    </div>
                </div>
            </div>
        }.into_view(),

        "ios_firefox" => view! {
            <div class="steps">
                <div class="step">
                    <div class="step-body has-text-warning-dark">{move || t("pwa.inst.ios_other.1")}</div>
                </div>
                <div class="step">
                    <div class="step-body">{move || t("pwa.inst.ios_other.2")}</div>
                </div>
            </div>
        }.into_view(),

        // Chrome on Android — the flow the user actually sees, illustrated with
        // screenshots of the live UI and a blinking hint on what to tap
        // (scripts/shot-yandex-hop-gifs.mjs builds them).
        "android_chrome" => view! {
            <div class="steps">
                <div class="step">
                    <span class="step-num">"1"</span>
                    <div class="step-body">
                        {move || t("pwa.inst.android_chrome.1")}
                        " "
                        // Значок обновления — прямо в строке, размером со строку,
                        // чтобы не раздвигать межстрочный интервал.
                        <img src="/onboard-img/update-icon.png" alt="значок обновления"
                             style="height: 1.05em; width: auto; vertical-align: -0.18em;" />
                        "."
                        <img src="/onboard-img/pwa-menu.gif" alt="" class="step-shot" />
                    </div>
                </div>
                <div class="step">
                    <span class="step-num">"2"</span>
                    <div class="step-body">
                        {move || t("pwa.inst.android_chrome.2")}
                        <img src="/onboard-img/pwa-addscreen.gif" alt="" class="step-shot" />
                    </div>
                </div>
                <div class="step">
                    <span class="step-num">"3"</span>
                    <div class="step-body">
                        {move || t("pwa.inst.android_chrome.3")}
                        <img src="/onboard-img/pwa-install.gif" alt="" class="step-shot" />
                    </div>
                </div>
                <div class="step">
                    <span class="step-num">"4"</span>
                    <div class="step-body">{move || t("pwa.inst.android_chrome.4")}</div>
                </div>
            </div>
        }.into_view(),

        "android_samsung" => view! {
            <div class="steps">
                <div class="step">
                    <span class="step-num">"1"</span>
                    <div class="step-body">
                        {move || t("pwa.inst.android_samsung.1")} " " <HamburgerIcon />
                    </div>
                </div>
                <div class="step">
                    <span class="step-num">"2"</span>
                    <div class="step-body">{move || t("pwa.inst.android_samsung.2")}</div>
                </div>
            </div>
        }.into_view(),

        "android_firefox" => view! {
            <div class="steps">
                <div class="step">
                    <span class="step-num">"1"</span>
                    <div class="step-body">
                        {move || t("pwa.inst.android_firefox.1")} " " <ThreeDotsIcon />
                    </div>
                </div>
                <div class="step">
                    <span class="step-num">"2"</span>
                    <div class="step-body">{move || t("pwa.inst.android_firefox.2")}</div>
                </div>
                <div class="step">
                    <span class="step-num">"3"</span>
                    <div class="step-body">{move || t("pwa.inst.android_firefox.3")}</div>
                </div>
            </div>
        }.into_view(),

        "android_yandex" => view! {
            <div class="steps">
                <div class="step">
                    <span class="step-num">"1"</span>
                    <div class="step-body">
                        {move || t("pwa.inst.android_yandex.1")} " " <ThreeDotsIcon />
                    </div>
                </div>
                <div class="step">
                    <span class="step-num">"2"</span>
                    <div class="step-body">{move || t("pwa.inst.android_yandex.2")}</div>
                </div>
            </div>
        }.into_view(),

        "macos_safari" => view! {
            <div class="steps">
                <div class="step">
                    <span class="step-num">"1"</span>
                    <div class="step-body">{move || t("pwa.inst.macos_safari.1")}</div>
                </div>
                <div class="step">
                    <span class="step-num">"2"</span>
                    <div class="step-body">{move || t("pwa.inst.macos_safari.2")}</div>
                </div>
            </div>
        }.into_view(),

        "macos_chrome" | "desktop_chrome" => view! {
            <div class="steps">
                <div class="step">
                    <span class="step-num">"1"</span>
                    <div class="step-body">
                        {move || t("pwa.inst.chrome.1")} " " <InstallIcon />
                    </div>
                </div>
                <div class="step">
                    <span class="step-num">"2"</span>
                    <div class="step-body">{move || t("pwa.inst.chrome.2")}</div>
                </div>
            </div>
        }.into_view(),

        "macos_edge" | "desktop_edge" => view! {
            <div class="steps">
                <div class="step">
                    <span class="step-num">"1"</span>
                    <div class="step-body">
                        {move || t("pwa.inst.edge.1")} " " <ThreeDotsIcon />
                    </div>
                </div>
                <div class="step">
                    <span class="step-num">"2"</span>
                    <div class="step-body">{move || t("pwa.inst.edge.2")}</div>
                </div>
            </div>
        }.into_view(),

        "macos_firefox" | "desktop_firefox" => view! {
            <div class="steps">
                <div class="step">
                    <div class="step-body has-text-warning-dark">{move || t("pwa.inst.firefox.1")}</div>
                </div>
            </div>
        }.into_view(),

        _ => view! {
            <div class="steps">
                <div class="step">
                    <span class="step-num">"1"</span>
                    <div class="step-body">
                        {move || t("pwa.inst.chrome.1")} " " <InstallIcon />
                    </div>
                </div>
                <div class="step">
                    <span class="step-num">"2"</span>
                    <div class="step-body">{move || t("pwa.inst.chrome.2")}</div>
                </div>
            </div>
        }.into_view(),
    }
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
    if platform == "unknown" {
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
                    <ChromeMark />
                    <h1 class="title is-5" style="line-height: 1.35; margin-bottom: 14px;">
                        {move || t("pwa.unknown.title")}
                    </h1>
                    <p class="has-text-grey" style="line-height: 1.6; margin-bottom: 18px;">
                        {move || t("pwa.unknown.signal")}
                    </p>
                    <p style="line-height: 1.6; margin-bottom: 18px;">
                        {move || t("pwa.unknown.chrome")}
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
                            <div style="line-height: 1.5;">{move || t("pwa.unknown.step2")}</div>
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
        <style>"
            .steps { display: flex; flex-direction: column; gap: 0.75rem; }
            .step { display: flex; align-items: flex-start; gap: 0.75rem; }
            .step-num {
                flex-shrink: 0; width: 1.75rem; height: 1.75rem;
                border-radius: 50%; background: var(--bulma-link); color: var(--bulma-link-invert);
                display: flex; align-items: center; justify-content: center;
                font-size: 0.85rem; font-weight: 600;
            }
            .step-body { font-size: 0.95rem; line-height: 1.5; padding-top: 0.15rem; }
            /* Скриншот живого интерфейса под текстом шага — мигающая подсказка
               показывает, куда нажимать. */
            .step-shot {
                display: block; width: 100%; margin-top: 0.5rem;
                border-radius: 10px; border: 1px solid var(--bulma-border);
            }
        "</style>
        <div style="min-height: 100vh; display: flex; flex-direction: column; align-items: center; justify-content: center; padding: 2rem; text-align: center; background: var(--bulma-scheme-main); overflow-y: auto;">
            <div style="max-width: 24rem;">
                <img src="/icon-192.png" alt="re:Norma" style="width: 80px; height: 80px; border-radius: 16px; margin-bottom: 1rem;" />
                <h1 class="title is-3" style="margin-bottom: 0.5rem;">"re:Norma"</h1>
                <p class="has-text-grey mb-5" style="font-size: 1.05rem; line-height: 1.6;">
                    {move || t("pwa.description")}
                </p>

                <div class="box" style="text-align: left; margin-bottom: 2rem;">
                    <p class="has-text-weight-semibold mb-4">{t(title)}</p>
                    {steps}
                </div>

                // «Продолжить в браузере» есть только на десктопе. На телефоне
                // приложение обязано стоять как PWA: браузерная вкладка на
                // Android открывается системным браузером (у части людей это
                // Яндекс, где приложение не работает), поэтому выхода из
                // инструкции нет — ставим PWA.
                {(!platform.starts_with("android") && !platform.starts_with("ios")).then(|| view! {
                    <button
                        attr:data-testid="pwa-btn-dismiss"
                        class="button is-ghost has-text-grey"
                        style="text-decoration: underline; font-size: 0.85rem;"
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
