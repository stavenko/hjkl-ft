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
        "nav.story" => "Story",
        "nav.diary" => "Diary",
        "nav.recipes" => "Recipes",
        "nav.settings" => "Settings",
        "nav.support" => "Support",

        // Chat
        "chat.requesting" => "Requesting",
        "chat.thinking" => "Thinking",
        "chat.answer" => "Answer",
        "chat.tool_running" => "Running tool",
        "chat.input_placeholder" => "Message support…",
        "chat.send" => "Send",
        "chat.attach_image" => "Attach image",
        "chat.record_voice" => "Record voice",
        "chat.recording" => "Recording…",
        "chat.stop_recording" => "Stop",
        "chat.recording" => "Recording…",
        "chat.escalated_banner" => "Transferring you to a live operator…",
        "chat.attached_image" => "[attached: image]",
        "chat.attached_voice" => "[attached: voice]",
        "chat.empty" => "No messages yet. Ask how to use the app, or describe a problem — I can help you file a bug report.",
        "chat.mic_denied" => "Microphone access denied",

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

        // Meal-split section headers
        "meal.breakfast" => "Breakfast",
        "meal.snack_morning" => "Morning snack",
        "meal.lunch" => "Lunch",
        "meal.snack_afternoon" => "Afternoon snack",
        "meal.dinner" => "Dinner",
        "meal.snack_night" => "Night snack",
        "meal.breakfast_sub" => "the morning binge",
        "meal.lunch_sub" => "the daytime binge",
        "meal.dinner_sub" => "the nighttime binge",

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
        "diary.duplicate" => "Duplicate",
        "diary.edit" => "Edit",
        "diary.edit_product" => "Edit product",
        "diary.repeat_today" => "Repeat today",
        "diary.no_entries" => "No entries for this day",
        "diary.per_week" => "per week",
        "diary.empty_today_1" => "This is where your food log will appear. There are no entries yet.",
        "diary.empty_today_2" => "To add an entry, tap the button below.",
        "diary.empty_past" => "there were no entries. This day has passed and you can no longer add food to it. You can only add food for today.",

        // Daily / weekly summary
        "summary.day_title" => "Summary of the day",
        "summary.generating" => "Preparing a summary…",
        "summary.gen_failed" => "Couldn't prepare the assessment — tap to try again.",
        "summary.good_title" => "What went well",
        "summary.improve_title" => "What to improve",
        "summary.all_good" => "Great job — keep it up!",
        "summary.facts_title" => "Facts",
        "summary.facts_veg_fruit" => "Vegetables & fruits",
        "summary.regenerate" => "Redo the assessment",
        "summary.source" => "source",
        "summary.good_weight_steps" => "You logged your weight and steps — awesome.",
        "summary.good_diary" => "You're keeping a food diary.",
        "summary.good_restaurant" => "You log your food even at a restaurant.",
        "summary.good_snack" => "You reached for a low-calorie snack — a great way to close the urge to chew without wrecking the day.",
        "summary.good_no_cal_drink" => "No calorie drinks today — those are the easiest calories to drink without noticing.",
        "summary.good_evening_protein" => "Strong evening protein — fullness lasts longer and there are fewer night raids on the fridge.",
        "summary.improve_weighing" => "Improve your weighing quality: the higher it is, the clearer it is whether you're in a surplus or a deficit.",
        "summary.improve_steps" => "Going over 7000 steps a day brings a substantial health improvement.",
        "summary.improve_drink" => "A calorie drink slipped in: swap it for a sugar-free version or the fruit itself — those calories give almost no fullness.",
        "summary.good_under_planka" => "You stayed within your calorie planka today — that's exactly the pace that lets fat go while muscle stays.",
        "summary.improve_over_planka" => "Watch your calorie planka — especially if you went above it: that's where the deficit quietly slips away. Eating far too little isn't good either, but going over is the main thing to fix.",
        "summary.week_button" => "Weekly report",
        "summary.week_title" => "Weekly report",
        "summary.week_pending" => "The weekly report will be ready on",

        // Diary add modal
        "diary_add.title" => "Add to diary",
        "diary_add.search" => "Search",
        "diary_add.new" => "New",
        "diary_add.search_placeholder" => "Search food...",
        "diary_add.done" => "Done",
        "diary_add.close" => "Close",
        "diary_add.how_much" => "How much?",
        "diary_add.add" => "Add",
        "diary_add.cancel" => "Cancel",
        "diary_add.nothing_found" => "Nothing found",
        "diary_add.new_food" => "New food",
        "diary_add.more" => "Show",
        "diary_add.products" => "more",
        "diary_add.add_new_food" => "Add new food",
        "diary_add.back" => "Diary",

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
        "settings.version" => "Version",
        "settings.version_current" => "Build:",
        "settings.version_up_to_date" => "Up to date",
        "settings.version_available" => "A new version is available",
        "settings.version_update" => "Update",
        "settings.danger_zone" => "Danger zone",
        "settings.danger_reset_story" => "Delete my story progress",
        "settings.danger_confirm_story" => "Delete all your progress in the story? Your logged data stays.",
        "settings.danger_delete_diary" => "Delete diary data",
        "settings.danger_delete_old" => "Delete data older than 1 year",
        "settings.danger_confirm_old" => "Delete diary entries older than 1 year? This cannot be undone.",
        "settings.danger_delete_all" => "Delete all data",
        "settings.danger_confirm_all" => "Delete ALL diary entries? This cannot be undone.",
        "settings.nutrient_placeholder" => "Omega 3, Fiber...",

        // Food editor
        "food_editor.product_name" => "Product name",
        "food_editor.add_photo" => "Add label photo",
        "food_editor.add_more_photo" => "Add another photo",
        "food_editor.photo_hint" => "Shoot the nutrition-facts table up close so it fills the frame — small/distant text is read poorly.",
        "food_editor.ai_uploading" => "Uploading photo\u{2026}",
        "food_editor.ai_queue" => "In queue:",
        "food_editor.ai_recognizing" => "Recognizing\u{2026}",
        "food_editor.ai_timeout" => "Recognition is taking too long — try again later.",
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
        "waste.not_whole" => "Didn't eat it whole",
        "waste.placeholder" => "Waste",
        "restaurant.eaten_out" => "Restaurant food",
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
        "settings.sex" => "Sex",
        "settings.sex_female" => "Female",
        "settings.sex_male" => "Male",
        "settings.sex_why" => "Why we ask: for women some nutrient targets are softer, and body weight naturally fluctuates over the menstrual cycle — knowing your sex lets the app track real weight changes more accurately.",

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
        "settings.check_notifications" => "Check notifications",
        "settings.notif_enable_check" => "Enable and check",
        "settings.notif_check" => "Check",
        "settings.notif_push_task" => "\u{1f514} Tap to complete the task",
        "settings.notif_push_plain" => "\u{2705} Notifications work!",
        "settings.sending" => "Sending…",
        "settings.push_enable" => "Enable push notifications",
        "settings.push_disable" => "Disable push notifications",
        "settings.push_enabled" => "Notifications enabled",
        "settings.push_not_supported" => "Push notifications not supported in this browser",
        "settings.schedule" => "Notification schedule",
        "settings.weigh_in" => "Weigh-in",
        "settings.breakfast" => "Breakfast",
        "settings.lunch" => "Lunch",
        "settings.dinner" => "Dinner",
        "settings.steps" => "Steps",

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
        "weight.add" => "Weigh in",
        "weight.edit" => "Edit today's weight",
        "weight.once_per_day" => "One entry per day — you can edit it",
        "weight.col_date" => "Date",
        "weight.col_time" => "Time",
        "weight.col_quality" => "Quality",
        "weight.col_weight" => "Weight",
        "weight.saved" => "Saved!",
        "weight.unit_kg" => "kg",
        "weight.unit_lbs" => "lbs",
        "weight.widget_title" => "Weight",
        "weight.widget_placeholder" => "Your weight chart will appear here. Not enough data to draw it yet — once you have at least three measurements, the chart will be shown.",
        "weight.trend.title" => "Trend · 14 days",
        "weight.trend.down" => "Losing",
        "weight.trend.up" => "Gaining",
        "weight.trend.unclear" => "Trend not clear yet",
        "weight.trend.insufficient" => "Not enough data for a trend",
        "weight.trend.preliminary" => "preliminary",
        "weight.trend.week" => "week",
        "weight.trend.confidence" => "confidence",
        "weight.cycle.label" => "Period",
        "weight.cycle.none" => "no cycle detected",
        "weight.cycle.insufficient" => "not enough data yet",
        "weight.cycle.day_short" => "d",
        "weight.cycle.decycled" => "Weight without the cycle",

        "steps.title" => "Steps",
        "steps.for_today" => "Recording evening steps for TODAY",
        "steps.for_yesterday" => "Recording morning steps for YESTERDAY",
        "steps.input_placeholder" => "Steps",
        "steps.unit" => "steps",
        "steps.save" => "Save",
        "steps.add" => "Record steps",
        "steps.edit" => "Edit today's steps",
        "steps.once_per_day" => "One entry per day — you can edit it",
        "steps.col_steps" => "Steps",

        // Story
        "story.title" => "Story",
        "story.chapter" => "Chapter",
        "story.sections_opened" => "Sections opened",
        "story.tasks_done" => "Tasks completed",
        "story.locked_hint" => "Complete these tasks to unlock:",
        "story.ch1.title" => "Nice to meet you",
        "story.ch1.intro" => "Introduction",
        "story.ch1.setup" => "Let's set up the app",
        "story.ch1.accounting" => "The accounting of weight loss",
        "story.ch1.first_food" => "My first food entries",
        "story.ch1.activity" => "Activity and weight",
        "story.ch1.cooking" => "I'm cooking",
        "story.ch1.bones" => "My food with bones",
        "story.ch1.restaurant" => "A party or a restaurant",
        "story.ch1.next" => "What's next",
        "story.next.p_intro" => "Right now the most important thing is to get comfortable with the app and improve your tracking skill day by day.",
        "story.next.rules_label" => "It's important to stick to a few simple rules:",
        "story.next.rule1" => "First log it — then eat it.",
        "story.next.rule2" => "If you logged it — eat all of it.",
        "story.next.rule3" => "Steps and weight matter a lot.",
        "story.next.p_discipline" => "Once you're disciplined on the weight-loss track and follow the tasks precisely, the app itself will start guiding you on where to go next.",
        "story.next.focus_label" => "So the key things are:",
        "story.next.focus1" => "Simplify tracking — the less complex and restaurant food, the better.",
        "story.next.focus2" => "Cut out alcohol. Once you've built the discipline you can bring it back, but for now it's best removed.",
        "story.next.focus3" => "Improve your tracking quality every day. We're not perfect and constantly slip up — always aim to count more accurately.",
        "story.next.p_goals" => "A little later your first tracking goals will appear — calories, protein, vegetables and fruit. But for now what matters is simply keeping track.",
        "story.next.p_report" => "And one more thing: the summary for yesterday is already available. Open the diary, swipe to a past day — the day's summary is below the food list.",
        "story.next.open_diary" => "Open the diary",
        "story.ch2.task_notif" => "Check that notifications work",
        "story.ch2.task_weight" => "Weigh in 7 days in a row",
        "story.ch2.task_subscription" => "Have an active subscription",
        "story.ch2.unlocked" => "Chapter 2 is open! Sections coming soon.",
        "story.ch2.title" => "Appetite",
        "story.ch2.soon" => "soon",
        "story.ch2.s1" => "The biggest mistake of dieters",
        "story.ch2.s2" => "Vegetables and fruit",
        "story.ch2.s3" => "Protein — your best helper",
        "story.ch2.s4" => "Low-calorie snacks",
        "story.ch2.s5" => "Juices and drinks",
        "story.ch2.s6" => "How many meals a day",
        "story.ch2.s7" => "Eating at night",

        // Story — chapter 2 «Appetite», section 1: the biggest mistake
        "story.ch2.mistake.p1" => "When someone decides to lose weight, the first thing they do almost on autopilot is start eating less. Smaller portions, a smaller plate, fewer trips to the fridge. The logic seems ironclad: less food — fewer calories — less weight. And for the first few days it even works.",
        "story.ch2.mistake.p2" => "The problem is that the stomach doesn't count calories. It counts volume. An empty stomach sends the brain a «I'm hungry» signal regardless of how much energy it held — and sooner or later that signal beats any willpower. That's why «small portion» diets almost always end in a binge: not because the person is weak, but because they're fighting physiology.",
        "story.ch2.mistake.p3" => "The way out sounds paradoxical: you need not less food, but more — enough to keep the stomach full and content. The whole trick is to sharply cut the calorie density of what you eat. A full plate of low-calorie food satisfies just as well as a dense one, and leaves no hunger behind — and everything else is built on that.",
        "story.ch2.mistake.task_label" => "Chapter task",
        "story.ch2.mistake.task" => "Keep the food diary 7 days in a row",
        "story.ch2.mistake.streak_label" => "Streak",
        "story.ch2.mistake.next_unlocked" => "Great — the next section is open.",
        "story.ch2.mistake.open_diary" => "Open the diary",
        "story.ch2.mistake.no_data" => "No diary data yet — start logging your food.",

        // Story — chapter 2, section 2: vegetables and fruit
        "story.ch2.veg.p1" => "Vegetables and fruit are the main tool for filling your plate without filling it with calories. They contain a lot of water and fibre — that is, volume that takes up space in the stomach and gives a feeling of fullness, while carrying very little energy. Plus vitamins and everything they're recommended for.",
        "story.ch2.veg.p2" => "The guideline is simple: 600–800 grams of vegetables and fruit a day. A man — closer to the upper bound, a woman — to the lower one, then adjust to yourself. This is not a «healthy add-on» to your diet but its foundation: it's precisely this volume that lets you eat your fill while staying in a deficit.",
        "story.ch2.veg.target_label" => "Yesterday's vegetables and fruit",
        "story.ch2.veg.no_data" => "No data for yesterday yet.",

        // Story — chapter 2, section 3: protein
        "story.ch2.protein.p1" => "If vegetables give you volume, protein gives a different kind of fullness — the kind that lasts for hours. Of all the macronutrients it satisfies the most: after a protein meal you get hungry noticeably later, and crave snacks less.",
        "story.ch2.protein.p2" => "That's why on a diet you don't cut protein — on the contrary, you make it a priority. A working guideline is from 1 gram per kilogram of body weight a day, and in most cases it makes sense to go higher. This is that rare case where «maximise» is the right strategy: protein won't be excessive here, while a shortfall almost guarantees hunger and muscle loss.",
        "story.ch2.protein.goal_label" => "Protein goal",
        "story.ch2.protein.goal_set" => "Your protein goal is set — see it in the diary header.",
        "story.ch2.protein.need_weight" => "Weigh in first so we can set your protein goal.",
        "story.ch2.protein.open_weight" => "Weigh in",

        // Story — chapter 2, section 4: low-calorie snacks
        "story.ch2.snack.p1" => "Hunger rarely comes on time. It hits between tasks, in the evening before a show, at a moment when a proper meal is still far off — and that's where it's decided whether you break or not. So it's worth keeping something on hand to snack on with almost no consequences.",
        "story.ch2.snack.p2" => "Unbuttered popcorn, cucumbers, tomatoes, apples — they have few calories and a lot of water; you can crunch on them almost to bursting and barely shift your daily balance. This is not «forbidden food in reverse» but normal food that closes the urge to chew on something.",
        "story.ch2.snack.p3" => "The only rule is to still log it in the diary. «It's just a cucumber, what's there to count» — and that's how the stuff you later can't explain on the scale quietly adds up. Everything counts, even the little things.",
        "story.ch2.snack.task_label" => "Chapter task",
        "story.ch2.snack.task" => "Log a low-calorie snack — and check it in tomorrow's report",
        "story.ch2.snack.no_report" => "The report for yesterday isn't ready yet — it appears once the day is over.",

        // Story — chapter 2, section 5: juices and drinks
        "story.ch2.drinks.p1" => "The most invisible way to drink your daily calorie quota is exactly that — to drink it. Sweet soda, packaged juice, a latte with syrup — these are calories that give almost no fullness. The body seems not to notice them: you've taken the equivalent of a full meal, and half an hour later you're hungry again.",
        "story.ch2.drinks.p2" => "So first of all we remove the calories from drinks. Swap regular soda for the sugar-free version — yes, sweeteners aren't the enemy here, no need to fear them; just find a couple of flavours you genuinely like. Replace juice with the fruit itself — that way you also get fibre and fullness. Water, coffee, tea — drink as much as you want, just remove or minimise what adds calories: sugar and cream.",
        "story.ch2.drinks.task_label" => "Chapter task",
        "story.ch2.drinks.task" => "Skip the calorie drinks — none in yesterday's report",
        "story.ch2.drinks.effort" => "We see the effort — a calorie-free drink. Logging it is optional.",

        // Story — chapter 2, section 6: how many meals a day
        "story.ch2.meals.p1" => "There's no sacred number here. Three meals, six, two — it all works if in total you stay in a deficit. So the first principle is to make the day as convenient as possible for you personally. Love snacking often and getting distracted by food — split it up. Don't like fussing five times — don't split it.",
        "story.ch2.meals.p2" => "The basic recommendation is still one: try not to let yourself get very hungry. Rising appetite is easily knocked down with sweet calorie-free water between meals, while toughing it out to the point of near-fainting is a direct path to a binge. And be sure to satisfy hunger at the end of the day: going to bed hungry is a bad idea — that's exactly where night raids on the fridge come from.",
        "story.ch2.meals.p3" => "And if you ask what works best — it's three meals. Breakfast, lunch, dinner — perhaps the best thing humanity invented for the rhythm of the day, and for losing weight it's an excellent framework.",
        "story.ch2.meals.advice" => "And a practical rule: cut down on snacking, and when the urge to chew hits — reach for a drink instead. Now your diary will group entries into meals, so you can see the picture of your day at a glance.",
        "story.ch2.meals.unlocked" => "Done — your diary now splits the day into meals.",

        // Story — chapter 2, section 7: eating at night
        "story.ch2.night.p1" => "Let's start with the main and unexpected point: eating at night isn't harmful. You can eat right before bed, you can not eat at all — it has no effect whatsoever on weight loss. Fat is stored not «because it's late» but because more calories came in over the day than went out. «Nothing after six» is a myth.",
        "story.ch2.night.p2" => "But there are a couple of honest caveats. The first is about binges: many people simply can't resist and eat up at night. If that's you, the best insurance is a hearty, protein-rich last meal: fullness lasts longer, and the pull to the fridge will be weaker.",
        "story.ch2.night.p3" => "The second is medical. With a number of conditions (reflux, for example) doctors advise not lying down sooner than three hours after dinner. This is no longer about weight loss but about health and a calm sleep — and that recommendation is worth following.",
        "story.ch2.night.feedback_label" => "Today's evening",
        "story.ch2.night.feedback_good" => "Lots of protein in the evening — excellent: fullness lasts longer, fewer night raids.",
        "story.ch2.night.feedback_hint" => "Try to make your last meal protein-rich — it keeps you full longer and softens the evening pull to the fridge.",

        // Story — chapter 3 «Why lose weight?»
        "story.ch3.title" => "Why lose weight?",
        "story.ch3.unlocked" => "Chapter 3 is open.",
        "story.ch3.locked_hint" => "Complete every section of chapter 2 to unlock.",
        "story.ch3.s1" => "Why do we have fat at all?",
        "story.ch3.s2" => "What does a «beautiful person» look like?",
        "story.ch3.s3" => "What is the physiological fat minimum?",
        "story.ch3.s4" => "The difference between «lean» and «fat-stripped»",
        "story.ch3.s5" => "Life without fat",

        // Story — chapter 3, section 1: why do we have fat at all
        "story.ch3.fat.p1" => "Fat is not a flaw in the body and not a punishment for weakness. It's the most ancient survival system there is. For hundreds of thousands of years food was unstable — feast then famine — and the ones who survived were those who could store energy in reserve to make it through a hungry winter or a failed harvest. The ability to stockpile fat was a superpower, and evolution honed it to perfection.",
        "story.ch3.fat.p2" => "It all broke down in the last few decades. Food became not just sufficient — it became abundant, cheap, calorie-dense and available every minute. The hungry winter never comes anymore, yet the storage system runs as if nothing has changed. The mechanism that saved our ancestors turned into a problem in a world of plenty.",
        "story.ch3.fat.p3" => "And the problem isn't only about how it looks. Fat tissue is not a passive warehouse but a full-fledged endocrine organ: it produces hormones and signalling substances. When there's too much fat, those signals go haywire — and one of the key consequences is insulin resistance: cells stop hearing insulin properly. From there a long trail of health problems follows. So it's worth losing weight not for the picture, but because excess fat actively interferes with how the body works.",
        "story.ch3.fat.goal_label" => "Daily calorie planka",
        "story.ch3.fat.kcal_unit" => "kcal",
        "story.ch3.fat.goal_set" => "Your daily calorie planka is set — see it in the diary header.",
        "story.ch3.fat.need_diary" => "Keep logging your food for a few days so we can set your calorie planka.",
        "story.ch3.fat.open_diary" => "Open the diary",

        // Story — chapter 3, section 2: what a beautiful person looks like
        "story.ch3.beauty.p1" => "People have argued about beauty for millennia, but if you look closely, it almost always comes down to two fairly simple things. The first is the bones. If the facial skeleton developed normally, the features come out harmonious — and that is largely given by nature.",
        "story.ch3.beauty.p2" => "The second thing, though, is entirely in our hands. Take a person with normal facial features and remove the excess fat, leaving a healthy physiological minimum — and they will almost certainly look beautiful. Not because they «lost weight», but because what fat usually hides starts to show: the lines of the face, the contours of the body, the muscular frame.",
        "story.ch3.beauty.p3" => "In essence, the beauty of a body is made of two terms: the absence of excess fat and the presence of muscle under the skin. This is an important idea we'll return to: the goal is not to «become smaller» but to reveal the structure that is already there. And it's achieved not by starving but by combining moderate fat with normally developed muscle.",

        // Story — chapter 3, section 3: the physiological fat minimum
        "story.ch3.minimum.p1" => "Fat has a lower bound below which you must not go — the so-called physiological, essential fat. It's not a reserve but a working part of the body: it's needed for hormones, the nervous system, organ protection. Going below that line isn't «getting extra-lean» — it's harming yourself.",
        "story.ch3.minimum.p2" => "It's important to understand: this minimum differs for men and women, and for women it's noticeably higher. That's not a shortcoming but the norm — female physiology, including reproduction, simply requires more fat, and trying to force yourself into «male» numbers ends in a breakdown of hormones and the cycle. A woman should aim for a healthy fat level, not the lowest possible one.",
        "story.ch3.minimum.p3" => "And here lies a common trap. A healthy minimum on a woman without muscle may look nothing like she wants — not toned, but rather «empty». And the conclusion is not «lose more» but exactly the opposite: it's not about excess fat but a lack of muscle. The fix isn't a diet — it's the gym.",

        // Story — chapter 3, section 4: lean vs fat-stripped
        "story.ch3.lean.p1" => "The scale shows one number, but very different things stand behind it. When a person loses weight fast and hard, they lose not only fat. Muscle goes with it — and muscle is exactly what keeps the body toned and the metabolism alive. The result: weight drops, but the mirror and how you feel are disappointing.",
        "story.ch3.lean.p2" => "From this come two principles worth accepting right away. First: lose weight gradually. The gentler the pace, the better the odds that it's fat that leaves while muscle stays. Second: one «weight-loss course» isn't enough — you have to change the lifestyle itself.",
        "story.ch3.lean.p3" => "Otherwise the sad arithmetic kicks in. You drop weight on a harsh diet, losing both fat and muscle; you go back to old habits — the weight returns. But it comes back mostly as fat, and the lost muscle isn't so easy to get back. You end up at the same weight as before the diet, only with a worse body composition and no health reserve. So the goal isn't to «lose weight» but to rebuild your life so the new weight holds on its own.",

        // Story — chapter 3, section 5: life without fat
        "story.ch3.lifestyle.p1" => "The main idea of the whole chapter is simple: a normal fat percentage is not a destination but a by-product. You don't «reach» it in one heroic push only to grimly hold on afterwards. It comes about by itself for someone who lives that way: eats normally, moves, sleeps, carries muscle.",
        "story.ch3.lifestyle.p2" => "It works in reverse too. Obesity is almost always not an accident or a verdict of genetics, but a reflection of how life is arranged day to day. What's in the fridge, how much a person moves, how they sleep, what they soothe stress with — the number on the scale is built from these little things.",
        "story.ch3.lifestyle.p3" => "It sounds harsh, but it's actually good news. If weight is a derivative of lifestyle, then it's manageable. No feat is required and no eternal willpower diet: it's enough to change the habits themselves — and then a normal weight becomes not a goal you battle for, but simply something that is, because that's how you live.",

        // Story — chapter 1, introduction
        "story.intro.p1" => "Hi. This is the «Slimming Story» app. It is made specifically for people who can't lose weight and have a bit of a problem with extra pounds.",
        "story.intro.p2" => "Having a low body-fat percentage is very important. So you need to be able not just to «get slim for the summer», but also to keep your weight steady.",
        "story.intro.p3" => "It's not hard.",
        "story.intro.p4" => "But it does take some habit.",
        "story.intro.p5" => "Building the right habits is very boring and sometimes very unpleasant. So I suggest we play a little.",
        "story.intro.p6" => "This app lets you complete simple tasks in a playful way and gradually move towards a healthy weight, a healthy body and active longevity.",
        "story.intro.p7" => "Each chapter of the story unlocks new tasks, and by completing them you unlock new chapters, gradually learning how to eat properly. The tasks help you absorb new habits and a new lifestyle.",
        "story.intro.p8" => "This chapter holds the most important task of the whole game. Imagine how you will feel in your new, slim body.",
        "story.intro.task_label" => "Chapter task",
        "story.intro.task_desc" => "Imagine how I will look, how other people will look at me, what clothes I will be able to afford and how gorgeous I will look in photos.",
        "story.intro.checkbox" => "I want a new body",
        "story.intro.unlocked_hint" => "Great! The next section — «Let's set up the app» — is now unlocked.",
        "story.intro.photo_task_label" => "Task · before photos",
        "story.intro.photo_desc" => "Take three photos of yourself — front, side and back — in tight or minimal clothing, against a plain wall. Repeat them over time to watch your body change. The photos stay on your device only.",
        "story.intro.photo_check" => "Take front, side and back photos",
        "progress.title" => "Progress photos",
        "progress.subtitle" => "Front, side and back. Stored on your device only.",
        "progress.capture" => "Take photo",
        "progress.tips_title" => "Recommendations",
        "progress.tip_bg" => "Try to shoot against a plain background.",
        "progress.tip_height" => "Place the camera at chest level.",
        "progress.take_photo" => "Camera",
        "progress.from_gallery" => "Gallery",
        "progress.history" => "History",
        "progress.empty" => "No photos yet.",
        "progress.pose_front" => "Front",
        "progress.pose_side" => "Side",
        "progress.pose_back" => "Back",

        // Story — chapter 1, set up the app
        "story.setup.intro" => "The app has 4 sections:",
        "story.setup.s_story" => "Story — where you gain new knowledge and tasks.",
        "story.setup.s_diary" => "Diary — where you'll log food, weight and steps.",
        "story.setup.s_recipes" => "Recipes — for those who like to cook.",
        "story.setup.s_settings" => "Settings — which let us use the app comfortably.",
        "story.setup.task_intro" => "Right now our task is to make sure that:",
        "story.setup.check_lang_line" => "you understand everything written here — the language is set correctly;",
        "story.setup.check_notif_line" => "notifications reach you.",
        "story.setup.instructions" => "Open «Settings» in the menu and press the «Check notifications» button there. It is only active when notifications are allowed. When the notification arrives — tap it. That's how one of the tasks gets done.",
        "story.setup.task_label" => "Section tasks",
        "story.setup.checkbox_lang" => "I set the language to suit me",
        "story.setup.notif_status_done" => "Notification received",
        "story.setup.notif_status_pending" => "No notification yet",
        "story.setup.sex_status_done" => "Sex set in settings",
        "story.setup.sex_status_pending" => "Set your sex in settings",
        "story.setup.next_unlocked" => "Great! The next section — «The accounting of weight loss» — is now available.",
        "story.setup.open_settings" => "Open settings",

        // Story — chapter 1, accounting
        "story.acc.p1" => "When things go badly — say, in business, or when excess weight piles up — people very often turn to accounting. Just to figure out where exactly things are going wrong.",
        "story.acc.p2" => "Rich people often don't count their money. And thin people don't count calories.",
        "story.acc.p3" => "But to understand where our habits fail and why we run into problems, we need to count calories a little.",
        "story.acc.p4" => "There are many methods for this — people keep food diaries, use the plate method. In this app we tried to make food-diary keeping as easy as possible. It has the basic set of features needed to keep a diary successfully, and nothing extra that only gets in the way.",
        "story.acc.p5" => "Here we'll count not only calories, but also track two things:",
        "story.acc.li_weight" => "how exactly your weight changes;",
        "story.acc.li_calories" => "how you spend your calories.",
        "story.acc.p6" => "We encourage you to see this counting as a temporary measure — a kind of treatment course that will greatly help your health and remove the risks of an early decline.",
        "story.acc.p7" => "You can keep track in different ways. For example, you can log everything in great detail and without mistakes — and we encourage you to try. But don't suffer or scold yourself if something doesn't work out. Long-held habits don't change easily. So treat yourself with love and understanding while you walk this path.",
        "story.acc.task_label" => "Section tasks",
        "story.acc.task1" => "Go to settings and enable the weigh-in reminder.",
        "story.acc.task2" => "After the notification arrives — take your first measurement.",
        "story.acc.task3" => "Weigh in every day for a week straight, trying to leave fewer empty checkboxes than yesterday.",
        "story.acc.streak_label" => "Days in a row",
        "story.acc.next_unlocked" => "The «My first food entries» section is now open.",
        "story.acc.chapter_unlocked" => "Great! Chapter 2 is now open.",
        "story.acc.push_first_weigh" => "\u{1f389} First measurement done! Keep it up!",
        "story.acc.howto_title" => "How to record your weight",
        "story.acc.howto" => "Open the «Diary», the «Today» tab. Top-left there's a «Weight» widget — tap it. A window opens with the chart and a table of your measurements. Tap «Weigh in», enter your weight and save. Weight can be recorded once a day.",

        // Story — chapter 1, first food entries
        "story.ff.p1" => "Food is the number-one reason we gain weight. Adjusting your diet always pays off — it lets you both keep your weight steady and avoid risking your health.",
        "story.ff.p2" => "So we need to know what we ate: was it enough, was it too much?",
        "story.ff.p3" => "And at the early stages there's no way around counting calories. First you need to understand what mistakes you make in your diet, and only then correct them.",
        "story.ff.ways_intro" => "There are three ways to log food in this app:",
        "story.ff.way1" => "Find it online — just type the name and the app fills in all the nutrients.",
        "story.ff.way2" => "Photograph the label — the app reads the calories/protein/fat/carbs for you (if you're lazy).",
        "story.ff.way3" => "Enter it by hand — yourself, if you don't want to wait or have no internet.",
        "story.ff.howto_open" => "Open the «Diary» and tap the big green button:",
        "story.ff.step_new" => "Then tap «New food».",
        "story.ff.step_name" => "Type a product name — e.g. apple, rice or a Snickers — and tap «Fill in nutritional value».",
        "story.ff.step_add" => "Then tap «Add» and specify the weight.",
        "story.ff.step_more" => "After that you can look up something else right away.",
        "story.ff.task_label" => "Section task",
        "story.ff.task" => "Try entering some food yourself.",
        "story.ff.next_unlocked" => "The «Activity and weight» section is now open.",
        "story.ff.open_diary" => "Open the diary",

        // Story — chapter 1, activity and weight
        "story.act.p1" => "The second crucial factor for a beautiful and healthy body is your activity level.",
        "story.act.p2" => "And specifically everyday activity — literally, the number of your steps.",
        "story.act.p3" => "People who need to lose weight are very often prescribed high activity levels — and for good reason.",
        "story.act.p4" => "Lots of low-intensity activity — walking, strolls, dancing — helps burn your calories. Literally: the more you walk, the more you burn.",
        "story.act.p5" => "Walking has another upside — it keeps all your muscles toned. There's a huge body of research confirming that people who walk stay healthy longer.",
        "story.act.p6" => "So we'll definitely be recording your step count.",
        "story.act.p7" => "We need to look at how much you eat, compare it with your activity level and with your weight dynamics — and from all of that decide how exactly to act.",
        "story.act.p8" => "Regular step tracking, proper weigh-ins and understanding what you eat — these are the three pillars of weight-loss accounting that let you understand everything about your own body.",
        "story.act.task_label" => "Section tasks",
        "story.act.task1" => "Set up the steps reminder.",
        "story.act.task2" => "Record your steps at least once.",
        "story.act.task3" => "Record your steps for a week straight, no gaps.",
        "story.act.streak_label" => "Days in a row",
        "story.act.next_unlocked" => "The «I'm cooking» section is now open.",
        "story.act.record_steps" => "Record steps",
        "story.act.howto_title" => "How to record your steps",
        "story.act.howto" => "In the «Diary», on the «Today» tab, top-right there's a «Steps» widget — tap it. In the window that opens tap «Record steps», choose «today» or «yesterday», enter the number and save. Steps can be recorded once a day.",

        // Story — chapter 1, I cook
        "story.cook.p1" => "Many people struggle to log cooked food. Almost no app lets you do it correctly, accurately and conveniently.",
        "story.cook.p2" => "We believe the right choice of products and minimal time spent on cooking and tracking are the main factors of weight-loss accounting.",
        "story.cook.p3" => "Any cooking is done in two steps:",
        "story.cook.step1" => "First create a recipe and give it a name — e.g. «Fried potatoes». Add the ingredients: the mass of raw potatoes, the weight of oil, onion (if you add it).",
        "story.cook.step2" => "When the dish is ready, weigh the finished product (everything that was in the pan or pot). Tap «Finalize» and enter the mass of the finished dish. After that you can add it to your diary — its calories are calculated automatically. Just find it in the add list.",
        "story.cook.task_label" => "Section tasks",
        "story.cook.task1" => "Cook a dish and enter it into the app.",
        "story.cook.task2" => "Add the cooked dish to your diary.",
        "story.cook.next_unlocked" => "The «My food with bones» section is now open.",
        "story.cook.open_recipes" => "Open recipes",

        // Story — chapter 1, My food with bones
        "story.bones.p1" => "Sometimes the food on our plate contains pits or bones.",
        "story.bones.p2" => "Often you can just ignore them, but some people like a bit more control in their lives. For them we made it possible to quickly enter the waste in the food you ate.",
        "story.bones.p3" => "To do this, tap the mass of an added entry and check the «Didn't eat it whole» box. Then enter an approximate or precisely measured value.",
        "story.bones.p4" => "Try this feature and use it if you like it.",
        "story.bones.task_label" => "Section task",
        "story.bones.task1" => "Weigh some food with pits — cherries, for example — and enter that value in the field.",
        "story.bones.next_unlocked" => "The «A party or a restaurant» section is now open.",
        "story.bones.open_diary" => "Open the diary",

        // Story — chapter 1, A party or a restaurant
        "story.rest.p1" => "Sooner or later each of us ends up somewhere the food is delicious and high-calorie. On holidays, for example. Or when you go to a restaurant.",
        "story.rest.p2" => "Unfortunately, any food cooked away from home has a terrible margin of error and is very, very hard to account for correctly. But we follow these principles:",
        "story.rest.method1" => "When logging food, we can enter its calories — for example by asking for the nutrition card. Some restaurants will give it to you.",
        "story.rest.method2" => "If there's no card, you can look the dish up by name online (as you've done before). All that's left is to enter the weight.",
        "story.rest.p3" => "Both methods give imprecise data, but even bad data is better than logging nothing.",
        "story.rest.p4" => "Most important: when adding the food to your diary, mark it as restaurant food (you can do the same for food cooked by your relatives).",
        "story.rest.p5" => "The app will automatically add a small calorie buffer — because it's normal practice in any restaurant to add a bit of calorie-rich oil to any dish.",
        "story.rest.p6" => "We're against harsh restrictions and the «never eat at a restaurant» rule. Our job is simply to understand what consequences this carries and how to live with it if we want to be healthy. Rest assured, by the end of your journey spontaneity and the ability to eat any food will return to you.",
        "story.rest.p7" => "The app will give you recommendations as you go. Just try to apply them and you'll do great.",
        "story.rest.task_label" => "Section task",
        "story.rest.task1" => "Eat a restaurant food you love — fries, a burger. Go somewhere or order online. Then log the restaurant food.",
        "story.rest.next_unlocked" => "Congratulations — you've completed all the tasks of chapter 1!",
        "story.rest.open_diary" => "Open the diary",

        // Story — chapter 1, What's next (paywall)
        "story.next.p1" => "You've reached the end of chapter 1. The journey continues — but to go further, two things are needed.",
        "story.next.p2" => "First, finish the remaining tasks across this chapter's sections. Second, support the project so we can keep going (and keep the AI on).",
        "story.next.p3" => "AI features are free for your first 14 days. After that, an active subscription keeps food recognition — and your progress through the Story — going.",
        "paywall.loading" => "Loading…",
        "paywall.status_trial" => "Trial",
        "paywall.status_paid" => "Subscription active",
        "paywall.status_expired" => "Subscription expired",
        "paywall.days_left" => "days left",
        "paywall.code_placeholder" => "Code word",
        "paywall.pay_button" => "Pay",
        "paywall.paying" => "Processing…",
        "paywall.invalid_code" => "Wrong code word.",
        "paywall.success" => "Thank you! Your subscription is active.",
        "paywall.back_to_story" => "Back to the Story",

        _ => "???",
    }
}

