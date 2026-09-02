//! Строки приложения тренировок. Две таблицы, как у остальных приложений, и по
//! той же причине: язык — настройка человека, а не сборки.
//!
//! Слов ПОШАГОВОЙ ИНСТРУКЦИИ здесь нет и быть не должно: шаги рисует общий крейт
//! `pwa-prompt`, и слова живут там же, рядом со своей разметкой. Здесь — только
//! то, что у этого приложения своё.

use std::cell::Cell;

use leptos::{create_rw_signal, RwSignal, SignalGet, SignalSet};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Lang {
    Ru,
    En,
}

const KEY_LANG: &str = "gym_lang";

thread_local! {
    static LANG: Cell<Option<RwSignal<Lang>>> = const { Cell::new(None) };
}

fn stored() -> Lang {
    match web_sys::window()
        .and_then(|w| w.local_storage().ok().flatten())
        .and_then(|s| s.get_item(KEY_LANG).ok().flatten())
        .as_deref()
    {
        Some("en") => Lang::En,
        Some("ru") => Lang::Ru,
        // Настройки нет — берём язык браузера. Приложение русскоязычное по
        // умолчанию, английский достаётся тем, у кого система не на русском.
        _ => match web_sys::window()
            .map(|w| w.navigator().language().unwrap_or_default())
            .unwrap_or_default()
        {
            l if l.starts_with("ru") => Lang::Ru,
            l if l.is_empty() => Lang::Ru,
            _ => Lang::En,
        },
    }
}

pub fn init() {
    LANG.with(|c| c.set(Some(create_rw_signal(stored()))));
}

fn signal() -> RwSignal<Lang> {
    LANG.with(|c| c.get().expect("i18n::init() must run first"))
}

pub fn get() -> Lang {
    signal().get()
}

pub fn set(lang: Lang) {
    signal().set(lang);
    if let Some(s) = web_sys::window().and_then(|w| w.local_storage().ok().flatten()) {
        let _ = s.set_item(KEY_LANG, if lang == Lang::En { "en" } else { "ru" });
    }
}

/// Язык для шагов общего крейта.
pub fn pwa_lang() -> pwa_prompt::Lang {
    match get() {
        Lang::En => pwa_prompt::Lang::En,
        Lang::Ru => pwa_prompt::Lang::Ru,
    }
}

/// Перевод по ключу. Отсутствующий ключ возвращает «???» — пропажу видно, и она
/// не притворяется пустотой.
pub fn t(key: &str) -> &'static str {
    match get() {
        Lang::Ru => ru(key),
        Lang::En => en(key),
    }
}

