//! Опознание браузера и пошаговые инструкции по установке PWA.
//!
//! Экраны выстраданы на живых устройствах: у Яндекса на iPhone в листе
//! «Поделиться» нет пункта «На экран „Домой“», Chrome на iPhone зовётся в UA
//! «CriOS» и без отдельной проверки получал инструкцию Safari, Mi Browser не
//! умеет ключи вовсе. Всё это здесь, в одном месте, потому что кураторское
//! приложение ставится теми же способами, и вторая копия этих правил неизбежно
//! отстала бы от первой.

use leptos::*;

pub mod icons;
use icons::*;

/// Язык инструкций.
///
/// Свой, а не заимствованный у приложения: крейт не должен знать, как каждое из
/// них хранит настройку языка, — только какой язык выбран сейчас.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Lang {
    Ru,
    En,
}

/// Текст шага на выбранном языке.
///
/// Слова живут ЗДЕСЬ, рядом с разметкой, которая их показывает. Раньше их давало
/// приложение-хозяин через свой переводчик — и кураторское приложение, заведённое
/// позже, их просто не завело: экран установки показывал «???» вместо инструкций.
/// Договор, который надо помнить, рано или поздно забывают; договора больше нет.
///
/// Приложению остаются его собственные слова — заголовок экрана, объяснение,
/// зачем ставить. Они у каждого свои. А «нажмите значок „Поделиться“ посередине
/// нижней панели» описывает Safari, и второй копии у этой фразы быть не должно.
fn step(lang: Lang, key: &str) -> &'static str {
    match lang {
        Lang::Ru => ru(key),
        Lang::En => en(key),
    }
}

fn ru(key: &str) -> &'static str {
    match key {
        "pwa.inst.ios_safari.1" => "Нажмите значок «Поделиться» посередине нижней панели.",
        "pwa.inst.ios_safari.2" => "Прокрутите список и выберите «На экран „Домой“».",
        "pwa.inst.ios_safari.3" => "Нажмите «Добавить» в правом верхнем углу. Значок появится на домашнем экране — открывайте приложение с него.",
        "pwa.inst.ios_yandex.1" => "Нажмите на три точки в адресной строке.",
        "pwa.inst.ios_yandex.2" => "Выберите «Добавить ярлык на телефон».",
        "pwa.inst.ios_yandex.3" => "Затем — «На экран „Домой“».",
        "pwa.inst.ios_yandex.4" => "Нажмите «Добавить» в правом верхнем углу.",
        "pwa.inst.ios_yandex.5" => "Значок появится на домашнем экране. Открывайте приложение с него — оно запустится отдельным окном.",
        "pwa.inst.ios_chrome.1" => "Нажмите значок «Поделиться» в правом краю адресной строки.",
        "pwa.inst.ios_chrome.2" => "В меню выберите «На экран „Домой“».",
        "pwa.inst.ios_chrome.3" => "Нажмите «Добавить» в правом верхнем углу. Значок появится на домашнем экране — открывайте приложение с него.",
        "pwa.inst.ios_other.1" => "Установка PWA на iOS работает только в Safari",
        "pwa.inst.ios_other.2" => "Откройте эту страницу в Safari и следуйте инструкции",
        "pwa.inst.android_chrome.1" => "Нажмите на кебаб — вместо него может быть значок обновления",
        "pwa.inst.android_chrome.2" => "Затем — строчка меню «Установить и создать ярлык».",
        "pwa.inst.android_chrome.3" => "Затем нажмите «Установить».",
        "pwa.inst.android_chrome.4" => "И подождите немного. Значок приложения будет показан на главном экране.",
        "pwa.inst.android_samsung.1" => "Нажмите меню \u{2261} в правом нижнем углу",
        "pwa.inst.android_samsung.2" => "Нажмите «Добавить страницу на» \u{2192} «Главный экран»",
        "pwa.inst.android_firefox.1" => "Нажмите меню \u{22ee} (три точки)",
        "pwa.inst.android_firefox.2" => "Нажмите «Установить»",
        "pwa.inst.android_firefox.3" => "Подтвердите установку",
        "pwa.inst.android_yandex.1" => "Нажмите меню \u{22ee} (три точки) в правом нижнем углу",
        "pwa.inst.android_yandex.2" => "Выберите «Добавить ярлык», затем «Добавить автоматически»",
        "pwa.inst.macos_safari.1" => "В меню: Файл \u{2192} Добавить в Dock",
        "pwa.inst.macos_safari.2" => "Приложение появится в вашем Dock",
        "pwa.inst.chrome.1" => "Нажмите значок установки в адресной строке",
        "pwa.inst.chrome.2" => "Нажмите «Установить» во всплывающем окне",
        "pwa.inst.edge.1" => "Меню \u{2026} \u{2192} Приложения \u{2192} Установить этот сайт как приложение",
        "pwa.inst.edge.2" => "Нажмите «Установить» для подтверждения",
        "pwa.inst.firefox.1" => "Firefox на компьютере не поддерживает установку PWA. Используйте Chrome, Edge или Safari.",
        _ => "???",
    }
}

