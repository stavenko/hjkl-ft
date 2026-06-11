use std::cell::Cell;
use leptos::*;

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Lang {
    En,
    Ru,
}

const KEY_LANG: &str = "app_lang";
const KEY_WEIGHT_UNIT: &str = "weight_unit";

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum WeightUnit {
    Kg,
    Lbs,
}

impl WeightUnit {
    pub fn to_kg(self, value: f64) -> f64 {
        match self {
            WeightUnit::Kg => value,
            WeightUnit::Lbs => value * 0.45359237,
        }
    }

    pub fn from_kg(self, kg: f64) -> f64 {
        match self {
            WeightUnit::Kg => kg,
            WeightUnit::Lbs => kg / 0.45359237,
        }
    }
}

fn stored_weight_unit() -> WeightUnit {
    web_sys::window()
        .and_then(|w| w.local_storage().ok().flatten())
        .and_then(|s| s.get_item(KEY_WEIGHT_UNIT).ok().flatten())
        .map(|v| if v == "lbs" { WeightUnit::Lbs } else { WeightUnit::Kg })
        .unwrap_or(WeightUnit::Kg)
}

thread_local! {
    static WEIGHT_UNIT_SIGNAL: Cell<Option<RwSignal<WeightUnit>>> = const { Cell::new(None) };
}

pub fn init_weight_unit() {
    let sig = create_rw_signal(stored_weight_unit());
    WEIGHT_UNIT_SIGNAL.with(|c| c.set(Some(sig)));
}

pub fn weight_unit_signal() -> RwSignal<WeightUnit> {
    WEIGHT_UNIT_SIGNAL.with(|c| c.get().expect("weight_unit not initialized"))
}

pub fn set_weight_unit(unit: WeightUnit) {
    weight_unit_signal().set(unit);
    if let Some(storage) = web_sys::window()
        .and_then(|w| w.local_storage().ok().flatten())
    {
        let val = match unit { WeightUnit::Kg => "kg", WeightUnit::Lbs => "lbs" };
        storage.set_item(KEY_WEIGHT_UNIT, val).expect("write weight_unit");
    }
}

fn stored_lang() -> Lang {
    web_sys::window()
        .and_then(|w| w.local_storage().ok().flatten())
        .and_then(|s| s.get_item(KEY_LANG).ok().flatten())
        .map(|v| if v == "en" { Lang::En } else { Lang::Ru })
        .unwrap_or(Lang::Ru)
}

thread_local! {
    static LANG_SIGNAL: Cell<Option<RwSignal<Lang>>> = const { Cell::new(None) };
}

pub fn init_lang() {
    let sig = create_rw_signal(stored_lang());
    LANG_SIGNAL.with(|c| c.set(Some(sig)));
}

fn lang_signal() -> RwSignal<Lang> {
    LANG_SIGNAL.with(|c| c.get().expect("i18n not initialized"))
}

pub fn set_lang(lang: Lang) {
    lang_signal().set(lang);
    if let Some(storage) = web_sys::window()
        .and_then(|w| w.local_storage().ok().flatten())
    {
        let val = match lang { Lang::En => "en", Lang::Ru => "ru" };
        storage.set_item(KEY_LANG, val).expect("write lang");
    }
}

pub fn get_lang() -> Lang {
    lang_signal().get()
}

pub fn t(key: &str) -> &'static str {
    match lang_signal().get() {
        Lang::En => en(key),
        Lang::Ru => ru(key),
    }
}

pub fn nutrient_name(key: &str) -> &'static str {
    match key {
        "Calories" => t("nutrient.calories"),
        "Protein" => t("nutrient.protein"),
        "Fat" => t("nutrient.fat"),
        "Carbs" => t("nutrient.carbs"),
        _ => "???",
    }
}

pub fn nutrient_badge(key: &str) -> &'static str {
    match key {
        "Calories" => t("badge.calories"),
        "Protein" => t("badge.protein"),
        "Fat" => t("badge.fat"),
        "Carbs" => t("badge.carbs"),
        _ => "???",
    }
}