fn ru(key: &str) -> &'static str {
    match key {
        // ── Вход ──
        "login.title" => "Тренировки re:Norma",
        "login.sub" => "Войдите тем же ключом, что и в приложение питания — аккаунт и подписка общие.",
        "login.enter" => "Войти ключом",
        "login.working" => "Заходим…",
        "login.no_key" => "Ключа ещё нет?",
        "login.register" => "Завести новый",
        "login.name" => "Как вас зовут",
        "login.name_hint" => "Это имя увидит система, когда спросит ключ.",
        "login.name_required" => "Введите имя",
        "login.create" => "Создать ключ",
        "login.back" => "Назад ко входу",

        // ── Подписка ──
        "checking.title" => "Проверяем подписку",
        "locked.title" => "Нужна подписка re:Norma",
        "locked.body" => "Тренировки входят в ту же подписку, что и приложение питания. \
                          Оформите её в приложении питания или у нашего бота — доступ откроется здесь сразу.",
        "locked.relogin" => "Войти другим ключом",
        "offline.title" => "Нет связи",
        "offline.body" => "Не получается проверить подписку. Проверьте интернет — как только связь появится, продолжим сами.",
        "offline.retry" => "Проверить снова",

        // ── Установка ──
        // Заголовок инструкции. Слова общего крейта `pwa-prompt` — только ШАГИ;
        // ключ заголовка он отдаёт (`title_key`), а сам заголовок каждое
        // приложение пишет своими словами.
        "pwa.title.ios" => "Как поставить на iPhone",
        "pwa.title.android" => "Как поставить на Android",
        "pwa.title.macos" => "Как поставить на Mac",
        "pwa.title.desktop" => "Как поставить на компьютер",
        "install.title" => "Поставьте приложение",
        "install.body" => "Тренировки живут на домашнем экране: так они открываются в один тап и работают как обычное приложение, а не как вкладка.",
        "install.desktop_lead" => "Это приложение для телефона.",
        "install.desktop_phone" => "Если вы сейчас с телефона — в браузере включена «Версия для ПК». Выключите её и откройте страницу снова.",
        "install.desktop_pc" => "Если вы за компьютером — откройте gym.renorma.app на телефоне. Здесь можно просто осмотреться.",
        "install.desktop_continue" => "Продолжить в браузере",
        "install.done_title" => "Приложение поставлено",
        "install.done_body" => "Дальше открывайте его с иконки на домашнем экране.",
        "install.done_wait" => "Иконка появляется не мгновенно: телефон дособирает приложение уже после того, как страница отчиталась об установке. Подождите полминуты.",
        "install.done_missing" => "Иконки так и нет?",
        "install.done_show" => "Показать инструкцию заново",

        // ── Тупиковые браузеры ──
        "dead.mi.title" => "Откройте приложение в Chrome",
        "dead.mi.open" => "Открыть в Chrome",
        "dead.yandex.title" => "Здесь приложение не заработает",
        "dead.yandex.lead" => "В Яндекс.Браузере нельзя ни создать ключ, ни поставить приложение. Перенесите страницу в Chrome — это два действия.",
        "dead.yandex.step1" => "Нажмите «Поделиться» в меню браузера.",
        "dead.yandex.step2" => "В списке выберите Chrome.",
        "dead.unknown.title" => "Незнакомый браузер",
        "dead.unknown.signal" => "Мы не знаем этот браузер и не ручаемся, что приложение в нём заработает: ключ там может не создаться, а приложение — не поставиться.",
        "dead.unknown.chrome" => "Откройте приложение в Chrome — там всё работает.",
        "dead.unknown.safari" => "Откройте приложение в Safari — на iPhone приложение ставится только оттуда.",
        "dead.unknown.step1" => "Скопируйте адрес — нажмите на него:",
        "dead.unknown.copied" => "Скопировано",
        "dead.unknown.step2" => "Откройте Chrome и вставьте адрес в строку поиска.",
        "dead.unknown.step2_safari" => "Откройте Safari и вставьте адрес в строку поиска.",
        "dead.unknown.step3" => "Дальше приложение подскажет само.",

        // ── Настройки ──
        "set.title" => "Настройки",
        "set.back" => "‹ Назад",
        "set.language" => "Язык",
        "set.version" => "Версия",
        "set.version_current" => "Установлена последняя версия",
        "set.version_available" => "Доступно обновление",
        "set.version_update" => "Обновить",
        "set.version_check" => "Проверить",
        "set.version_checking" => "Проверяем…",
        "set.account" => "Аккаунт",
        "set.key_add" => "Добавить ключ на это устройство",
        "set.key_adding" => "Создаём ключ…",
        "set.key_added" => "Ключ добавлен. Теперь на этом устройстве можно входить им.",
        "set.key_hint" => "Ключ привязан к устройству. На новом телефоне добавьте свой — или войдите фразой восстановления.",
        "set.phrase" => "Фраза восстановления",
        "set.phrase_desc" => "Пять слов, которыми можно войти без ключа — если телефон потерян или сломан. Фраза общая с приложением питания: новая заменит старую в обоих.",
        "set.phrase_warning" => "Запишите её и храните отдельно от телефона. Кто знает фразу — войдёт в аккаунт.",
        "set.phrase_generate" => "Придумать фразу",
        "set.phrase_regenerate" => "Придумать новую",
        "set.phrase_generating" => "Придумываем…",
        "set.phrase_failed" => "Не удалось придумать фразу, попробуйте ещё раз",
        "set.logout" => "Выйти",
        "set.logout_confirm" => "Выйти из аккаунта? Чтобы вернуться, понадобится ключ или фраза восстановления.",

        // ── Заглушка ──
        "stub.title" => "Тренировки скоро",
        "stub.body" => "Вход, подписка и установка готовы. Сами тренировки — журнал подходов, справочник упражнений и программы — появятся здесь следующим обновлением.",
        "stub.signed_as" => "Вы вошли",
        "stub.logout" => "Выйти",

        _ => "???",
    }
}