fn en(key: &str) -> &'static str {
    match key {
        // iOS Safari
        "pwa.inst.ios_safari.1" => "Tap the Share icon in the middle of the bottom bar.",
        "pwa.inst.ios_safari.2" => "Scroll the list and pick \"Add to Home Screen\".",
        "pwa.inst.ios_safari.3" => "Tap \"Add\" in the top right corner. The icon appears on your home screen — open the app from there.",
        "pwa.inst.ios_yandex.1" => "Tap the three dots in the address bar.",
        "pwa.inst.ios_yandex.2" => "Choose \"Add shortcut to phone\".",
        "pwa.inst.ios_yandex.3" => "Then \"Add to Home Screen\".",
        "pwa.inst.ios_yandex.4" => "Tap \"Add\" in the top right corner.",
        "pwa.inst.ios_yandex.5" => "The icon appears on your home screen. Open the app from there — it will run in its own window.",
        // iOS Chrome/Firefox
        "pwa.inst.ios_chrome.1" => "Tap the Share icon at the right end of the address bar.",
        "pwa.inst.ios_chrome.2" => "In the menu, pick \"Add to Home Screen\".",
        "pwa.inst.ios_chrome.3" => "Tap \"Add\" in the top right corner. The icon appears on your home screen — open the app from there.",
        "pwa.inst.ios_other.1" => "PWA install is only supported in Safari on iOS",
        "pwa.inst.ios_other.2" => "Open this page in Safari and follow the instructions",
        // Android Chrome
        "pwa.inst.android_chrome.1" => "Tap the kebab menu — an update icon may be shown in its place",
        "pwa.inst.android_chrome.2" => "Then the menu row \"Install and create a shortcut\".",
        "pwa.inst.android_chrome.3" => "Then tap \"Install\".",
        "pwa.inst.android_chrome.4" => "And wait a little. The app icon will appear on the home screen.",
        // Android Samsung
        "pwa.inst.android_samsung.1" => "Tap the menu \u{2261} at the bottom right",
        "pwa.inst.android_samsung.2" => "Tap \"Add page to\" \u{2192} \"Home screen\"",
        // Android Firefox
        "pwa.inst.android_firefox.1" => "Tap the menu \u{22ee} (three dots)",
        "pwa.inst.android_firefox.2" => "Tap \"Install\"",
        "pwa.inst.android_firefox.3" => "Confirm the installation",
        // Android Yandex
        "pwa.inst.android_yandex.1" => "Tap the menu \u{22ee} (three dots) at the bottom right",
        "pwa.inst.android_yandex.2" => "Tap \"Add shortcut\", then \"Add automatically\"",
        // System-browser hop screen (Android browsers that can't install a PWA).
        // macOS Safari
        "pwa.inst.macos_safari.1" => "In the menu bar: File \u{2192} Add to Dock",
        "pwa.inst.macos_safari.2" => "The app will appear in your Dock",
        // Chrome (desktop & macOS)
        "pwa.inst.chrome.1" => "Click the install icon in the address bar",
        "pwa.inst.chrome.2" => "Click \"Install\" in the popup",
        // Edge
        "pwa.inst.edge.1" => "Click the \u{2026} menu \u{2192} Apps \u{2192} Install this site as an app",
        "pwa.inst.edge.2" => "Click \"Install\" to confirm",
        // Firefox desktop
        "pwa.inst.firefox.1" => "Firefox desktop does not support PWA install. Use Chrome, Edge, or Safari.",
        _ => "???",
    }
}