fn ru(key: &str) -> &'static str {
    match key {
        // Навигация
        "nav.story" => "История",
        "nav.diary" => "Дневник",
        "nav.recipes" => "Рецепты",
        "nav.settings" => "Настройки",
        "nav.support" => "Поддержка",

        // Чат
        "chat.requesting" => "Запрос",
        "chat.thinking" => "Думаю",
        "chat.answer" => "Ответ",
        "chat.tool_running" => "Запускаю инструмент",
        "chat.input_placeholder" => "Сообщение в поддержку…",
        "chat.send" => "Отправить",
        "chat.attach_image" => "Прикрепить изображение",
        "chat.record_voice" => "Записать голос",
        "chat.recording" => "Запись…",
        "chat.stop_recording" => "Стоп",
        "chat.recording" => "Запись…",
        "chat.escalated_banner" => "Перевожу на живого оператора…",
        "chat.attached_image" => "[вложение: изображение]",
        "chat.attached_voice" => "[вложение: голос]",
        "chat.empty" => "Сообщений пока нет. Спросите, как пользоваться приложением, или опишите проблему — помогу оформить баг-репорт.",
        "chat.mic_denied" => "Доступ к микрофону запрещён",

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

        // Meal-split section headers
        "meal.breakfast" => "Завтрак",
        "meal.snack_morning" => "Утренний перекус",
        "meal.lunch" => "Обед",
        "meal.snack_afternoon" => "Дневной перекус",
        "meal.dinner" => "Ужин",
        "meal.snack_night" => "Ночной перекус",
        "meal.breakfast_sub" => "утренний жор",
        "meal.lunch_sub" => "дневной жор",
        "meal.dinner_sub" => "ночной жор",

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
        "diary.duplicate" => "Дублировать",
        "diary.edit" => "Изменить",
        "diary.edit_product" => "Изменить продукт",
        "diary.repeat_today" => "Повторить сегодня",
        "diary.no_entries" => "Нет записей за этот день",
        "diary.per_week" => "в неделю",
        "diary.empty_today_1" => "Здесь будет список того, что вы съели. Пока что здесь нет ни одной записи.",
        "diary.empty_today_2" => "Чтобы добавить запись — нажмите кнопку ниже.",
        "diary.empty_past" => "не было ни одной записи. Этот день прошёл, и в него нельзя добавить еду. Еду можно добавить только сегодня.",

        // Суммаризация дня / недели
        "summary.day_title" => "Итог дня",
        "summary.generating" => "Готовлю итог…",
        "summary.gen_failed" => "Не удалось подготовить оценку — нажмите, чтобы повторить.",
        "summary.good_title" => "Что было сделано хорошо",
        "summary.improve_title" => "Что надо улучшить",
        "summary.all_good" => "Вы молодец, так держать!",
        "summary.facts_title" => "Факты",
        "summary.facts_veg_fruit" => "Овощи и фрукты",
        "summary.regenerate" => "Переделать оценку",
        "summary.source" => "источник",
        "summary.good_weight_steps" => "Вы записали вес и шаги — это круто.",
        "summary.good_diary" => "Вы ведёте дневник питания.",
        "summary.good_restaurant" => "Вы записываете еду даже в ресторане.",
        "summary.good_snack" => "Вы выбрали низкокалорийную закусь — отличный способ закрыть порыв пожевать, не сорвав день.",
        "summary.good_no_cal_drink" => "Сегодня без калорийных напитков — а это самые незаметные калории, которые легко выпить.",
        "summary.good_evening_protein" => "Вечером много белка — сытость держится дольше, и меньше ночных набегов на холодильник.",
        "summary.improve_weighing" => "Улучшайте качество взвешивания: чем оно выше, тем точнее понятно, в профиците вы или в дефиците.",
        "summary.improve_steps" => "Свыше 7000 шагов в день дают существенное улучшение здоровья.",
        "summary.improve_drink" => "Затесался калорийный напиток: замените его на версию без сахара или на сам фрукт — такие калории почти не дают сытости.",
        "summary.good_under_planka" => "Сегодня вы уложились в свою планку калорий — это и есть тот темп, при котором уходит жир, а мышцы остаются.",
        "summary.improve_over_planka" => "Следите за планкой калорий — особенно если вышли выше: именно так незаметно теряется дефицит. Есть сильно меньше тоже плохо, но перебор — главное, что стоит поправить.",
        "summary.week_button" => "Отчёт недели",
        "summary.week_title" => "Отчёт недели",
        "summary.week_pending" => "Отчёт недели будет посчитан",

        // Модалка добавления в дневник
        "diary_add.title" => "Добавить в дневник",
        "diary_add.search" => "Поиск",
        "diary_add.new" => "Новый",
        "diary_add.search_placeholder" => "Найти продукт...",
        "diary_add.done" => "Готово",
        "diary_add.close" => "Закрыть",
        "diary_add.how_much" => "Сколько?",
        "diary_add.add" => "Добавить",
        "diary_add.cancel" => "Отмена",
        "diary_add.nothing_found" => "Ничего не найдено",
        "diary_add.new_food" => "Новая еда",
        "diary_add.more" => "Ещё",
        "diary_add.products" => "продуктов",
        "diary_add.add_new_food" => "Добавить новый продукт",
        "diary_add.back" => "Дневник",

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
        "settings.version" => "Версия",
        "settings.version_current" => "Сборка:",
        "settings.version_up_to_date" => "Актуальная версия",
        "settings.version_available" => "Доступна новая версия",
        "settings.version_update" => "Обновить",
        "settings.danger_zone" => "Опасные дела",
        "settings.danger_reset_story" => "Удалить моё продвижение в истории",
        "settings.danger_confirm_story" => "Удалить весь ваш прогресс в истории? Записанные данные останутся.",
        "settings.danger_delete_diary" => "Удалить данные дневника",
        "settings.danger_delete_old" => "Удалить данные старше 1 года",
        "settings.danger_confirm_old" => "Удалить записи дневника старше 1 года? Это необратимо.",
        "settings.danger_delete_all" => "Удалить все данные",
        "settings.danger_confirm_all" => "Удалить ВСЕ записи дневника? Это необратимо.",
        "settings.nutrient_placeholder" => "Omega 3, Fiber...",

        // Редактор продукта
        "food_editor.product_name" => "Название продукта",
        "food_editor.add_photo" => "Добавить фото этикетки",
        "food_editor.add_more_photo" => "Добавить ещё фото",
        "food_editor.photo_hint" => "Снимайте таблицу КБЖУ крупно, чтобы она занимала весь кадр — мелкий или далёкий текст распознаётся плохо.",
        "food_editor.ai_uploading" => "Загрузка фото\u{2026}",
        "food_editor.ai_queue" => "В очереди:",
        "food_editor.ai_recognizing" => "Распознаётся\u{2026}",
        "food_editor.ai_timeout" => "Распознавание не успело — попробуйте позже.",
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
        "waste.not_whole" => "Не съел целиком",
        "waste.placeholder" => "Отходы",
        "restaurant.eaten_out" => "Ресторанная еда",
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
        "settings.sex" => "Пол",
        "settings.sex_female" => "Женский",
        "settings.sex_male" => "Мужской",
        "settings.sex_why" => "Зачем это нужно: для женщин некоторые нормы нутриентов мягче, а вес естественно колеблется в течение менструального цикла — зная пол, приложение точнее отслеживает реальные изменения веса.",

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
        "settings.check_notifications" => "Проверить уведомления",
        "settings.notif_enable_check" => "Включить и проверить",
        "settings.notif_check" => "Проверить",
        "settings.notif_push_task" => "\u{1f514} Нажмите, чтобы выполнить задание",
        "settings.notif_push_plain" => "\u{2705} Уведомления работают!",
        "settings.sending" => "Отправляем…",
        "settings.push_enable" => "Включить уведомления",
        "settings.push_disable" => "Отключить уведомления",
        "settings.push_enabled" => "Уведомления включены",
        "settings.push_not_supported" => "Push-уведомления не поддерживаются в этом браузере",
        "settings.schedule" => "Расписание уведомлений",
        "settings.weigh_in" => "Взвешивание",
        "settings.breakfast" => "Завтрак",
        "settings.lunch" => "Обед",
        "settings.dinner" => "Ужин",
        "settings.steps" => "Шаги",

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
        "weight.add" => "Взвеситься",
        "weight.edit" => "Изменить вес за сегодня",
        "weight.once_per_day" => "Одна запись в день — её можно изменить",
        "weight.col_date" => "Дата",
        "weight.col_time" => "Время",
        "weight.col_quality" => "Качество",
        "weight.col_weight" => "Вес",
        "weight.saved" => "Сохранено!",
        "weight.unit_kg" => "кг",
        "weight.unit_lbs" => "фунты",
        "weight.widget_title" => "Вес",
        "weight.widget_placeholder" => "Здесь будет график вашего веса. Пока что график не изобразить, потому что слишком мало данных. Когда появится хотя бы три измерения, график будет нарисован.",
        "weight.trend.title" => "Тренд · 14 дней",
        "weight.trend.down" => "Снижается",
        "weight.trend.up" => "Растёт",
        "weight.trend.unclear" => "Тренд пока не ясен",
        "weight.trend.insufficient" => "Недостаточно данных для тренда",
        "weight.trend.preliminary" => "предварительно",
        "weight.trend.week" => "нед",
        "weight.trend.confidence" => "достоверность",
        "weight.cycle.label" => "Месячные",
        "weight.cycle.none" => "цикл не обнаружен",
        "weight.cycle.insufficient" => "пока недостаточно данных",
        "weight.cycle.day_short" => "дн",
        "weight.cycle.decycled" => "Вес без месячных",

        "steps.title" => "Шаги",
        "steps.for_today" => "Записываю шаги вечером за СЕГОДНЯ",
        "steps.for_yesterday" => "Записываю шаги с утра за ВЧЕРА",
        "steps.input_placeholder" => "Шаги",
        "steps.unit" => "шагов",
        "steps.save" => "Сохранить",
        "steps.add" => "Записать шаги",
        "steps.edit" => "Изменить шаги за сегодня",
        "steps.once_per_day" => "Одна запись в день — её можно изменить",
        "steps.col_steps" => "Шаги",

        // История
        "story.title" => "История",
        "story.chapter" => "Глава",
        "story.sections_opened" => "Открыто секций",
        "story.tasks_done" => "Выполнено заданий",
        "story.locked_hint" => "Для открытия выполните задания:",
        "story.ch1.title" => "Приятно познакомиться",
        "story.ch1.intro" => "Введение",
        "story.ch1.setup" => "Для начала настроим приложение",
        "story.ch1.accounting" => "Бухгалтерия похудения",
        "story.ch1.first_food" => "Мои первые записи еды",
        "story.ch1.activity" => "Активность и вес",
        "story.ch1.cooking" => "Я готовлю",
        "story.ch1.bones" => "Моя еда с костями",
        "story.ch1.restaurant" => "Праздник или ресторан",
        "story.ch1.next" => "Что дальше",
        "story.next.p_intro" => "Сейчас самое важное — освоиться с приложением и день за днём улучшать навык подсчёта.",
        "story.next.rules_label" => "Очень важно придерживаться простых правил:",
        "story.next.rule1" => "Сначала записать — потом съесть.",
        "story.next.rule2" => "Если записал — ешь целиком.",
        "story.next.rule3" => "Шаги и вес — это очень важно.",
        "story.next.p_discipline" => "Когда вы дисциплинированно встанете на рельсы похудения и будете точно выполнять задания, программа сама будет подсказывать, куда двигаться дальше.",
        "story.next.focus_label" => "Поэтому самое важное:",
        "story.next.focus1" => "Упрощать подсчёт — чем меньше сложной и ресторанной еды, тем лучше.",
        "story.next.focus2" => "Убрать алкоголь. Когда наладите дисциплину — его можно будет вернуть, но сейчас лучше убрать.",
        "story.next.focus3" => "Каждый день улучшать качество подсчёта. Мы не идеальны и постоянно ошибаемся — но всегда стремимся считать точнее.",
        "story.next.p_goals" => "Чуть позже появятся первые цели подсчёта — калории, белки, овощи и фрукты. Но сейчас важно просто вести подсчёт.",
        "story.next.p_report" => "И ещё: итог за вчерашний день уже можно посмотреть. Откройте дневник, пролистайте на прошлый день — итог дня будет под списком еды.",
        "story.next.open_diary" => "Открыть дневник",
        "story.ch2.task_notif" => "Проверить, что уведомления работают",
        "story.ch2.task_weight" => "Взвешиваться 7 дней подряд",
        "story.ch2.task_subscription" => "Иметь активную подписку",
        "story.ch2.unlocked" => "Глава 2 открыта! Секции скоро.",
        "story.ch2.title" => "Аппетит",
        "story.ch2.soon" => "скоро",
        "story.ch2.s1" => "Основная ошибка худеющих",
        "story.ch2.s2" => "Фрукты и овощи",
        "story.ch2.s3" => "Белок — лучший помощник",
        "story.ch2.s4" => "Низкокалорийная закусь",
        "story.ch2.s5" => "Соки и питьё",
        "story.ch2.s6" => "Сколько раз в день надо есть",
        "story.ch2.s7" => "Еда на ночь",

        // История — глава 2 «Аппетит», секция 1: основная ошибка
        "story.ch2.mistake.p1" => "Когда человек решает похудеть, первое, что он делает почти на автомате, — начинает есть меньше. Меньше порция, меньше тарелка, реже подходы к холодильнику. Логика кажется железной: меньше еды — меньше калорий — меньше вес. И первые дни это даже работает.",
        "story.ch2.mistake.p2" => "Проблема в том, что желудок не считает калории. Он считает объём. Пустой желудок шлёт в мозг сигнал «я голоден» независимо от того, сколько в нём было энергии, — и рано или поздно этот сигнал побеждает любую силу воли. Поэтому диеты «на маленьких порциях» почти всегда заканчиваются срывом: не потому что человек слабый, а потому что он воюет с физиологией.",
        "story.ch2.mistake.p3" => "Выход звучит парадоксально: еды нужно не меньше, а больше — столько, чтобы желудок был полным и довольным. Весь фокус в том, чтобы при этом резко снизить калорийность того, что вы едите. Полная тарелка низкокалорийной еды насыщает не хуже плотной, а голода не оставляет — и на этом строится всё остальное.",
        "story.ch2.mistake.task_label" => "Задание на главу",
        "story.ch2.mistake.task" => "Вести дневник еды 7 дней подряд",
        "story.ch2.mistake.streak_label" => "Серия",
        "story.ch2.mistake.next_unlocked" => "Отлично — следующая секция открыта.",
        "story.ch2.mistake.open_diary" => "Открыть дневник",
        "story.ch2.mistake.no_data" => "Пока нет записей в дневнике — начните записывать еду.",

        // История — глава 2, секция 2: фрукты и овощи
        "story.ch2.veg.p1" => "Овощи и фрукты — это главный инструмент, чтобы наполнить тарелку, не наполнив её калориями. В них много воды и клетчатки, то есть объёма, который занимает место в желудке и даёт сытость, и при этом совсем немного энергии. Плюс витамины и всё то, ради чего их и советуют есть.",
        "story.ch2.veg.p2" => "Ориентир простой: 600–800 граммов овощей и фруктов в день. Мужчине — ближе к верхней границе, женщине — к нижней, дальше подстраивайте под себя. Это не «полезная добавка» к рациону, а его основа: именно за счёт этого объёма получается есть досыта и при этом оставаться в дефиците.",
        "story.ch2.veg.target_label" => "Вчера овощей и фруктов",
        "story.ch2.veg.no_data" => "Пока нет данных за вчера.",

        // История — глава 2, секция 3: белок
        "story.ch2.protein.p1" => "Если овощи дают объём, то белок даёт сытость другого рода — ту, что держится часами. Из всех макронутриентов он насыщает сильнее всего: после белковой еды есть хочется заметно позже, и тянет на перекусы меньше.",
        "story.ch2.protein.p2" => "Поэтому на похудении белок не урезают, а наоборот — выводят в приоритет. Рабочий ориентир — от 1 грамма на килограмм веса в день, и в большинстве случаев имеет смысл идти выше. Это тот редкий случай, когда «максимизировать» — верная стратегия: лишним белок здесь не будет, а вот недобор почти гарантированно обернётся голодом и потерей мышц.",
        "story.ch2.protein.goal_label" => "Цель по белку",
        "story.ch2.protein.goal_set" => "Цель по белку установлена — смотрите её в шапке дневника.",
        "story.ch2.protein.need_weight" => "Сначала взвесьтесь, чтобы мы могли установить цель по белку.",
        "story.ch2.protein.open_weight" => "Взвеситься",

        // История — глава 2, секция 4: низкокалорийная закусь
        "story.ch2.snack.p1" => "Голод редко приходит вовремя. Он накрывает между делами, вечером перед сериалом, в момент, когда до нормального приёма пищи ещё далеко, — и вот тут решается, сорвётесь вы или нет. Поэтому стоит заранее держать под рукой то, чем можно перекусить почти без последствий.",
        "story.ch2.snack.p2" => "Попкорн без масла, огурцы, помидоры, яблоки — в них мало калорий и много воды, ими можно хрустеть почти до отвала и почти не сдвинуть свой дневной баланс. Это не «запрещёнка наоборот», а нормальная еда, которая закрывает порыв что-нибудь сжевать.",
        "story.ch2.snack.p3" => "Единственное правило — всё равно вносить это в дневник. «Это же огурчик, что там считать» — и так незаметно набегает то, что вы потом не можете объяснить на весах. Считается всё, даже мелочь.",
        "story.ch2.snack.task_label" => "Задание на главу",
        "story.ch2.snack.task" => "Записать низкокалорийную закусь — и увидеть её в завтрашней оценке",
        "story.ch2.snack.no_report" => "Оценка за вчера ещё не готова — она появляется, когда день закончился.",

        // История — глава 2, секция 5: соки и питьё
        "story.ch2.drinks.p1" => "Самый незаметный способ выпить дневную норму калорий — это именно выпить. Сладкая газировка, пакетированный сок, латте с сиропом — это калории, которые не дают почти никакой сытости. Организм их будто не замечает: вы приняли эквивалент полноценного приёма пищи, а через полчаса снова голодны.",
        "story.ch2.drinks.p2" => "Поэтому первым делом убираем из питья калории. Обычную газировку меняем на версию без сахара — да, сахзамы тут не враг, бояться их не нужно; достаточно найти пару вкусов, которые вам реально нравятся. Сок заменяем на сам фрукт — так вы получите ещё и клетчатку, и сытость. Вода, кофе, чай — пейте сколько хотите, просто уберите или сведите к минимуму то, что добавляет калории: сахар и сливки.",
        "story.ch2.drinks.task_label" => "Задание на главу",
        "story.ch2.drinks.task" => "Обойтись без калорийных напитков — чтобы их не было во вчерашней оценке",
        "story.ch2.drinks.effort" => "Видим старание — некалорийный напиток. Вписывать его необязательно.",

        // История — глава 2, секция 6: сколько раз в день есть
        "story.ch2.meals.p1" => "Здесь нет священного числа. Три приёма пищи, шесть, два — всё работает, если в сумме вы остаётесь в дефиците. Поэтому первый принцип — сделать день максимально удобным лично вам. Любите перекусывать часто и отвлекаться на еду — дробите. Не любите возиться по пять раз — не дробите.",
        "story.ch2.meals.p2" => "Базовая рекомендация всё же одна: старайтесь не доводить себя до сильного голода. Подступающий аппетит легко сбить сладкой некалорийной водичкой между приёмами, а вот терпеть до полуобморока — прямой путь к срыву. И обязательно утоляйте голод в конце дня: ложиться спать голодным — плохая идея, именно отсюда растут ночные набеги на холодильник.",
        "story.ch2.meals.p3" => "А если спросить, что работает лучше всего, — это три приёма пищи. Завтрак, обед, ужин — пожалуй, лучшее, что человечество придумало для ритма дня, и для похудения это отличный каркас.",
        "story.ch2.meals.advice" => "И практическое правило: сокращайте перекусы, а когда тянет что-нибудь сжевать — потянитесь к напитку. Теперь дневник будет группировать записи по приёмам пищи, чтобы вы видели картину дня сразу.",
        "story.ch2.meals.unlocked" => "Готово — дневник теперь разбивает день на приёмы пищи.",

        // История — глава 2, секция 7: еда на ночь
        "story.ch2.night.p1" => "Начнём с главного и неожиданного: есть на ночь не вредно. Можно поесть прямо перед сном, можно не есть вовсе — на похудение это не влияет никак. Жир откладывается не «оттого что поздно», а оттого, что калорий за день пришло больше, чем ушло. «После шести нельзя» — миф.",
        "story.ch2.night.p2" => "Но есть пара честных оговорок. Первая — про срывы: многие просто не могут устоять и доедают ночью. Если это про вас, лучшая страховка — плотный, богатый белком последний приём пищи: сытость продержится дольше, и тянуть к холодильнику будет меньше.",
        "story.ch2.night.p3" => "Вторая — медицинская. При ряде проблем (например, рефлюксе) врачи советуют не ложиться раньше чем через три часа после ужина. Это уже не про похудение, а про здоровье и спокойный сон — и этой рекомендации стоит придерживаться.",
        "story.ch2.night.feedback_label" => "Сегодня вечером",
        "story.ch2.night.feedback_good" => "Вечером много белка — отлично: дольше сытость, меньше ночных набегов.",
        "story.ch2.night.feedback_hint" => "Старайтесь делать последний приём пищи богатым белком — он дольше держит сытость и смягчает вечернюю тягу к холодильнику.",

        // История — глава 3 «Зачем надо худеть?»
        "story.ch3.title" => "Зачем надо худеть?",
        "story.ch3.unlocked" => "Глава 3 открыта.",
        "story.ch3.locked_hint" => "Для открытия пройдите все секции главы 2.",
        "story.ch3.s1" => "Зачем нам вообще жир?",
        "story.ch3.s2" => "Как выглядит «красивый человек»?",
        "story.ch3.s3" => "Что такое физиологический минимум жира?",
        "story.ch3.s4" => "Разница между «худым» и «обезжиренным»",
        "story.ch3.s5" => "Жизнь без жира",

        // История — глава 3, секция 1: зачем нам вообще жир
        "story.ch3.fat.p1" => "Жир — это не ошибка организма и не наказание за слабость. Это древнейшая система выживания. Сотни тысяч лет еда была нестабильной: то густо, то пусто, — и выживали те, кто умел запасать энергию впрок, чтобы протянуть голодную зиму или неурожай. Способность копить жир была суперспособностью, и эволюция отточила её до совершенства.",
        "story.ch3.fat.p2" => "Сломалось всё в последние несколько десятилетий. Еды стало не просто достаточно — её стало в избытке, она дешёвая, калорийная и доступна в любую минуту. Голодная зима больше не наступает, а система запасания работает как ни в чём не бывало. Механизм, спасавший наших предков, в мире изобилия превратился в проблему.",
        "story.ch3.fat.p3" => "И проблема не только в том, как это выглядит. Жировая ткань — не пассивный склад, а полноценный эндокринный орган: она вырабатывает гормоны и сигнальные вещества. Когда жира становится слишком много, эти сигналы сбиваются, и одно из ключевых последствий — инсулинорезистентность: клетки перестают нормально слышать инсулин. Отсюда тянется длинный шлейф проблем со здоровьем. То есть худеть стоит не ради картинки, а потому что лишний жир активно вмешивается в работу тела.",
        "story.ch3.fat.goal_label" => "Дневная планка калорий",
        "story.ch3.fat.kcal_unit" => "ккал",
        "story.ch3.fat.goal_set" => "Дневная планка калорий установлена — смотрите её в шапке дневника.",
        "story.ch3.fat.need_diary" => "Поведите дневник еды несколько дней, чтобы мы могли установить планку калорий.",
        "story.ch3.fat.open_diary" => "Открыть дневник",

        // История — глава 3, секция 2: как выглядит красивый человек
        "story.ch3.beauty.p1" => "О красоте спорят тысячелетиями, но если присмотреться, в основе почти всегда лежат две довольно простые вещи. Первая — кости. Если лицевой скелет развился нормально, черты получаются гармоничными, и это во многом дано от природы.",
        "story.ch3.beauty.p2" => "А вот вторая вещь полностью в наших руках. Возьмите человека с нормальными чертами лица и уберите лишний жир, оставив здоровый физиологический минимум, — и он почти наверняка будет выглядеть красиво. Не потому что «похудел», а потому что проступает то, что жир обычно скрывает: линии лица, очертания тела, мышечный каркас.",
        "story.ch3.beauty.p3" => "По сути красота тела складывается из двух слагаемых: отсутствие лишнего жира и наличие мышц под кожей. Это важная мысль, к которой мы ещё вернёмся: цель — не «стать меньше», а проявить структуру, которая уже есть. И достигается это не голоданием, а сочетанием умеренного жира и нормально развитой мускулатуры.",

        // История — глава 3, секция 3: физиологический минимум жира
        "story.ch3.minimum.p1" => "У жира есть нижняя граница, ниже которой опускаться нельзя, — так называемый физиологический, незаменимый жир. Это не запас, а рабочая часть организма: он нужен для гормонов, нервной системы, защиты органов. Уйти ниже этой черты — значит не «досушиться», а навредить себе.",
        "story.ch3.minimum.p2" => "Важно понимать: у мужчин и женщин этот минимум разный, и у женщин он заметно выше. Это не недостаток, а норма — женская физиология, в том числе репродуктивная, просто требует больше жира, и попытки загнать себя в «мужские» цифры заканчиваются сбоем гормонов и цикла. Женщине стоит стремиться к здоровому уровню жира, а не к самому низкому возможному.",
        "story.ch3.minimum.p3" => "Тут и кроется частая ловушка. Здоровый минимум у девушки без мускулатуры может выглядеть совсем не так, как ей хочется, — не подтянуто, а скорее «пусто». И вывод из этого не «сбросить ещё», а ровно противоположный: дело не в избытке жира, а в нехватке мышц. Решается это не диетой, а залом.",

        // История — глава 3, секция 4: худой против обезжиренного
        "story.ch3.lean.p1" => "Весы показывают одну цифру, но за ней стоят очень разные вещи. Когда человек худеет резко и жёстко, он теряет не только жир. Вместе с ним уходит и мышца — а это именно то, что держит тело подтянутым, а обмен — живым. В итоге вес падает, но результат в зеркале и по самочувствию разочаровывает.",
        "story.ch3.lean.p2" => "Отсюда два принципа, которые лучше принять сразу. Первый: худеть стоит постепенно. Чем мягче темп, тем больше шансов, что уходит именно жир, а мышцы остаются на месте. Второй: одного «курса похудения» мало — надо менять сам образ жизни.",
        "story.ch3.lean.p3" => "Потому что иначе включается грустная арифметика. Сбросили на жёсткой диете, потеряв и жир, и мышцы; вернулись к старым привычкам — вес вернулся. Но возвращается он преимущественно жиром, а потерянную мышцу так просто не вернуть. Человек оказывается на том же весе, что и до диеты, только с худшим составом тела и без запаса по здоровью. Поэтому цель — не «похудеть», а перестроить жизнь так, чтобы новый вес держался сам.",

        // История — глава 3, секция 5: жизнь без жира
        "story.ch3.lifestyle.p1" => "Главная мысль всей главы простая: нормальный процент жира — это не пункт назначения, а побочный продукт. Его не «достигают» рывком, чтобы потом героически удерживать. Он сам собой получается у того, кто так живёт: нормально ест, двигается, спит, носит на себе мышцы.",
        "story.ch3.lifestyle.p2" => "Работает это и в обратную сторону. Ожирение почти всегда не случайность и не приговор генетики, а отражение того, как устроена жизнь изо дня в день. Что лежит в холодильнике, сколько человек двигается, как спит, чем заедает стресс — из этих мелочей и складывается цифра на весах.",
        "story.ch3.lifestyle.p3" => "Звучит сурово, но на самом деле это хорошая новость. Если вес — производная образа жизни, значит, он управляем. Не нужен подвиг и не нужна вечная диета на силе воли: достаточно поменять сами привычки — и тогда нормальный вес станет не целью, за которую вы бьётесь, а тем, что просто есть, потому что вы так живёте.",

        // История — глава 1, введение
        "story.intro.p1" => "Привет. Это приложение «Худеющая история». Оно сделано специально для тех, кто не может похудеть и у кого есть небольшие проблемы с лишним весом.",
        "story.intro.p2" => "Иметь низкий процент жира — это очень важно. Поэтому надо не просто уметь «худеть к лету», но и уметь держать свой вес в норме.",
        "story.intro.p3" => "Это не сложно.",
        "story.intro.p4" => "Но это требует некоторой привычки.",
        "story.intro.p5" => "Закрепление нужных привычек — это очень скучно, а иногда очень неприятно. Поэтому я предлагаю вам немного поиграть.",
        "story.intro.p6" => "Это приложение позволит вам в игровой форме выполнять нехитрые задания и постепенно приближаться к правильному весу, здоровому телу и активному долголетию.",
        "story.intro.p7" => "Каждая глава истории открывает вам новые задания, а при их выполнении вы открываете новые главы, постепенно начиная понимать, как правильно питаться. Задания помогают усваивать новые привычки и образ жизни.",
        "story.intro.p8" => "В этой главе — самое важное задание всей игры. Представьте себе, как вы будете ощущать себя в новом стройном теле.",
        "story.intro.task_label" => "Задание на главу",
        "story.intro.task_desc" => "Представить, как я буду выглядеть, как на меня будут смотреть другие люди, какую одежду я смогу себе позволить и как шикарно я буду выглядеть на фотографиях.",
        "story.intro.checkbox" => "Я хочу новое тело",
        "story.intro.unlocked_hint" => "Отлично! Открыта следующая секция — «Для начала настроим приложение».",
        "story.intro.photo_task_label" => "Задание · фото «до»",
        "story.intro.photo_desc" => "Сделайте три фото себя — спереди, сбоку и со спины — в облегающей или минимальной одежде, на фоне однотонной стены. Повторяйте их со временем, чтобы видеть, как меняется тело. Фото хранятся только на вашем устройстве.",
        "story.intro.photo_check" => "Сделать фото спереди, сбоку и со спины",
        "progress.title" => "Фото прогресса",
        "progress.subtitle" => "Спереди, сбоку и со спины. Хранятся только на вашем устройстве.",
        "progress.capture" => "Сделать фото",
        "progress.tips_title" => "Рекомендации",
        "progress.tip_bg" => "Постарайтесь снимать на однотонном фоне.",
        "progress.tip_height" => "Разместите камеру на уровне груди.",
        "progress.take_photo" => "Камера",
        "progress.from_gallery" => "Галерея",
        "progress.history" => "История",
        "progress.empty" => "Пока нет фото.",
        "progress.pose_front" => "Прямо",
        "progress.pose_side" => "Сбоку",
        "progress.pose_back" => "Со спины",

        // История — глава 1, настроим приложение
        "story.setup.intro" => "Приложение имеет 4 раздела:",
        "story.setup.s_story" => "История — где вы получаете новые знания и задания.",
        "story.setup.s_diary" => "Дневник — в него вы будете заполнять еду, вес и шаги.",
        "story.setup.s_recipes" => "Рецепты — это для тех, кто хочет готовить.",
        "story.setup.s_settings" => "Настройки — которые позволяют нам комфортно пользоваться приложением.",
        "story.setup.task_intro" => "Сейчас наша задача — убедиться в том, что:",
        "story.setup.check_lang_line" => "вы понимаете всё, что написано — язык выставлен правильно;",
        "story.setup.check_notif_line" => "уведомления вам приходят.",
        "story.setup.instructions" => "Откройте в меню пункт «Настройки» и нажмите там кнопку «Проверить уведомления». Она активна только если уведомления разрешены. Когда вы получите уведомление — нажмите на него. Так и будет выполнено одно из заданий.",
        "story.setup.task_label" => "Задания секции",
        "story.setup.checkbox_lang" => "Я настроил язык под себя",
        "story.setup.notif_status_done" => "Уведомление получено",
        "story.setup.notif_status_pending" => "Уведомление ещё не приходило",
        "story.setup.sex_status_done" => "Пол указан в настройках",
        "story.setup.sex_status_pending" => "Укажите пол в настройках",
        "story.setup.next_unlocked" => "Отлично! Доступна следующая секция — «Бухгалтерия похудения».",
        "story.setup.open_settings" => "Открыть настройки",

        // История — глава 1, бухгалтерия
        "story.acc.p1" => "Если дела идут плохо — например, в бизнесе или когда накапливается лишний вес, — люди очень часто прибегают к учёту. Просто чтобы понять, где именно дела идут не так.",
        "story.acc.p2" => "Богачи часто не считают деньги. А худые люди не считают калории.",
        "story.acc.p3" => "Но чтобы понять, где наши привычки дают сбой и почему у нас бывают проблемы, надо немного посчитать калории.",
        "story.acc.p4" => "Для этого есть много методов — например, люди ведут дневники питания, используют метод тарелки. В этом приложении мы постарались максимально облегчить вам задачу дневника питания. Оно содержит базовый набор функций, необходимый для успешного ведения дневника, и не содержит ничего лишнего, что только мешает.",
        "story.acc.p5" => "Здесь мы будем считать не только калории, но и учитывать две вещи:",
        "story.acc.li_weight" => "как именно меняется ваш вес;",
        "story.acc.li_calories" => "как вы тратите ваши калории.",
        "story.acc.p6" => "Призываем вас смотреть на этот подсчёт как на временную меру — своего рода курс лечения, который очень сильно поможет улучшить здоровье и устранит риски раннего угасания.",
        "story.acc.p7" => "Вести учёт можно по-разному. Например, можно очень подробно и без ошибок заносить все данные — и мы призываем стараться на этом пути. Однако не надо страдать или ругать себя, если что-то не получается. Долгие привычки меняются нелегко. Поэтому отнеситесь к себе с любовью и пониманием, пока идёте по этому пути.",
        "story.acc.task_label" => "Задания секции",
        "story.acc.task1" => "Зайдите в настройки и включите напоминание взвеситься.",
        "story.acc.task2" => "После того как придёт уведомление — сделайте свой первый замер.",
        "story.acc.task3" => "Неделю подряд взвешивайтесь каждый день, стараясь оставлять меньше пустых галочек, чем вчера.",
        "story.acc.streak_label" => "Дней подряд",
        "story.acc.next_unlocked" => "Открыта секция «Мои первые записи еды».",
        "story.acc.chapter_unlocked" => "Отлично! Открыта Глава 2.",
        "story.acc.push_first_weigh" => "\u{1f389} Первый замер сделан! Так держать!",
        "story.acc.howto_title" => "Как записать вес",
        "story.acc.howto" => "Откройте «Дневник», вкладка «Сегодня». Вверху слева есть виджет «Вес» — нажмите на него. Откроется окно с графиком и таблицей замеров. Нажмите «Взвеситься», введите вес и сохраните. Записать вес можно один раз в день.",

        // История — глава 1, первые записи еды
        "story.ff.p1" => "Еда — самая главная причина того, что мы набираем вес. Коррекция нашего питания всегда даёт отличный результат: она позволяет и держать свой вес в норме, и при этом не рисковать своим здоровьем.",
        "story.ff.p2" => "Поэтому мы должны знать, что мы съели: достаточно ли, не слишком ли много.",
        "story.ff.p3" => "И тут без подсчёта калорий на ранних этапах не обойтись. Сначала нужно понять, какие ошибки в своём питании вы допускаете, а уже потом их корректировать.",
        "story.ff.ways_intro" => "Чтобы записывать еду, в этом приложении есть три пути:",
        "story.ff.way1" => "Найти в интернете — просто введите название, и программа сама заполнит все нужные нутриенты.",
        "story.ff.way2" => "Сфотографировать этикетку — программа заполнит КБЖУ за вас (если лень).",
        "story.ff.way3" => "Ввести вручную — руками, если лень ждать или нет интернета.",
        "story.ff.howto_open" => "Откройте «Дневник» и нажмите большую зелёную кнопку:",
        "story.ff.step_new" => "Затем нажмите «Новая еда».",
        "story.ff.step_name" => "Введите название продукта — например, яблоко, рис или сникерс — и нажмите «Заполнить питательную ценность».",
        "story.ff.step_add" => "После этого нажмите «Добавить» и укажите вес.",
        "story.ff.step_more" => "Потом можно сразу найти что-то ещё.",
        "story.ff.task_label" => "Задание секции",
        "story.ff.task" => "Попробуйте ввести какую-то еду самостоятельно.",
        "story.ff.next_unlocked" => "Открыта секция «Активность и вес».",
        "story.ff.open_diary" => "Открыть дневник",

        // История — глава 1, активность и вес
        "story.act.p1" => "Второй важнейший фактор красивого и здорового тела — это уровень активности.",
        "story.act.p2" => "Причём именно бытовой активности — то есть, буквально, количество ваших шагов.",
        "story.act.p3" => "Людям, которым надо худеть, очень часто прописывают большие уровни активности — и не зря.",
        "story.act.p4" => "Большое количество низкоинтенсивной активности — ходьба, прогулки, танцы — помогает сжигать калории. Буквально: чем больше ходишь, тем больше сжигаешь.",
        "story.act.p5" => "У ходьбы есть ещё одна положительная сторона — она держит все мышцы в тонусе. Есть огромное количество исследований, подтверждающих, что люди, которые ходят, дольше сохраняют хорошее здоровье.",
        "story.act.p6" => "Поэтому мы с вами обязательно будем записывать количество шагов.",
        "story.act.p7" => "Нам надо взять и посмотреть, сколько вы едите, сопоставить это с уровнем вашей активности и с динамикой вашего веса. И уже из всего этого принимать решения, как именно действовать.",
        "story.act.p8" => "Регулярный учёт шагов, правильное взвешивание и понимание, что вы едите, — вот три кита бухгалтерии похудения, благодаря которым вы поймёте всё о собственном теле.",
        "story.act.task_label" => "Задания секции",
        "story.act.task1" => "Поставьте напоминание про шаги.",
        "story.act.task2" => "Запишите свои шаги хотя бы один раз.",
        "story.act.task3" => "Записывайте шаги неделю подряд, без пропусков.",
        "story.act.streak_label" => "Дней подряд",
        "story.act.next_unlocked" => "Открыта секция «Я готовлю».",
        "story.act.record_steps" => "Записать шаги",
        "story.act.howto_title" => "Как записать шаги",
        "story.act.howto" => "В «Дневнике» на вкладке «Сегодня» вверху справа есть виджет «Шаги» — нажмите на него. В открывшемся окне нажмите «Записать шаги», выберите «сегодня» или «вчера», введите количество и сохраните. Записать шаги можно один раз в день.",

        // История — глава 1, я готовлю
        "story.cook.p1" => "Многие люди испытывают сложности с записью готовой еды. Практически ни одно приложение не даёт возможности делать всё правильно, точно и удобно.",
        "story.cook.p2" => "Мы верим, что правильный выбор продуктов и минимальное время на готовку и учёт — это основные факторы бухгалтерии похудения.",
        "story.cook.p3" => "Любая готовка делается в два этапа:",
        "story.cook.step1" => "Сначала создаём рецепт и даём ему название — например, «Жареная картошка». Добавляем ингредиенты: массу сырой картошки, вес масла, лука (если добавляете).",
        "story.cook.step2" => "Когда блюдо готово — взвешиваем массу готового продукта (всё, что было на сковородке или в кастрюле). Нажимаем «Завершить» и вносим массу готового блюда. После этого блюдо можно добавить в дневник — его калорийность рассчитается автоматически. Просто найдите его в списке добавления.",
        "story.cook.task_label" => "Задания секции",
        "story.cook.task1" => "Приготовьте блюдо и внесите его в программу.",
        "story.cook.task2" => "Добавьте приготовленное блюдо в ваш дневник.",
        "story.cook.next_unlocked" => "Открыта секция «Моя еда с костями».",
        "story.cook.open_recipes" => "Открыть рецепты",

        // История — глава 1, моя еда с костями
        "story.bones.p1" => "Иногда еда, которая попадает нам в тарелку, содержит косточки.",
        "story.bones.p2" => "Часто можно их просто игнорировать, но некоторые люди любят побольше контроля в своей жизни. Специально для них мы создали возможность быстро ввести отходы в съеденную еду.",
        "story.bones.p3" => "Для этого надо кликнуть по массе добавленного продукта и отметить галочку «Не съел целиком». И ввести примерное или точно измеренное значение.",
        "story.bones.p4" => "Попробуйте эту функцию и пользуйтесь ей, если она вам нравится.",
        "story.bones.task_label" => "Задание секции",
        "story.bones.task1" => "Взвесьте себе немного еды с косточками — например черешни — и введите это значение в поле.",
        "story.bones.next_unlocked" => "Открыта секция «Праздник или ресторан».",
        "story.bones.open_diary" => "Открыть дневник",

        // История — глава 1, праздник или ресторан
        "story.rest.p1" => "Каждый из нас рано или поздно оказывается в ситуации, когда еда очень вкусная и калорийная. Например, на праздники. Или например, когда вы идёте в ресторан.",
        "story.rest.p2" => "К сожалению, любая еда, приготовленная вне дома, имеет ужасную погрешность, и её очень-очень сложно правильно учесть. Но мы руководствуемся следующими принципами:",
        "story.rest.method1" => "При добавлении еды мы можем вписать её калорийность — например, попросив КБЖУ-карту. В некоторых ресторанах вам её предоставят.",
        "story.rest.method2" => "Если карты нет — можно найти по названию блюда в интернете (как вы уже это делали ранее). Останется только вписать вес.",
        "story.rest.p3" => "Оба этих метода дают неточные данные, однако даже эти плохие данные лучше, чем ничего не вписать.",
        "story.rest.p4" => "Самое важное — при внесении еды в дневник необходимо указать, что это ресторанная еда (то же самое можно делать и с едой, которую приготовили ваши родственники).",
        "story.rest.p5" => "Программа автоматически добавит небольшой запас калорий. Потому что это нормальная практика в любом ресторане — немного добавить калорийного масла в любое блюдо.",
        "story.rest.p6" => "Мы против жёстких ограничений и принципа «Никогда не ешь в ресторане». Наша задача — это всего лишь понимать, какие последствия это для нас несёт и как нам с этим жить, если мы хотим быть здоровы. Будьте уверены, в конце вашего пути к вам вернётся спонтанность и возможность есть любую еду.",
        "story.rest.p7" => "Программа будет вам по ходу вашего движения выдавать рекомендации. Просто старайтесь их применять, и у вас всё получится.",
        "story.rest.task_label" => "Задание секции",
        "story.rest.task1" => "Съешьте ресторанную еду, которую вы любите, — картошку фри, бургер. Сходите куда-нибудь или закажите онлайн. Впишите ресторанную еду.",
        "story.rest.next_unlocked" => "Поздравляем — вы выполнили все задания первой главы!",
        "story.rest.open_diary" => "Открыть дневник",

        // История — глава 1, что дальше (paywall)
        "story.next.p1" => "Вы дошли до конца первой главы. Путь продолжается — но чтобы идти дальше, нужны две вещи.",
        "story.next.p2" => "Во-первых, выполните оставшиеся задания в секциях этой главы. Во-вторых, поддержите проект — чтобы мы могли продолжать (и держать AI включённым).",
        "story.next.p3" => "AI-функции бесплатны первые 14 дней. Дальше активная подписка сохраняет распознавание еды и ваш прогресс по Истории.",
        "paywall.loading" => "Загрузка…",
        "paywall.status_trial" => "Пробный период",
        "paywall.status_paid" => "Подписка активна",
        "paywall.status_expired" => "Подписка истекла",
        "paywall.days_left" => "дн. осталось",
        "paywall.code_placeholder" => "Кодовое слово",
        "paywall.pay_button" => "Оплатить",
        "paywall.paying" => "Обработка…",
        "paywall.invalid_code" => "Неверное кодовое слово.",
        "paywall.success" => "Спасибо! Подписка активна.",
        "paywall.back_to_story" => "Назад к Истории",

        _ => "???",
    }
}