fn en(key: &str) -> &'static str {
    match key {
        "login.title" => "re:Norma workouts",
        "login.sub" => "Sign in with the same key you use for the nutrition app — the account and the subscription are shared.",
        "login.enter" => "Sign in with your key",
        "login.working" => "Signing in…",
        "login.no_key" => "No key yet?",
        "login.register" => "Create one",
        "login.name" => "Your name",
        "login.name_hint" => "The system shows this name when it asks for the key.",
        "login.name_required" => "Enter a name",
        "login.create" => "Create key",
        "login.back" => "Back to sign in",

        "checking.title" => "Checking your subscription",
        "locked.title" => "A re:Norma subscription is required",
        "locked.body" => "Workouts are part of the same subscription as the nutrition app. \
                          Subscribe in the nutrition app or via our bot — access opens here immediately.",
        "locked.relogin" => "Sign in with a different key",
        "offline.title" => "No connection",
        "offline.body" => "We can't check the subscription. Check your internet — we'll continue by ourselves once it's back.",
        "offline.retry" => "Try again",

        "pwa.title.ios" => "How to install on iPhone",
        "pwa.title.android" => "How to install on Android",
        "pwa.title.macos" => "How to install on Mac",
        "pwa.title.desktop" => "How to install on your computer",
        "install.title" => "Install the app",
        "install.body" => "Workouts belong on your home screen: one tap to open, and it behaves like a real app instead of a browser tab.",
        "install.desktop_lead" => "This is a phone app.",
        "install.desktop_phone" => "If you're on a phone, your browser has \"Desktop site\" turned on. Turn it off and reload this page.",
        "install.desktop_pc" => "If you're on a computer, open gym.renorma.app on your phone. Here you can just look around.",
        "install.desktop_continue" => "Continue in the browser",
        "install.done_title" => "The app is installed",
        "install.done_body" => "From now on open it from the icon on your home screen.",
        "install.done_wait" => "The icon doesn't appear instantly: your phone finishes building the app after the page reports the install. Give it half a minute.",
        "install.done_missing" => "Still no icon?",
        "install.done_show" => "Show the instructions again",

        "dead.mi.title" => "Open the app in Chrome",
        "dead.mi.open" => "Open in Chrome",
        "dead.yandex.title" => "The app won't work here",
        "dead.yandex.lead" => "Yandex Browser can neither create a key nor install the app. Move the page to Chrome — it takes two steps.",
        "dead.yandex.step1" => "Tap \"Share\" in the browser menu.",
        "dead.yandex.step2" => "Pick Chrome from the list.",
        "dead.unknown.title" => "Unfamiliar browser",
        "dead.unknown.signal" => "We don't know this browser and can't promise the app works in it: the key may not be created and the app may not install.",
        "dead.unknown.chrome" => "Open the app in Chrome — everything works there.",
        "dead.unknown.safari" => "Open the app in Safari — on iPhone the app can only be installed from there.",
        "dead.unknown.step1" => "Copy the address — tap it:",
        "dead.unknown.copied" => "Copied",
        "dead.unknown.step2" => "Open Chrome and paste the address into the search bar.",
        "dead.unknown.step2_safari" => "Open Safari and paste the address into the search bar.",
        "dead.unknown.step3" => "The app takes it from there.",

        // ── Settings ──
        "set.title" => "Settings",
        "set.back" => "‹ Back",
        "set.language" => "Language",
        "set.version" => "Version",
        "set.version_current" => "You're on the latest version",
        "set.version_available" => "An update is available",
        "set.version_update" => "Update",
        "set.version_check" => "Check",
        "set.version_checking" => "Checking…",
        "set.account" => "Account",
        "set.key_add" => "Add a key on this device",
        "set.key_adding" => "Creating the key…",
        "set.key_added" => "Key added. You can now sign in with it on this device.",
        "set.key_hint" => "A key belongs to one device. On a new phone add its own — or sign in with the recovery phrase.",
        "set.phrase" => "Recovery phrase",
        "set.phrase_desc" => "Five words that sign you in without a key — if the phone is lost or broken. The phrase is shared with the nutrition app: a new one replaces the old in both.",
        "set.phrase_warning" => "Write it down and keep it away from the phone. Anyone who knows the phrase can sign in.",
        "set.phrase_generate" => "Create a phrase",
        "set.phrase_regenerate" => "Create a new one",
        "set.phrase_generating" => "Creating…",
        "set.phrase_failed" => "Couldn't create a phrase, try again",
        "set.logout" => "Sign out",
        "set.logout_confirm" => "Sign out? You'll need your key or the recovery phrase to come back.",

        "stub.title" => "Workouts are coming",
        "stub.body" => "Sign-in, subscription and install are ready. The workouts themselves — a set log, an exercise catalogue and programmes — arrive in the next update.",
        "stub.signed_as" => "Signed in",
        "stub.logout" => "Sign out",

        _ => "???",
    }
}