pub fn unit_label(key: &str) -> &'static str {
    match key {
        "kcal" => t("common.unit.kcal"),
        "g" => t("common.unit.g"),
        "mg" => t("common.unit.mg"),
        "µg" | "mcg" => t("common.unit.mcg"),
        _ => "???",
    }
}

fn en(key: &str) -> &'static str {
    match key {
        // Navigation
        "nav.diary" => "Diary",
        "nav.recipes" => "Recipes",
        "nav.settings" => "Settings",

        // Diary: relative dates
        "diary.today" => "Today",
        "diary.yesterday" => "Yesterday",
        "diary.day_before" => "Day before yesterday",

        // Diary: weekday full
        "diary.weekday.mon" => "Monday",
        "diary.weekday.tue" => "Tuesday",
        "diary.weekday.wed" => "Wednesday",
        "diary.weekday.thu" => "Thursday",
        "diary.weekday.fri" => "Friday",
        "diary.weekday.sat" => "Saturday",
        "diary.weekday.sun" => "Sunday",

        // Diary: weekday short
        "diary.weekday_short.mon" => "Mo",
        "diary.weekday_short.tue" => "Tu",
        "diary.weekday_short.wed" => "We",
        "diary.weekday_short.thu" => "Th",
        "diary.weekday_short.fri" => "Fr",
        "diary.weekday_short.sat" => "Sa",
        "diary.weekday_short.sun" => "Su",

        // Diary: months (genitive for dates)
        "diary.month.1" => "January",
        "diary.month.2" => "February",
        "diary.month.3" => "March",
        "diary.month.4" => "April",
        "diary.month.5" => "May",
        "diary.month.6" => "June",
        "diary.month.7" => "July",
        "diary.month.8" => "August",
        "diary.month.9" => "September",
        "diary.month.10" => "October",
        "diary.month.11" => "November",
        "diary.month.12" => "December",

        // Diary: weekday prepositional (for "On Monday there were no entries")
        "diary.weekday_prep.mon" => "On Monday",
        "diary.weekday_prep.tue" => "On Tuesday",
        "diary.weekday_prep.wed" => "On Wednesday",
        "diary.weekday_prep.thu" => "On Thursday",
        "diary.weekday_prep.fri" => "On Friday",
        "diary.weekday_prep.sat" => "On Saturday",
        "diary.weekday_prep.sun" => "On Sunday",

        // Diary: actions
        "diary.delete" => "Delete",
        "diary.repeat_today" => "Repeat today",
        "diary.no_entries" => "No entries for this day",
        "diary.per_week" => "per week",
        "diary.empty_today_1" => "This is where your food log will appear. There are no entries yet.",
        "diary.empty_today_2" => "To add an entry, tap the button below.",
        "diary.empty_past" => "there were no entries. This day has passed and you can no longer add food to it. You can only add food for today.",

        // Diary add modal
        "diary_add.title" => "Add to diary",
        "diary_add.search" => "Search",
        "diary_add.new" => "New",
        "diary_add.search_placeholder" => "Search food...",
        "diary_add.done" => "Done",
        "diary_add.how_much" => "How much?",
        "diary_add.add" => "Add",
        "diary_add.nothing_found" => "Nothing found",
        "diary_add.new_food" => "New food",
        "diary_add.add_new_food" => "Add new food",

        // Foods page
        "foods.title" => "Foods",
        "foods.add" => "+ Add",
        "foods.archive" => "Archive",

        // Recipes page
        "recipes.title" => "Recipes",
        "recipes.new" => "+ New",
        "recipes.search_placeholder" => "Search recipes...",
        "recipes.cook_again" => "Cook again",
        "recipes.complete" => "Complete",
        "recipes.in_progress" => "In Progress",

        // Recipe detail
        "recipe.loading" => "Loading...",
        "recipe.back" => "\u{2190} Recipes",
        "recipe.nutrients_whole" => "Nutrients for the whole dish",
        "recipe.whole_dish" => "Whole dish",
        "recipe.per_100g" => "Per 100g",
        "recipe.other_nutrients_hint" => "To display other nutrients change",
        "recipe.settings_link" => "settings",
        "recipe.add_ingredient" => "+ Add ingredient",
        "recipe.finalize" => "Finalize",
        "recipe.finalize_title" => "Finalize Recipe",
        "recipe.total_weight" => "Total ingredients weight:",
        "recipe.unknown_food" => "Unknown food",

        // Settings
        "settings.title" => "Settings",
        "settings.goals" => "Goals",
        "settings.not_less" => "not less",
        "settings.not_more" => "not more",
        "settings.period.day" => "day",
        "settings.period.week" => "week",
        "settings.period.month" => "month",
        "settings.off" => "off",
        "settings.add" => "+ Add",
        "settings.data" => "Data",
        "settings.wipe_all" => "Wipe all data",
        "settings.wipe_confirm" => "Are you sure? All local data will be deleted.",
        "settings.nutrient_placeholder" => "Omega 3, Fiber...",

        // Food editor
        "food_editor.product_name" => "Product name",
        "food_editor.add_photo" => "Add label photo",
        "food_editor.filling" => "Filling...",
        "food_editor.fill_info" => "Fill nutrition info",
        "food_editor.calories" => "Calories",
        "food_editor.protein" => "Protein",
        "food_editor.fat" => "Fat",
        "food_editor.carbs" => "Carbs",
        "food_editor.add" => "Add",

        // New food panel
        "new_food.title" => "New food",
        "new_food.history" => "History",

        // Add ingredient modal
        "add_ingredient.title" => "Add ingredient",
        "add_ingredient.search" => "Search",
        "add_ingredient.new" => "New",
        "add_ingredient.search_placeholder" => "Search food...",
        "add_ingredient.done" => "Done",

        // Weight modals
        "weight.per_100g" => "Per 100g:",
        "weight.package" => "Package",
        "weight.cancel" => "Cancel",
        "weight.ok" => "OK",
        "weight.save" => "Save",

        // Food modal
        "food_modal.title" => "Add Food",

        // Common
        "common.back" => "Back",
        "common.cancel" => "Cancel",
        "common.unit.kcal" => "kcal",
        "common.unit.g" => "g",
        "common.unit.mg" => "mg",
        "common.unit.mcg" => "µg",

        // Standard nutrient names (for display in goals, badges, etc.)
        "nutrient.calories" => "Calories",
        "nutrient.protein" => "Protein",
        "nutrient.fat" => "Fat",
        "nutrient.carbs" => "Carbs",

        // Badge short labels
        "badge.calories" => "C",
        "badge.protein" => "P",
        "badge.fat" => "F",
        "badge.carbs" => "Cb",

        // Language
        "settings.language" => "Language",

        // Auth
        "auth.main_description" => "This app works locally on your device and does not store data on remote servers. However, some features — such as syncing between devices or AI — require signing in.",
        "auth.create_account" => "Sign up",
        "auth.already_used" => "I already use this app:",
        "auth.creating" => "Creating...",
        "auth.authenticating" => "Signing in...",
        "auth.login_title" => "Sign in",
        "auth.login_have_device" => "If you have another signed-in device:",
        "auth.login_option1_hint" => "On the other device: Settings → Connect device → Scan QR code. Then press here:",
        "auth.login_option2_hint" => "On the other device: Settings → Connect device → Show QR code. Then press here:",
        "auth.login_no_device" => "If you don't have a signed-in device:",
        "auth.try_passkey" => "Try signing in with PassKey",
        "auth.show_qr_hint" => "Show this QR code to your signed-in device",
        // QR scanner
        "qr.no_camera" => "No camera found on this device.",
        "qr.permission_denied" => "Camera access denied. Allow camera in browser settings.",
        "qr.camera_error" => "Could not start camera.",
        "qr.copy_link" => "Copy link",
        "qr.copied" => "Copied!",
        "qr.paste_link" => "Paste link",

        "auth.error_network" => "Could not connect to server. Check your internet connection.",
        "auth.error_passkey" => "PassKey is not supported in this browser.",
        "auth.error_cancelled" => "PassKey creation was cancelled.",
        "auth.recovery_link" => "Recover access with password",
        "auth.recovery_title" => "Recover access",
        "auth.recovery_hint" => "Enter your recovery password to regain access to your account.",
        "auth.back" => "Back",
        "auth.name_placeholder" => "Your name",
        "auth.name_label" => "Display name",

        // PWA prompt
        "pwa.description" => "This is an app for managing your nutrition and building healthy eating habits. It can work as an app on your phone. To do that, you need to install it.",
        "pwa.title.ios" => "How to install on iPhone:",
        "pwa.title.android" => "How to install on Android:",
        "pwa.title.macos" => "How to install on Mac:",
        "pwa.title.desktop" => "How to install:",
        // iOS Safari
        "pwa.inst.ios_safari.1" => "Tap the Share button \u{1F4E4} at the bottom of the screen",
        "pwa.inst.ios_safari.2" => "Scroll down and tap \"Add to Home Screen\"",
        "pwa.inst.ios_safari.3" => "Tap \"Add\" in the top right corner",
        // iOS Chrome/Firefox
        "pwa.inst.ios_other.1" => "PWA install is only supported in Safari on iOS",
        "pwa.inst.ios_other.2" => "Open this page in Safari and follow the instructions",
        // Android Chrome
        "pwa.inst.android_chrome.1" => "Tap the menu \u{22ee} (three dots) in the top right",
        "pwa.inst.android_chrome.2" => "Tap \"Add to Home screen\" or \"Install app\"",
        "pwa.inst.android_chrome.3" => "Tap \"Install\" to confirm",
        // Android Samsung
        "pwa.inst.android_samsung.1" => "Tap the menu \u{2261} at the bottom right",
        "pwa.inst.android_samsung.2" => "Tap \"Add page to\" \u{2192} \"Home screen\"",
        // Android Firefox
        "pwa.inst.android_firefox.1" => "Tap the menu \u{22ee} (three dots)",
        "pwa.inst.android_firefox.2" => "Tap \"Install\"",
        "pwa.inst.android_firefox.3" => "Confirm the installation",
        // Android Yandex
        "pwa.inst.android_yandex.1" => "Tap the menu \u{22ee} (three dots)",
        "pwa.inst.android_yandex.2" => "Tap \"Add to Home screen\"",
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
        "pwa.use_browser" => "I want to use it in the browser anyway",

        // Pairing
        "pair.title" => "Connect device",
        "pair.show_qr" => "Show QR code",
        "pair.scan_qr" => "Scan QR code",
        "pair.waiting" => "Waiting for the other device...",
        "pair.success" => "Device connected!",
        "pair.expired" => "QR code expired. Try again.",
        "pair.error" => "Pairing failed. Try again.",
        "pair.scan_hint" => "Point camera at the QR code on the other device",
        "pair.show_hint_logged" => "Show this QR code to your new device",
        "pair.show_hint_new" => "Show this QR code to your logged-in device",
        "pair.add_device" => "Add device",
        "pair.back" => "Back",
        "pair.error_invalid_qr" => "Invalid QR code. Expected hjkl-pair:// link.",
        "settings.add_device" => "Add device",
        "settings.privacy" => "Privacy",
        "settings.active_sessions" => "Active sessions",
        "settings.current_device" => "This device",

        // Privacy page
        "privacy.title" => "Privacy",
        "privacy.back" => "\u{2190} Settings",
        "privacy.sessions" => "Active sessions",
        "privacy.this_device" => "This device",
        "privacy.add_device" => "Connect device",

        // Goals page
        "goals.title" => "Goals",
        "goals.back" => "\u{2190} Settings",
        "goals.standard" => "Standard nutrients",
        "goals.custom" => "Custom nutrients",
        "goals.no_custom" => "No custom nutrients added",
        "goals.mode_track" => "Track",
        "goals.mode_goal" => "Goal",

        // Notifications
        "settings.notifications" => "Notifications",
        "settings.push_enable" => "Enable push notifications",
        "settings.push_disable" => "Disable push notifications",
        "settings.push_enabled" => "Notifications enabled",
        "settings.push_not_supported" => "Push notifications not supported in this browser",
        "settings.schedule" => "Notification schedule",
        "settings.weigh_in" => "Weigh-in",
        "settings.breakfast" => "Breakfast",
        "settings.lunch" => "Lunch",
        "settings.dinner" => "Dinner",

        "push_onboarding.title" => "Notifications",
        "push_onboarding.description" => "This app can send notifications to remind you to fill in some data during the day. You need to grant permission so your device can show them.",
        "push_onboarding.allow" => "Allow notifications",
        "push_onboarding.skip" => "Not now",
        "push_onboarding.schedule_title" => "When to remind?",
        "push_onboarding.schedule_description" => "Choose which meals you want to be reminded about.",
        "push_onboarding.done" => "Done",
        "push_onboarding.skip_schedule" => "Skip",

        "weight.title" => "Weigh-in",
        "weight.no_water" => "I didn't drink water",
        "weight.no_food" => "I didn't eat",
        "weight.no_wash" => "I didn't shower or wash my face",
        "weight.used_toilet" => "I used the toilet before weighing",
        "weight.morning" => "I'm weighing in the morning",
        "weight.input_placeholder" => "Weight",
        "weight.save" => "Save",
        "weight.saved" => "Saved!",
        "weight.unit_kg" => "kg",
        "weight.unit_lbs" => "lbs",
        "weight.widget_title" => "Weight",
        "weight.widget_placeholder" => "Your weight chart will appear here. Not enough data to draw it yet — once you have at least three measurements, the chart will be shown.",

        _ => "???",
    }
}