pub fn detect_platform() -> &'static str {
    let ua = web_sys::window()
        .and_then(|w| w.navigator().user_agent().ok())
        .unwrap_or_default();
    detect_platform_from_ua(&ua)
}

/// Та же развилка, но от ЗАДАННОЙ строки UA — чтобы её можно было проверить
/// тестом, а не только живым телефоном.
pub fn detect_platform_from_ua(ua: &str) -> &'static str {
    let ua = ua.to_lowercase();

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
    // Неопознанный браузер на iPhone — СВОЙ исход, а не «сойдёт инструкция Safari».
    // Внутри там у всех WebKit, и сафаревская подсказка чаще всего работает, но
    // именно «чаще всего»: у Яндекса на айфоне пункта «На экран „Домой“» в листе
    // «Поделиться» нет вовсе, и мы завели ему отдельный экран. Значит найдётся и
    // следующий такой — пусть получает честное «мы не умеем», а не чужую инструкцию.
    if is_ios { return "ios_unknown"; }

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

/// Заголовок экрана по платформе.
pub fn title_key(platform: &str) -> &'static str {
    match platform {
        s if s.starts_with("ios") => "pwa.title.ios",
        s if s.starts_with("android") => "pwa.title.android",
        s if s.starts_with("macos") => "pwa.title.macos",
        _ => "pwa.title.desktop",
    }
}

