//! Строки кураторского приложения. Две таблицы, как в приложении худеющего, и по
//! той же причине: язык — настройка человека, а не сборки.
//!
//! Ключи установки PWA (`pwa.*`) совпадают с ключами приложения худеющего:
//! экраны общие (крейт `pwa-prompt`), и переводит их каждое приложение своей
//! таблицей.

use std::cell::Cell;

use leptos::{create_rw_signal, RwSignal, SignalGet, SignalSet};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Lang {
    Ru,
    En,
}

const KEY_LANG: &str = "curator_lang";

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
        _ => Lang::Ru,
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

/// Перевод по ключу. Отсутствующий ключ возвращает «???» — так же, как в
/// приложении худеющего: пропажу видно, и она не притворяется пустотой.
pub fn t(key: &str) -> &'static str {
    match get() {
        Lang::Ru => ru(key),
        Lang::En => en(key),
    }
}

fn ru(key: &str) -> &'static str {
    match key {
        // Вход и установка
        "app.title" => "Куратор",
        "install.title" => "Установите приложение",
        "install.body" => "Кабинет куратора работает как приложение на телефоне. Поставьте его на домашний экран — иначе не будет ни ключа для входа, ни уведомлений о сообщениях.",
        "login.title" => "Кабинет куратора",
        "login.sub" => "Вход по ключу этого устройства",
        "login.enter" => "Войти",
        "login.first_time" => "Первый вход на этом устройстве",
        "login.name" => "Как вас зовут",
        "login.register" => "Создать ключ",
        "login.name_required" => "Введите имя — его увидят ваши клиенты",

        // Клиенты
        "clients.title" => "Клиенты",
        "clients.empty" => "Пока никого. Добавьте первого клиента и отправьте ему ссылку.",
        "clients.add" => "Добавить клиента",
        "clients.add_name" => "Имя клиента",
        "clients.add_hint" => "Это имя видите только вы.",
        "clients.create" => "Добавить",
        "clients.cancel" => "Отмена",
        "clients.pending" => "ждёт согласия",
        "clients.copy_link" => "Скопировать ссылку",
        "clients.copied" => "Ссылка скопирована",

        // Клиент
        "client.invite_title" => "Пригласите человека",
        "client.invite_body" => "Отправьте эту ссылку любым удобным способом. Он откроет её на телефоне и подтвердит согласие.",
        "client.no_report" => "Данных ещё нет. Запросите их — человек увидит просьбу в приложении.",
        "client.request" => "Запросить данные",
        "client.request_days" => "дней",
        "client.requested" => "Запрос отправлен",
        "client.waiting" => "Ждём ответа с {date}",
        "client.report_at" => "Отчёт от {date}",
        "client.chat" => "Переписка",
        "client.unbind" => "Прекратить работу",
        "client.delete" => "Удалить клиента",
        "client.unbind_confirm" => "Прекратить работу с этим человеком? Он получит сообщение, а вы сможете пригласить его снова.",
        "client.delete_confirm" => "Удалить клиента из списка? Переписка и отчёт пропадут.",

        // Планки
        "planka.edit" => "Планка",
        "planka.value" => "Значение",
        "planka.calc" => "Рассчитать",
        "planka.calc_hint" => "По последним данным клиента — то же число, к которому пришло бы его приложение само.",
        "planka.save" => "Применить",

        // Чат
        "chat.placeholder" => "Сообщение",
        "chat.send" => "Отправить",
        "chat.empty" => "Переписки пока нет.",

        // Настройки
        "settings.title" => "Настройки",
        "settings.name" => "Имя",
        "settings.name_hint" => "Его видят ваши клиенты в приглашении и в переписке.",
        "settings.lang" => "Язык",
        "settings.save" => "Сохранить",
        "settings.saved" => "Сохранено",
        "settings.logout" => "Выйти",

        "common.back" => "Назад",
        "common.retry" => "Повторить",
        "common.loading" => "Загрузка…",
        _ => "???",
    }
}