fn ru(key: &str) -> &'static str {
    match key {
        // Навигация
        "nav.diary" => "Дневник",
        "nav.recipes" => "Рецепты",
        "nav.settings" => "Настройки",

        // Дневник: относительные даты
        "diary.today" => "Сегодня",
        "diary.yesterday" => "Вчера",
        "diary.day_before" => "Позавчера",

        // Дневник: дни недели полные
        "diary.weekday.mon" => "Понедельник",
        "diary.weekday.tue" => "Вторник",
        "diary.weekday.wed" => "Среда",
        "diary.weekday.thu" => "Четверг",
        "diary.weekday.fri" => "Пятница",
        "diary.weekday.sat" => "Суббота",
        "diary.weekday.sun" => "Воскресенье",

        // Дневник: дни недели короткие
        "diary.weekday_short.mon" => "Пн",
        "diary.weekday_short.tue" => "Вт",
        "diary.weekday_short.wed" => "Ср",
        "diary.weekday_short.thu" => "Чт",
        "diary.weekday_short.fri" => "Пт",
        "diary.weekday_short.sat" => "Сб",
        "diary.weekday_short.sun" => "Вс",

        // Дневник: месяцы (родительный падеж)
        "diary.month.1" => "января",
        "diary.month.2" => "февраля",
        "diary.month.3" => "марта",
        "diary.month.4" => "апреля",
        "diary.month.5" => "мая",
        "diary.month.6" => "июня",
        "diary.month.7" => "июля",
        "diary.month.8" => "августа",
        "diary.month.9" => "сентября",
        "diary.month.10" => "октября",
        "diary.month.11" => "ноября",
        "diary.month.12" => "декабря",

        // Дневник: дни недели с предлогом
        "diary.weekday_prep.mon" => "В понедельник",
        "diary.weekday_prep.tue" => "Во вторник",
        "diary.weekday_prep.wed" => "В среду",
        "diary.weekday_prep.thu" => "В четверг",
        "diary.weekday_prep.fri" => "В пятницу",
        "diary.weekday_prep.sat" => "В субботу",
        "diary.weekday_prep.sun" => "В воскресенье",

        // Дневник: действия
        "diary.delete" => "Удалить",
        "diary.repeat_today" => "Повторить сегодня",
        "diary.no_entries" => "Нет записей за этот день",
        "diary.per_week" => "в неделю",
        "diary.empty_today_1" => "Здесь будет список того, что вы съели. Пока что здесь нет ни одной записи.",
        "diary.empty_today_2" => "Чтобы добавить запись — нажмите кнопку ниже.",
        "diary.empty_past" => "не было ни одной записи. Этот день прошёл, и в него нельзя добавить еду. Еду можно добавить только сегодня.",

        // Модалка добавления в дневник
        "diary_add.title" => "Добавить в дневник",
        "diary_add.search" => "Поиск",
        "diary_add.new" => "Новый",
        "diary_add.search_placeholder" => "Найти продукт...",
        "diary_add.done" => "Готово",
        "diary_add.how_much" => "Сколько?",
        "diary_add.add" => "Добавить",
        "diary_add.nothing_found" => "Ничего не найдено",
        "diary_add.new_food" => "Новый продукт",
        "diary_add.add_new_food" => "Добавить новый продукт",

        // Продукты
        "foods.title" => "Продукты",
        "foods.add" => "+ Добавить",
        "foods.archive" => "Архив",

        // Рецепты
        "recipes.title" => "Рецепты",
        "recipes.new" => "+ Новый",
        "recipes.search_placeholder" => "Найти рецепт...",
        "recipes.cook_again" => "Приготовить снова",
        "recipes.complete" => "Готов",
        "recipes.in_progress" => "Готовится",

        // Детали рецепта
        "recipe.loading" => "Загрузка...",
        "recipe.back" => "\u{2190} Рецепты",
        "recipe.nutrients_whole" => "Количество нутриентов на всё блюдо",
        "recipe.whole_dish" => "Всё блюдо",
        "recipe.per_100g" => "На 100г",
        "recipe.other_nutrients_hint" => "Чтобы отобразить другие нутриенты измени",
        "recipe.settings_link" => "настройки",
        "recipe.add_ingredient" => "+ Добавить ингредиент",
        "recipe.finalize" => "Завершить",
        "recipe.finalize_title" => "Завершить рецепт",
        "recipe.total_weight" => "Общий вес ингредиентов:",
        "recipe.unknown_food" => "Неизвестный продукт",

        // Настройки
        "settings.title" => "Настройки",
        "settings.goals" => "Цели",
        "settings.not_less" => "не менее",
        "settings.not_more" => "не более",
        "settings.period.day" => "день",
        "settings.period.week" => "неделя",
        "settings.period.month" => "месяц",
        "settings.off" => "выкл",
        "settings.add" => "+ Добавить",
        "settings.data" => "Данные",
        "settings.wipe_all" => "Удалить все данные",
        "settings.wipe_confirm" => "Вы уверены? Все локальные данные будут удалены.",
        "settings.nutrient_placeholder" => "Omega 3, Fiber...",

        // Редактор продукта
        "food_editor.product_name" => "Название продукта",
        "food_editor.add_photo" => "Добавить фото этикетки",
        "food_editor.filling" => "Заполняю...",
        "food_editor.fill_info" => "Заполнить питательную ценность",
        "food_editor.calories" => "Калории",
        "food_editor.protein" => "Белки",
        "food_editor.fat" => "Жиры",
        "food_editor.carbs" => "Углеводы",
        "food_editor.add" => "Добавить",

        // Панель нового продукта
        "new_food.title" => "Новый продукт",
        "new_food.history" => "История",

        // Модалка ингредиента
        "add_ingredient.title" => "Добавить ингредиент",
        "add_ingredient.search" => "Поиск",
        "add_ingredient.new" => "Новый",
        "add_ingredient.search_placeholder" => "Найти продукт...",
        "add_ingredient.done" => "Готово",

        // Модалки веса
        "weight.per_100g" => "На 100г:",
        "weight.package" => "Упаковка",
        "weight.cancel" => "Отмена",
        "weight.ok" => "OK",
        "weight.save" => "Сохранить",

        // Модалка продукта
        "food_modal.title" => "Добавить продукт",

        // Общее
        "common.back" => "Назад",
        "common.cancel" => "Отмена",
        "common.unit.kcal" => "ккал",
        "common.unit.g" => "г",
        "common.unit.mg" => "мг",
        "common.unit.mcg" => "мкг",

        // Стандартные нутриенты
        "nutrient.calories" => "Калории",
        "nutrient.protein" => "Белок",
        "nutrient.fat" => "Жиры",
        "nutrient.carbs" => "Углеводы",

        // Бейджи
        "badge.calories" => "К",
        "badge.protein" => "Б",
        "badge.fat" => "Ж",
        "badge.carbs" => "У",

        // Язык
        "settings.language" => "Язык",

        // Авторизация
        "auth.main_description" => "Это приложение работает локально на вашем устройстве и не хранит данные на удалённых серверах. Однако для некоторых функций — таких как синхронизация между устройствами или ИИ — необходимо авторизоваться.",
        "auth.create_account" => "Зарегистрироваться",
        "auth.already_used" => "Я уже пользовался этим приложением:",
        "auth.creating" => "Создаю...",
        "auth.authenticating" => "Вхожу...",
        "auth.login_title" => "Войти",
        "auth.login_have_device" => "Если у вас есть другое устройство, где вы вошли:",
        "auth.login_option1_hint" => "На другом устройстве: Настройки → Подключить устройство → Сканировать QR-код. Затем нажмите здесь:",
        "auth.login_option2_hint" => "На другом устройстве: Настройки → Подключить устройство → Показать QR-код. Затем нажмите здесь:",
        "auth.login_no_device" => "Если у вас нет залогиненного устройства:",
        "auth.try_passkey" => "Попробовать войти с ключом входа",
        "auth.show_qr_hint" => "Покажите этот QR-код залогиненному устройству",
        // QR сканер
        "qr.no_camera" => "Камера не найдена на этом устройстве.",
        "qr.permission_denied" => "Доступ к камере запрещён. Разрешите камеру в настройках браузера.",
        "qr.camera_error" => "Не удалось запустить камеру.",
        "qr.copy_link" => "Копировать ссылку",
        "qr.copied" => "Скопировано!",
        "qr.paste_link" => "Вставить ссылку",

        "auth.error_network" => "Не удалось подключиться к серверу. Проверьте интернет.",
        "auth.error_passkey" => "PassKey не поддерживается в этом браузере.",
        "auth.error_cancelled" => "Создание PassKey было отменено.",
        "auth.recovery_link" => "Восстановить доступ по паролю",
        "auth.recovery_title" => "Восстановление доступа",
        "auth.recovery_hint" => "Введите пароль восстановления для доступа к аккаунту.",
        "auth.back" => "Назад",
        "auth.name_placeholder" => "Ваше имя",
        "auth.name_label" => "Имя",

        // PWA
        "pwa.description" => "Это приложение для организации питания и формирования здоровых пищевых привычек. Оно может работать как приложение в вашем телефоне. Для этого его нужно установить.",
        "pwa.title.ios" => "Как установить на iPhone:",
        "pwa.title.android" => "Как установить на Android:",
        "pwa.title.macos" => "Как установить на Mac:",
        "pwa.title.desktop" => "Как установить:",
        "pwa.inst.ios_safari.1" => "Нажмите кнопку «Поделиться» \u{1F4E4} внизу экрана",
        "pwa.inst.ios_safari.2" => "Прокрутите вниз и нажмите «На экран Домой»",
        "pwa.inst.ios_safari.3" => "Нажмите «Добавить» в правом верхнем углу",
        "pwa.inst.ios_other.1" => "Установка PWA на iOS работает только в Safari",
        "pwa.inst.ios_other.2" => "Откройте эту страницу в Safari и следуйте инструкции",
        "pwa.inst.android_chrome.1" => "Нажмите меню \u{22ee} (три точки) в правом верхнем углу",
        "pwa.inst.android_chrome.2" => "Нажмите «Добавить на главный экран» или «Установить»",
        "pwa.inst.android_chrome.3" => "Нажмите «Установить» для подтверждения",
        "pwa.inst.android_samsung.1" => "Нажмите меню \u{2261} в правом нижнем углу",
        "pwa.inst.android_samsung.2" => "Нажмите «Добавить страницу на» \u{2192} «Главный экран»",
        "pwa.inst.android_firefox.1" => "Нажмите меню \u{22ee} (три точки)",
        "pwa.inst.android_firefox.2" => "Нажмите «Установить»",
        "pwa.inst.android_firefox.3" => "Подтвердите установку",
        "pwa.inst.android_yandex.1" => "Нажмите меню \u{22ee} (три точки)",
        "pwa.inst.android_yandex.2" => "Нажмите «Добавить на Домашний экран»",
        "pwa.inst.macos_safari.1" => "В меню: Файл \u{2192} Добавить в Dock",
        "pwa.inst.macos_safari.2" => "Приложение появится в вашем Dock",
        "pwa.inst.chrome.1" => "Нажмите значок установки в адресной строке",
        "pwa.inst.chrome.2" => "Нажмите «Установить» во всплывающем окне",
        "pwa.inst.edge.1" => "Меню \u{2026} \u{2192} Приложения \u{2192} Установить этот сайт как приложение",
        "pwa.inst.edge.2" => "Нажмите «Установить» для подтверждения",
        "pwa.inst.firefox.1" => "Firefox на компьютере не поддерживает установку PWA. Используйте Chrome, Edge или Safari.",
        "pwa.use_browser" => "Я хочу использовать в браузере",

        // Pairing
        "pair.title" => "Подключить устройство",
        "pair.show_qr" => "Показать QR-код",
        "pair.scan_qr" => "Сканировать QR-код",
        "pair.waiting" => "Ожидание другого устройства...",
        "pair.success" => "Устройство подключено!",
        "pair.expired" => "QR-код истёк. Попробуйте снова.",
        "pair.error" => "Не удалось подключить. Попробуйте снова.",
        "pair.scan_hint" => "Наведите камеру на QR-код на другом устройстве",
        "pair.show_hint_logged" => "Покажите этот QR-код новому устройству",
        "pair.show_hint_new" => "Покажите этот QR-код залогиненному устройству",
        "pair.add_device" => "Подключить устройство",
        "pair.back" => "Назад",
        "pair.error_invalid_qr" => "Неверный QR-код. Ожидалась ссылка hjkl-pair://.",
        "settings.add_device" => "Подключить устройство",
        "settings.privacy" => "Приватность",
        "settings.active_sessions" => "Активные сессии",
        "settings.current_device" => "Это устройство",

        // Страница приватности
        "privacy.title" => "Приватность",
        "privacy.back" => "\u{2190} Настройки",
        "privacy.sessions" => "Активные сессии",
        "privacy.this_device" => "Это устройство",
        "privacy.add_device" => "Подключить устройство",

        // Страница целей
        "goals.title" => "Цели",
        "goals.back" => "\u{2190} Настройки",
        "goals.standard" => "Стандартные нутриенты",
        "goals.custom" => "Пользовательские нутриенты",
        "goals.no_custom" => "Нет пользовательских нутриентов",
        "goals.mode_track" => "Следить",
        "goals.mode_goal" => "Цель",

        // Уведомления
        "settings.notifications" => "Уведомления",
        "settings.push_enable" => "Включить уведомления",
        "settings.push_disable" => "Отключить уведомления",
        "settings.push_enabled" => "Уведомления включены",
        "settings.push_not_supported" => "Push-уведомления не поддерживаются в этом браузере",
        "settings.schedule" => "Расписание уведомлений",
        "settings.weigh_in" => "Взвешивание",
        "settings.breakfast" => "Завтрак",
        "settings.lunch" => "Обед",
        "settings.dinner" => "Ужин",

        "push_onboarding.title" => "Уведомления",
        "push_onboarding.description" => "Это приложение может рассылать уведомления, чтобы проинформировать о необходимости заполнить некоторые данные в течение дня. Надо дать разрешение, чтобы ваше устройство могло вам их показывать.",
        "push_onboarding.allow" => "Разрешить уведомления",
        "push_onboarding.skip" => "Не сейчас",
        "push_onboarding.schedule_title" => "Когда напоминать?",
        "push_onboarding.schedule_description" => "Выберите приёмы пищи, о которых хотите получать напоминания.",
        "push_onboarding.done" => "Готово",
        "push_onboarding.skip_schedule" => "Пропустить",

        "weight.title" => "Взвешивание",
        "weight.no_water" => "Я не пил воду",
        "weight.no_food" => "Я не ел",
        "weight.no_wash" => "Я не мылся и не умывался",
        "weight.used_toilet" => "Я сходил в туалет",
        "weight.morning" => "Я взвешиваюсь с утра",
        "weight.input_placeholder" => "Вес",
        "weight.save" => "Сохранить",
        "weight.saved" => "Сохранено!",
        "weight.unit_kg" => "кг",
        "weight.unit_lbs" => "фунты",
        "weight.widget_title" => "Вес",
        "weight.widget_placeholder" => "Здесь будет график вашего веса. Пока что график не изобразить, потому что слишком мало данных. Когда появится хотя бы три измерения, график будет нарисован.",

        _ => "???",
    }
}