/// Пошаговая инструкция для платформы.
///
/// `lang` — ФУНКЦИЯ, а не значение: замыкания ниже перечитывают её на каждой
/// перерисовке, поэтому смена языка в настройках меняет и шаги. Значение
/// заморозило бы их до перезагрузки страницы.
pub fn render_steps(platform: &str, lang: fn() -> Lang) -> View {
    // Локальный переводчик: дальше по тексту `t("…")` читается ровно так же, как
    // читалось, когда его давало приложение.
    let t = move |key: &'static str| step(lang(), key);
    // Оформление — здесь же, а не у хозяина. Раньше оно лежало в `<style>` внутри
    // экрана приложения худеющего, и кураторское приложение, звавшее эту же
    // функцию, получало разметку без единого правила: номер и текст вставали
    // столбиком вместо строки. Забыть его теперь физически нельзя.
    //
    // Цвета берутся вложенными фолбэками: приложения живут в разных палитрах, и
    // одна и та же разметка обязана попасть в обе. Куратор определяет `--accent`
    // и `--line`, приложение худеющего — нет, и проваливается в свои `--bulma-*`.
    let styles = view! {
        <style>"
            .steps { display: flex; flex-direction: column; gap: 0.75rem; }
            .step { display: flex; align-items: flex-start; gap: 0.75rem; }
            .step-num {
                flex-shrink: 0; width: 1.75rem; height: 1.75rem;
                border-radius: 50%;
                background: var(--accent, var(--bulma-link, #0F9E70));
                color: var(--accent-ink, var(--bulma-link-invert, #FFFFFF));
                display: flex; align-items: center; justify-content: center;
                font-size: 0.85rem; font-weight: 600;
            }
            .step-body { font-size: 0.95rem; line-height: 1.5; padding-top: 0.15rem; }
            /* Скриншот живого интерфейса под текстом шага — мигающая подсказка
               показывает, куда нажимать. */
            .step-shot {
                display: block; width: 100%; margin-top: 0.5rem;
                border-radius: 10px;
                border: 1px solid var(--line, var(--bulma-border, #DFE4EA));
            }
        "</style>
    };
    let steps = match platform {
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
                        <img src="/pwa-img/ios-safari-share.gif" alt="" class="step-shot" />
                    </div>
                </div>
                <div class="step">
                    <span class="step-num">"2"</span>
                    <div class="step-body">
                        {move || t("pwa.inst.ios_safari.2")}
                        <img src="/pwa-img/ios-safari-home.gif" alt="" class="step-shot" />
                    </div>
                </div>
                <div class="step">
                    <span class="step-num">"3"</span>
                    <div class="step-body">
                        {move || t("pwa.inst.ios_safari.3")}
                        // Последний диалог — СИСТЕМНЫЙ, один на весь iOS независимо
                        // от браузера, поэтому берётся уже снятый в яндексовском
                        // прогоне кадр, а не заводится копия того же самого.
                        <img src="/pwa-img/ios-ya-add.gif" alt="" class="step-shot" />
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
                        <img src="/pwa-img/ios-ya-menu.gif" alt="" class="step-shot" />
                    </div>
                </div>
                <div class="step">
                    <span class="step-num">"2"</span>
                    <div class="step-body">
                        {move || t("pwa.inst.ios_yandex.2")}
                        <img src="/pwa-img/ios-ya-shortcut.gif" alt="" class="step-shot" />
                    </div>
                </div>
                <div class="step">
                    <span class="step-num">"3"</span>
                    <div class="step-body">
                        {move || t("pwa.inst.ios_yandex.3")}
                        <img src="/pwa-img/ios-ya-home.gif" alt="" class="step-shot" />
                    </div>
                </div>
                <div class="step">
                    <span class="step-num">"4"</span>
                    <div class="step-body">
                        {move || t("pwa.inst.ios_yandex.4")}
                        <img src="/pwa-img/ios-ya-add.gif" alt="" class="step-shot" />
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
                        <img src="/pwa-img/ios-chrome-share.gif" alt="" class="step-shot" />
                    </div>
                </div>
                <div class="step">
                    <span class="step-num">"2"</span>
                    <div class="step-body">
                        {move || t("pwa.inst.ios_chrome.2")}
                        <img src="/pwa-img/ios-chrome-add.gif" alt="" class="step-shot" />
                    </div>
                </div>
                <div class="step">
                    <span class="step-num">"3"</span>
                    <div class="step-body">
                        {move || t("pwa.inst.ios_chrome.3")}
                        // Тот же системный диалог iOS, что и в Safari.
                        <img src="/pwa-img/ios-ya-add.gif" alt="" class="step-shot" />
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
                        <img src="/pwa-img/update-icon.png" alt="значок обновления"
                             style="height: 1.05em; width: auto; vertical-align: -0.18em;" />
                        "."
                        <img src="/pwa-img/pwa-menu.gif" alt="" class="step-shot" />
                    </div>
                </div>
                <div class="step">
                    <span class="step-num">"2"</span>
                    <div class="step-body">
                        {move || t("pwa.inst.android_chrome.2")}
                        <img src="/pwa-img/pwa-addscreen.gif" alt="" class="step-shot" />
                    </div>
                </div>
                <div class="step">
                    <span class="step-num">"3"</span>
                    <div class="step-body">
                        {move || t("pwa.inst.android_chrome.3")}
                        <img src="/pwa-img/pwa-install.gif" alt="" class="step-shot" />
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
    };
    view! { {styles} {steps} }.into_view()
}

#[cfg(test)]
mod tests {
    use super::detect_platform_from_ua;

    // Настоящие строки UA с живых устройств — те самые случаи, из-за которых
    // развилка выглядит именно так.

    const IOS_SAFARI: &str = "Mozilla/5.0 (iPhone; CPU iPhone OS 17_5 like Mac OS X) \
        AppleWebKit/605.1.15 (KHTML, like Gecko) Version/17.5 Mobile/15E148 Safari/604.1";
    const IOS_CHROME: &str = "Mozilla/5.0 (iPhone; CPU iPhone OS 17_5 like Mac OS X) \
        AppleWebKit/605.1.15 (KHTML, like Gecko) CriOS/126.0.6478.54 Mobile/15E148 Safari/604.1";
    const IOS_YANDEX: &str = "Mozilla/5.0 (iPhone; CPU iPhone OS 17_5 like Mac OS X) \
        AppleWebKit/605.1.15 (KHTML, like Gecko) Version/17.5 YaBrowser/24.6.0 Mobile/15E148 Safari/604.1";
    const ANDROID_CHROME: &str = "Mozilla/5.0 (Linux; Android 14; Pixel 7) AppleWebKit/537.36 \
        (KHTML, like Gecko) Chrome/126.0.0.0 Mobile Safari/537.36";
    const ANDROID_MI: &str = "Mozilla/5.0 (Linux; U; Android 15; Redmi) AppleWebKit/537.36 \
        (KHTML, like Gecko) Version/4.0 Chrome/122.0.0.0 Mobile Safari/537.36 XiaoMi/MiuiBrowser/14.60";
    const ANDROID_SAMSUNG: &str = "Mozilla/5.0 (Linux; Android 14; SM-S918B) AppleWebKit/537.36 \
        (KHTML, like Gecko) SamsungBrowser/25.0 Chrome/121.0.0.0 Mobile Safari/537.36";
    const ANDROID_YA_APP: &str = "Mozilla/5.0 (Linux; Android 13; SM-A536E) AppleWebKit/537.36 \
        (KHTML, like Gecko) Chrome/119.0.0.0 Mobile Safari/537.36 YaApp_Android/24.55";

    #[test]
    fn safari_na_ajfone() {
        assert_eq!(detect_platform_from_ua(IOS_SAFARI), "ios_safari");
    }

    /// Chrome на iPhone зовётся «CriOS», слова «chrome» в его UA нет вовсе. Без
    /// отдельной проверки он подходил под правило Safari и человек получал чужую
    /// инструкцию: «кнопка „Поделиться“ внизу», которой у Chrome нет.
    #[test]
    fn chrome_na_ajfone_ne_safari() {
        assert_eq!(detect_platform_from_ua(IOS_CHROME), "ios_chrome");
    }

    /// У Яндекса на iPhone в UA есть и «safari», и «version/», поэтому он
    /// проверяется ПЕРВЫМ: пункта «На экран „Домой“» в его листе «Поделиться»
    /// нет, и сафаревская инструкция там не работает.
    #[test]
    fn yandex_na_ajfone_pervee_safari() {
        assert_eq!(detect_platform_from_ua(IOS_YANDEX), "ios_yandex");
    }

    #[test]
    fn chrome_i_samsung_na_androide() {
        assert_eq!(detect_platform_from_ua(ANDROID_CHROME), "android_chrome");
        assert_eq!(detect_platform_from_ua(ANDROID_SAMSUNG), "android_samsung");
    }

    /// В UA у Mi Browser есть «chrome», и без своей проверки он уходил бы в
    /// ветку Chrome — а ключи он не умеет вовсе.
    #[test]
    fn mi_browser_ne_chrome() {
        assert_eq!(detect_platform_from_ua(ANDROID_MI), "android_mi");
    }

    /// Встроенный браузер приложения Яндекса не несёт «yabrowser», зато несёт
    /// «chrome» — без явного правила он бы тоже стал Chrome.
    #[test]
    fn vstroennyj_brauzer_yandeksa() {
        assert_eq!(detect_platform_from_ua(ANDROID_YA_APP), "android_yandex");
    }

    /// Неопознанный браузер — СВОЙ исход, а не «сойдёт чужая инструкция».
    #[test]
    fn neopoznannoe_govorit_o_sebe_chestno() {
        assert_eq!(
            detect_platform_from_ua("Mozilla/5.0 (iPhone) SomethingElse/1.0"),
            "ios_unknown"
        );
        assert_eq!(
            detect_platform_from_ua("Mozilla/5.0 (Linux; Android 14) WeirdBrowser/1.0"),
            "unknown"
        );
        assert_eq!(detect_platform_from_ua(""), "unknown");
    }
}