fn en(key: &str) -> &'static str {
    match key {
        "app.title" => "Curator",
        "install.title" => "Install the app",
        "install.body" => "The curator workspace runs as an app on your phone. Add it to your home screen — otherwise there is no key to sign in with and no notifications about messages.",
        "login.title" => "Curator workspace",
        "login.sub" => "Sign in with this device's key",
        "login.enter" => "Sign in",
        "login.first_time" => "First time on this device",
        "login.name" => "Your name",
        "login.register" => "Create a key",
        "login.name_required" => "Enter your name — your clients will see it",

        "clients.title" => "Clients",
        "clients.empty" => "No one yet. Add your first client and send them the link.",
        "clients.add" => "Add client",
        "clients.add_name" => "Client name",
        "clients.add_hint" => "Only you see this name.",
        "clients.create" => "Add",
        "clients.cancel" => "Cancel",
        "clients.pending" => "awaiting consent",
        "clients.copy_link" => "Copy link",
        "clients.copied" => "Link copied",

        "client.invite_title" => "Invite this person",
        "client.invite_body" => "Send them this link any way you like. They open it on their phone and confirm.",
        "client.no_report" => "No data yet. Ask for it — they will see the request in their app.",
        "client.request" => "Request data",
        "client.request_days" => "days",
        "client.requested" => "Request sent",
        "client.waiting" => "Waiting since {date}",
        "client.report_at" => "Report from {date}",
        "client.chat" => "Messages",
        "client.unbind" => "Stop working together",
        "client.delete" => "Delete client",
        "client.unbind_confirm" => "Stop working with this person? They will be told, and you can invite them again.",
        "client.delete_confirm" => "Remove this client from the list? The conversation and the report will be gone.",

        "planka.edit" => "Target",
        "planka.value" => "Value",
        "planka.calc" => "Calculate",
        "planka.calc_hint" => "From the client's latest data — the same figure their app would have arrived at on its own.",
        "planka.save" => "Apply",

        "chat.placeholder" => "Message",
        "chat.send" => "Send",
        "chat.empty" => "No messages yet.",

        "settings.title" => "Settings",
        "settings.name" => "Name",
        "settings.name_hint" => "Your clients see it in the invitation and in the chat.",
        "settings.lang" => "Language",
        "settings.save" => "Save",
        "settings.saved" => "Saved",
        "settings.logout" => "Sign out",

        "common.back" => "Back",
        "common.retry" => "Retry",
        "common.loading" => "Loading…",
        _ => "???",
    }
}

#[cfg(test)]
mod tests {
    use super::{en, ru};

    /// Обе таблицы обязаны знать один и тот же набор ключей: язык — настройка
    /// человека, и пропажа в одной таблице означала бы «???» ровно у половины.
    #[test]
    fn tablicy_soglasovany() {
        for key in [
            "app.title", "install.title", "install.body", "login.title", "login.sub",
            "login.enter", "login.first_time", "login.name", "login.register",
            "login.name_required", "clients.title", "clients.empty", "clients.add",
            "clients.add_name", "clients.add_hint", "clients.create", "clients.cancel",
            "clients.pending", "clients.copy_link", "clients.copied", "client.invite_title",
            "client.invite_body", "client.no_report", "client.request", "client.request_days",
            "client.requested", "client.waiting", "client.report_at", "client.chat",
            "client.unbind", "client.delete", "client.unbind_confirm", "client.delete_confirm",
            "planka.edit", "planka.value", "planka.calc", "planka.calc_hint", "planka.save",
            "chat.placeholder",
            "chat.send", "chat.empty", "settings.title", "settings.name", "settings.name_hint",
            "settings.lang", "settings.save", "settings.saved", "settings.logout",
            "common.back", "common.retry", "common.loading",
        ] {
            assert_ne!(ru(key), "???", "нет русской строки для {key}");
            assert_ne!(en(key), "???", "нет английской строки для {key}");
        }
    }
}
