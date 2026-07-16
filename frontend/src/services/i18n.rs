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

/// A "YYYY-MM-DD" date as words relative to today: Сегодня / Вчера / Позавчера,
/// then the weekday name (3–7 days ago), then the full date (older / any future).
pub fn relative_date(date_str: &str) -> String {
    use chrono::Datelike;
    let today = crate::services::local::today_date();
    let date = match chrono::NaiveDate::parse_from_str(date_str, "%Y-%m-%d") {
        Ok(d) => d,
        Err(_) => return date_str.to_string(),
    };
    match (today - date).num_days() {
        d if d < 0 => date_str.to_string(),
        0 => t("diary.today").to_string(),
        1 => t("diary.yesterday").to_string(),
        2 => t("diary.day_before").to_string(),
        3..=7 => match date.weekday() {
            chrono::Weekday::Mon => t("diary.weekday.mon"),
            chrono::Weekday::Tue => t("diary.weekday.tue"),
            chrono::Weekday::Wed => t("diary.weekday.wed"),
            chrono::Weekday::Thu => t("diary.weekday.thu"),
            chrono::Weekday::Fri => t("diary.weekday.fri"),
            chrono::Weekday::Sat => t("diary.weekday.sat"),
            chrono::Weekday::Sun => t("diary.weekday.sun"),
        }
        .to_string(),
        _ => {
            let month = match date.month() {
                1 => t("diary.month.1"), 2 => t("diary.month.2"), 3 => t("diary.month.3"),
                4 => t("diary.month.4"), 5 => t("diary.month.5"), 6 => t("diary.month.6"),
                7 => t("diary.month.7"), 8 => t("diary.month.8"), 9 => t("diary.month.9"),
                10 => t("diary.month.10"), 11 => t("diary.month.11"), 12 => t("diary.month.12"),
                _ => "",
            };
            format!("{} {} {}", date.day(), month, date.year())
        }
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
        "nav.dashboard" => "Home",
        "dashboard.persona_setup_title" => "Set up your profile",
        "dashboard.persona_setup_hint" => "Height, age, sex and goal",
        "dashboard.persona_title" => "Profile",
        "dashboard.notifications_title" => "Notifications",
        "errors.title" => "Errors",
        "errors.hint" => "Something went wrong in the background. Tap an item to copy it.",
        "errors.none" => "No errors.",
        "errors.copied" => "Copied ✓",
        "errors.clear" => "Clear",
        "dashboard.close" => "Done",
        "dashboard.sex" => "Sex",
        "dashboard.sex_male" => "Male",
        "dashboard.sex_female" => "Female",
        "dashboard.height" => "Height, cm",
        "dashboard.birth_year" => "Year of birth",
        "dashboard.goal" => "Goal",
        "dashboard.goal_lose" => "Lose",
        "dashboard.goal_gain" => "Gain",
        "dashboard.goal_maintain" => "Maintain",
        "dashboard.progress.word_lose" => "weight loss",
        "dashboard.progress.word_gain" => "muscle gain",
        "dashboard.progress.word_maintain" => "weight maintenance",
        "dashboard.progress.intro" => "Very soon your {word} process will begin. Our algorithm will calculate the amount of calories you should eat every day. For the calculation to be accurate, you need to log all your food in the app, weigh yourself every day, and record your steps.",
        "dashboard.progress.nutrition" => "Nutrition",
        "dashboard.progress.calculate" => "Calculate my target",
        "dashboard.progress.recalc_needed" => "Your goal changed — the target needs recalculating.",
        "dashboard.progress.recalc" => "Recalculate the target",
        "dashboard.progress.done_title" => "Your daily target",
        "dashboard.progress.gate_title" => "Keep these indicators green for a week.",
        "dashboard.progress.gate_progress" => "Left: {n}/7 days.",
        "dashboard.progress.kcal_day" => "kcal/day",
        "dashboard.progress.done_hint" => "We'll adjust it as observations come in.",
        "dashboard.progress.help_1" => "Our algorithm will calculate your calorie target for you.",
        "dashboard.progress.help_2" => "For it to start working, you need to log your food every day.",
        "dashboard.progress.help_3" => "Tap the question mark to see how to do it.",
        "help.back" => "Back",
        "help.food.title" => "How to log food",
        "help.food.intro" => "For the algorithm to calculate your calorie target, food has to be logged every day. Here's how.",
        "help.food.where_title" => "Where the diary and the «+» button are",
        "help.food.where_text" => "Open the «Diary» tab in the bottom menu. At the bottom right there's a green round «+» button — tap it to add food.",
        "help.food.no_base" => "There's no global food database. You enter foods yourself — by hand, with an AI request, or by photo recognition. This gradually builds your own personal database of the foods you eat.",
        "help.food.new_how_title" => "How to open the form",
        "help.food.new_how1" => "On the diary, tap «+» and start searching for the product by name:",
        "help.food.new_how2" => "If there's no matching product in your base, tap «New food» at the bottom of the list — the new-product form opens:",
        "help.food.methods_title" => "Ways to log food",
        "help.food.search_title" => "Search your base",
        "help.food.search_text" => "Start typing a name — the app finds the product in your personal base. Pick it and enter the weight.",
        "help.food.ai_title" => "AI request",
        "help.food.ai_text" => "On the «By name» tab, type the product's name or description and tap «Fill nutrition info» — the AI fills in the calories and macros for you. Just review and save.",
        "help.food.photo_title" => "Photo & recognition",
        "help.food.photo_text" => "On the «By photo» tab, add a photo of the food or its label and tap «Detect calories» — the AI recognises the product and fills in the calories and macros.",
        "help.food.more_title" => "More",
        "help.link.food_search" => "Search the database",
        "help.link.food_ai" => "AI request",
        "help.link.food_photo" => "Photo & recognition",
        "help.link.copy_day" => "How to copy food from a past day",
        "help.link.recipes" => "How to make cooked food — recipes",
        "help.link.delete_food" => "How to delete food from the diary",
        "help.link.edit_weight" => "How to change the weight of logged food",
        "help.link.rename_food" => "How to rename an awkward food name",
        "help.link.diary" => "How to keep the diary",
        "help.link.food_diary" => "Food diary",
        "help.link.weigh" => "Your daily weigh-ins",
        "help.link.steps" => "Step count",
        "help.shot.diary_fab" => "[screenshot: diary and the «+» button]",
        "help.shot.search" => "[screenshot: search the database]",
        "help.shot.ai" => "[screenshot: AI request]",
        "help.shot.photo" => "[screenshot: photo & recognition]",
        "help.article.stub" => "Detailed instructions coming soon.",
        "help.demo.search_query" => "buckwheat",
        "help.demo.food1_name" => "Buckwheat, cooked",
        "help.demo.food2_name" => "Buckwheat, dry",
        "help.demo.ai_query" => "A two-egg omelette and a toast",
        "help.demo.ai1_name" => "Two-egg omelette",
        "help.demo.ai2_name" => "Toast",
        "help.demo.ai_button" => "Parse",
        "help.demo.photo_button" => "Take a photo",
        "help.demo.photo_name" => "Sardines in tomato sauce",
        "help.demo.recipe1_name" => "Rolled oats",
        "help.demo.recipe2_name" => "Cottage cheese 5%",
        "help.article.copy_day.p1" => "Open the past day you need with the ‹ › arrows at the top of the diary.",
        "help.article.copy_day.p2" => "Each past-day entry has a repeat button (circular arrows) on the right. Tap it and choose «Repeat today» — the food is copied into today.",
        "help.article.recipes.p1" => "Open the «Recipes» tab and tap «+ New».",
        "help.article.recipes.p2" => "Add ingredients with «+ Add ingredient», each with its weight, then tap «Finalize» and enter the final weight of the cooked dish — the app computes the calories/protein/fat/carbs per 100 g.",
        "help.article.recipes.p3" => "The finished dish is then logged in the diary through search: start typing its name and pick it like any other food.",
        "help.article.delete_food.p1" => "Tap «⋮» on an entry in the diary and choose «Delete».",
        "help.article.edit_weight.p1" => "Tap the gram number (e.g. «150 g») on an entry in the diary.",
        "help.article.edit_weight.p2" => "In the window that opens, change the weight — the calories/protein/fat/carbs recompute automatically.",
        "help.article.rename_food.p1" => "Tap «⋮» on an entry, choose «Edit», then change the name.",
        "help.article.rename_food.p2" => "This is handy when the AI mislabelled the dish — that can happen with photo recognition.",
        "help.article.diary.intro" => "Every day you fill in three things:",
        "help.article.weigh.intro" => "Weigh yourself every day — that way the algorithm sees your weight TREND, not random daily jumps. For the reading to be comparable day to day, keep the same conditions:",
        "help.article.weigh.p1" => "Weigh in the morning, right after waking up.",
        "help.article.weigh.p2" => "Before eating or drinking anything.",
        "help.article.weigh.p3" => "After using the toilet.",
        "help.article.weigh.p4" => "Before a shower or washing.",
        "help.article.weigh.p5" => "Without clothes (or in the same light clothing each time).",
        "help.article.weigh.record" => "Record the weight on the home screen — the weight widget, the «+» button. One entry per day; you can edit it.",
        "help.article.weigh.how_title" => "How to open the form",
        "help.article.weigh.open1" => "On the home screen, tap the weight widget:",
        "help.article.weigh.open1b" => "If you've already logged some weights, the widget shows a chart instead — tap it the same way:",
        "help.article.weigh.open2" => "A window opens with the weight chart and history. Tap «Weigh in» at the bottom:",
        "help.article.weigh.open3" => "Enter the weight, tick the conditions you met, and tap «Save». One entry per day — you can edit it.",
        "help.article.weigh.fluct" => "Weight swings from day to day because of water, salt, and — for women — the menstrual cycle. That's normal: the algorithm accounts for these swings and looks at the trend, so just weigh in every day and don't worry about a single number.",
        "help.article.steps.intro" => "Log how many steps you walked each day. Steps are everyday activity that burns calories without any workout.",
        "help.article.steps.p1" => "Take the number from your phone's step counter or a health app (Apple Health, Google Fit, «Health»).",
        "help.article.steps.p2" => "Enter it on the home screen — the steps widget, the «+» button.",
        "help.article.steps.p3" => "Once a day: in the evening for today, or in the morning for yesterday.",
        "help.article.steps.p4" => "Aim for at least 7000 steps a day — that already brings a substantial health improvement.",
        "help.article.steps.how_title" => "How to open the form",
        "help.article.steps.open1" => "On the home screen, tap the steps widget:",
        "help.article.steps.open1b" => "If you've already logged some steps, the widget shows a chart instead — tap it the same way:",
        "help.article.steps.open2" => "A window opens with the steps chart. Tap «Record steps»:",
        "help.article.steps.open3" => "Choose the day (today / yesterday), enter the step count, and tap «Save».",
        "cycle.title" => "Cycle",
        "cycle.day_label" => "Day",
        "cycle.not_set" => "—",
        "cycle.first_day" => "First day of the cycle",
        "cycle.set_first_day" => "Set the first day of the cycle",
        "cycle.set_prompt" => "Set the first day of your cycle to track its phases.",
        "cycle.weight_heading" => "Weight",
        "cycle.training_heading" => "Training",
        "cycle.save" => "Save",
        "cycle.cancel" => "Cancel",
        "cycle.phase.menstrual.name" => "Menstrual phase",
        "cycle.phase.menstrual.desc" => "The start of the cycle: menstruation is under way and hormone levels are at their lowest.",
        "cycle.phase.menstrual.weight" => "At the start of your period the body holds water and may bloat, so the scale can read higher than usual — that isn't fat. Toward the end of the phase the water leaves and weight drops; the algorithm already accounts for these swings.",
        "cycle.phase.menstrual.training" => "Well-being is often lower — reduce intensity and rest more. Light activity, walking and stretching suit better than heavy loads.",
        "cycle.phase.follicular.name" => "Follicular phase",
        "cycle.phase.follicular.desc" => "The body prepares for ovulation: estrogen rises and energy builds up.",
        "cycle.phase.follicular.weight" => "In this phase water is barely retained, so the scale is usually calm, with no sharp jumps. If the weight wobbles a little, that's normal day-to-day variation — the algorithm smooths it out.",
        "cycle.phase.follicular.training" => "Energy and recovery are on the rise — a great time for strength and intense training. You can push the load and go for personal records.",
        "cycle.phase.ovulation.name" => "Ovulation",
        "cycle.phase.ovulation.desc" => "Mid-cycle: the egg is released, estrogen and energy peak.",
        "cycle.phase.ovulation.weight" => "At the hormone peak there may be slight water retention, so weight can tick up for a day or two. It's temporary and doesn't affect real progress — the algorithm accounts for it.",
        "cycle.phase.ovulation.training" => "Peak strength and endurance — an excellent day for heavy training. Ligaments are a bit more relaxed in this period, so give your warm-up more attention.",
        "cycle.phase.luteal.name" => "Luteal phase",
        "cycle.phase.luteal.desc" => "The second half of the cycle: progesterone rises and the body tends to retain water.",
        "cycle.phase.luteal.weight" => "In the second half of the cycle the body holds more water — the scale can jump by 0.5–2 kg for no reason. That's water, not fat: it leaves once your period starts, and the algorithm already factors these swings in.",
        "cycle.phase.luteal.training" => "Energy drops and recovery slows — cut the volume and add rest. Cravings are likely: keep the focus on protein and your calorie target.",
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
        "chat.context" => "Context (tool calls)",
        "chat.mic_denied" => "Microphone access denied",
        "chat.mode_ai" => "AI",
        "chat.mode_live" => "Live person",
        "chat.live_empty" => "No messages yet. Write to a live support agent — they'll reply here.",
        "chat.live_sending" => "sending…",
        "chat.live_retry" => "not sent, tap to retry",

        // Curator data-request panel + share
        "curator.request_title" => "Curator's request",
        "curator.request_body" => "The curator is asking you for your body parameters",
        "curator.request_food" => "The curator is asking you for your food diary",
        "curator.request_weight" => "The curator is asking you for your weight diary",
        "curator.request_steps" => "The curator is asking you for your steps diary",
        "curator.request_all" => "The curator is asking you for all of your data",
        "curator.share" => "Share",
        "curator.sharing" => "Sharing…",
        "curator.shared_done" => "Data sent",
        "curator.shared_body" => "Data sent: body parameters",
        "curator.shared_food" => "Data sent: food diary",
        "curator.shared_weight" => "Data sent: weight diary",
        "curator.shared_steps" => "Data sent: steps diary",
        "curator.shared_all" => "Data sent: all your data",

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

        // Connectivity warning (dashboard triangle)
        "net.offline_title" => "Can't reach the server",
        "net.offline_body_vpn" => "Your data is saved on the device. Try toggling your VPN on or off.",
        "net.degraded_title" => "Some services are unavailable",
        "net.degraded_body" => "Data is saved locally; temporarily unavailable:",
        "net.worker.ai" => "AI",
        "net.worker.sync" => "sync",
        "net.worker.auth" => "sign-in",
        "net.worker.payment" => "subscription",
        "net.worker.ocr" => "label scan",
        "net.worker.bug" => "bug reports",
        "net.worker.support" => "support chat",
        "net.worker.push" => "notifications",
        "offline_gate.title" => "No connection",
        "offline_gate.body" => "We can't reach the server to finish setting up. This is a network problem — check your internet or VPN and try again.",
        "offline_gate.retry" => "Retry",
        "dashboard.calories_title" => "Calories",
        "chart.average" => "avg",
        "chart.no_data" => "No data yet",
        "chart.hint" => "Touch the chart to see a day",

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
        "recipes.change_weight" => "Change final weight",
        "recipes.complete" => "Complete",
        "recipes.in_progress" => "In Progress",

        // Recipe detail
        "recipe.loading" => "Loading...",
        "recipe.back" => "\u{2190} Recipes",
        "recipe.name_placeholder" => "Dish name",
        "recipe.name_required" => "Enter the dish name",
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
        "settings.version_check" => "Check for update",
        "settings.version_checking" => "Checking…",
        "settings.dev" => "Development",
        "settings.dev_refresh" => "Refresh",
        "settings.dev_copy" => "Copy",
        "settings.dev_clear" => "Clear",
        "settings.dev_empty" => "No diagnostics yet. Trigger a test notification, tap it, then Refresh.",
        "settings.subscription" => "Subscription",
        "settings.sub_active" => "Subscription active",
        "settings.sub_trial" => "Trial period",
        "settings.sub_expired" => "Subscription expired",
        "settings.sub_cancelled" => "Cancelled — active until the period ends",
        "settings.sub_cancel" => "Cancel subscription",
        "settings.sub_cancel_confirm" => "Cancel auto-renew? You keep access until the current period ends.",
        "settings.sub_cancel_msg" => "Cancel subscription? You'll keep access for another {n}.",
        "settings.sub_refund" => "Request a refund",
        "settings.sub_refund_title" => "Request a refund?",
        "settings.sub_refund_warn" => "Requesting a refund cuts off app access immediately.",
        "settings.sub_refund_amount" => "Refund amount",
        "settings.sub_refund_processing" => "Processing the request takes about a week, plus your bank's time to return the payment.",
        "settings.sub_refund_confirm" => "Request refund",
        "settings.sub_refund_error" => "Couldn't request the refund. Please try again.",
        "settings.sub_cancel_note" => "You can cancel anytime — here, via the link in lava's emails, or by writing to info@renorma.app. No app login required.",
        "settings.sub_buy_on_site" => "Your subscription isn't active. You can purchase one on the website.",
        "settings.sub_open_site" => "Open the website",
        "settings.sub_renew_after" => "You can renew in {n} — once your current access expires.",
        "settings.sub_buy_in_tg" => "Subscriptions are handled in Telegram.",
        "settings.sub_open_tg" => "Open in Telegram",
        "settings.sub_manage" => "Manage subscription",
        "settings.sub_since" => "Subscribed since",
        "settings.sub_until" => "Active until",
        "settings.sub_access_left" => "Access left",
        "settings.sub_cost" => "Price",
        "settings.account" => "Account",
        "settings.backup" => "Backup access",
        "backup.title" => "Backup access",
        "backup.back" => "Settings",
        "backup.desc" => "A backup phrase lets you sign in on a new device without a passkey. Keep it private — anyone with it can access your account.",
        "backup.generate" => "Create a backup phrase",
        "backup.regenerate" => "Generate a new phrase",
        "backup.generating" => "Generating…",
        "backup.your_phrase" => "Your phrase",
        "backup.warning" => "Save this phrase somewhere safe. Generating a new one replaces the old.",
        "backup.retry_failed" => "Couldn't create a phrase — try again",
        "settings.logout" => "Log out",
        "settings.logout_confirm" => "Log out? Your data is synced and stays on this device — signing back in restores it.",
        "settings.danger_zone" => "Danger zone",
        "settings.danger_delete_diary" => "Delete diary data",
        "settings.danger_delete_old" => "Delete data older than 1 year",
        "settings.danger_confirm_old" => "Delete diary entries older than 1 year? This cannot be undone.",
        "settings.danger_delete_all" => "Delete all data",
        "settings.danger_confirm_all" => "Delete ALL diary entries? This cannot be undone.",
        "settings.nutrient_placeholder" => "Omega 3, Fiber...",

        // Food editor
        "food_editor.product_name" => "Name or description of the dish",
        "food_editor.name_field" => "Name",
        "food_editor.name_field_ph" => "Product name",
        "food_editor.recommended_abbr" => "rec",
        "ai.extracted_from_label" => "Extracted from label",
        "food_editor.add_photo" => "Add label photo",
        "food_editor.add_more_photo" => "Add another photo",
        "food_editor.add_photo_short" => "Photo",
        "food_editor.detect_food" => "Detect food",
        "food_editor.photo_hint" => "Shoot the nutrition-facts table up close so it fills the frame — small/distant text is read poorly.",
        "food_editor.ai_uploading" => "Uploading photo\u{2026}",
        "food_editor.ai_queue" => "In queue:",
        "food_editor.ai_recognizing" => "Recognizing\u{2026}",
        "food_editor.ai_timeout" => "Recognition is taking too long — try again later.",
        "food_editor.filling" => "Filling...",
        "food_editor.fill_info" => "Fill nutrition info",
        "food_editor.tab_by_name" => "By description",
        "food_editor.tab_by_photo" => "By label",
        "food_editor.tab_by_food_photo" => "By food photo",
        "food_editor.food_photo_soon" => "Recognising a ready meal from a photo — enumerates the foods and their weights. Coming soon.",
        "food_editor.food_photo_hint" => "Shoot the whole plate from above with a scale reference (fork, hand). Weights are estimates — edit them per item.",
        "food_editor.detected_title" => "Detected in the photo",
        "food_editor.auto_tag" => "auto",
        "food_editor.suggested_tag" => "check",
        "food_editor.no_food_detected" => "No food recognised in the photo — try a clearer shot.",
        "food_editor.total" => "Total",
        "food_editor.add_all" => "Add all products",
        "food_editor.detect_by_name" => "Fill nutrition info",
        "food_editor.detect_short" => "Fill",
        "food_editor.detect_by_photo" => "Detect calories",
        "food_editor.calories" => "Calories",
        "food_editor.protein" => "Protein",
        "food_editor.fat" => "Fat",
        "food_editor.carbs" => "Carbs",
        "food_editor.add" => "Add",
        "food_editor.paywall_title" => "Subscription inactive",
        "food_editor.paywall_body" => "Automatic calorie & macro detection needs an active subscription.",
        "food_editor.paywall_pay" => "Subscribe",
        "food_editor.paywall_dismiss" => "Not now",

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
        "common.unit.steps" => "steps",

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
        "settings.height" => "Height",
        "settings.height_label" => "Height, cm",
        "settings.height_why" => "Why we ask: together with your weight, height gives your BMI — a coarse read on how much of your body mass is fat.",
        "settings.bmi" => "Your BMI: {n}",
        "settings.goal" => "Course goal",
        "settings.goal_lose" => "Lose weight",
        "settings.goal_maintain" => "Maintain weight",
        "settings.goal_why" => "What the whole discipline is aimed at. On maintenance we never suggest lowering your calorie planka.",
        "settings.birth_year" => "Birth year",
        "settings.birth_year_label" => "Year of birth",
        "settings.birth_year_why" => "Why we ask: your age is needed to estimate how many calories your body burns, so we can compute a sound recommendation.",

        // Weekly recommendation card

        // Onboard (paid-landing claim flow: register → bind the paid subscription)
        "onboard.title" => "Create your account",
        "onboard.subtitle" => "Your payment went through. Create an account and we'll link your subscription to it.",
        "onboard.claiming" => "Linking your subscription…",
        "onboard.pending_title" => "Confirming your payment…",
        "onboard.pending_body" => "This can take a moment. We'll keep checking automatically.",
        "onboard.retry" => "Retry",
        "onboard.error_title" => "Couldn't link the subscription",
        "onboard.error_body" => "This payment may already be linked to another account. Contact info@renorma.app if you think this is a mistake.",
        "onboard.link_unavailable" => "This link is no longer valid or has already been used. Please subscribe again.",
        "onboard.have_account" => "Already have an account? Sign in",
        "onboard.success" => "All set! Opening the app…",

        // Auth
        "auth.main_description" => "This app works locally on your device and does not store data on remote servers. However, some features — such as syncing between devices or AI — require signing in.",
        "auth.create_account" => "Sign up",
        "auth.already_used" => "I already use this app:",
        "auth.creating" => "Creating...",
        "auth.authenticating" => "Signing in...",
        "locked.title" => "Subscription required",
        "locked.body" => "This account doesn't have an active subscription. A subscription is purchased on the website. If you have another account, sign in below.",
        "auth.login_title" => "Sign in",
        "auth.login_have_device" => "If you have another signed-in device:",
        "auth.login_option1_hint" => "On the other device: Settings → Connect device → Scan QR code. Then press here:",
        "auth.login_option2_hint" => "On the other device: Settings → Connect device → Show QR code. Then press here:",
        "auth.login_no_device" => "If you don't have a signed-in device:",
        "auth.try_passkey" => "Try signing in with PassKey",
        "auth.tagline" => "Weight, nutrition & lifestyle, normalized.",
        "auth.sign_in" => "Sign in",
        "auth.register" => "Sign up",
        "auth.phrase_login" => "Sign in with a phrase",
        "auth.phrase_title" => "Sign in with your phrase",
        "auth.phrase_hint" => "Enter your backup phrase to sign in on this device.",
        "auth.phrase_placeholder" => "your five words",
        "auth.phrase_back" => "Back",
        "auth.phrase_invalid" => "That phrase doesn't match any account.",
        "auth.phrase_rate_limited" => "Too many attempts. Try again later.",
        "auth.add_device" => "Add a device",
        "auth.add_device_hint" => "On a device where you're already signed in: Settings → Connect device → Scan QR, then point it at this code.",
        "auth.scan_instead" => "Scan a QR instead",
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
        "privacy.add_passkey" => "Add a passkey on this device",
        "privacy.add_passkey_busy" => "Adding…",
        "privacy.add_passkey_done" => "Passkey added ✓",

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
        "settings.notif_disable" => "Turn off notifications",
        "settings.notif_enabled" => "Notifications enabled",
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
        "weight.empty_prompt" => "Tap here to log your weight",
        "weight.widget_placeholder" => "Your weight chart will appear here. Not enough data to draw it yet — once you have at least three measurements, the chart will be shown.",
        "weight.trend.title" => "Trend · 14 days",
        "weight.trend.down" => "Losing",
        "weight.trend.up" => "Gaining",
        "weight.trend.stable" => "Weight is holding steady",
        "weight.trend.insufficient" => "Not enough data for a trend",
        "weight.trend.preliminary" => "preliminary",
        "weight.trend.week" => "week",
        "weight.trend.confidence" => "confidence",
        "weight.trend.weak_down" => "Likely losing",
        "weight.trend.weak_up" => "Likely gaining",
        "weight.trend.low_confidence" => "low confidence",
        "weight.cycle.label" => "Period",
        "weight.cycle.none" => "no cycle detected",
        "weight.cycle.insufficient" => "not enough data yet",
        "weight.cycle.day_short" => "d",
        "weight.cycle.decycled" => "Weight without the cycle",

        "steps.title" => "Steps",
        "steps.empty_prompt" => "Tap here to log your steps",
        "steps.for_today" => "Recording evening steps for TODAY",
        "steps.for_yesterday" => "Recording morning steps for YESTERDAY",
        "steps.input_placeholder" => "Steps",
        "steps.unit" => "steps",
        "steps.save" => "Save",
        "steps.add" => "Record steps",
        "steps.edit" => "Edit today's steps",
        "steps.once_per_day" => "One entry per day — you can edit it",
        "steps.col_steps" => "Steps",









        // Chapter 3, section 1: Finding the deficit (prose before the planka widget)
        // Chapter 3, section 2: Why the weight isn't coming off
        // Chapter 3, section 3: The calorie
        // Chapter 3, section 4: A friend eats a lot but stays slim
        // Chapter 3, section 5: Sleep
        // Chapter 3, section 6: Walk more
        // Chapter 3, section 7: Swap awful habits for bad ones







        "progress.title" => "Progress photos",
        "progress.subtitle" => "Front, side and back. Stored on your device only.",
        "progress.capture" => "Take photo",
        "progress.tips_title" => "Recommendations",
        "progress.tip_bg" => "Try to shoot against a plain background.",
        "progress.tip_height" => "Place the camera at chest level.",
        "progress.history" => "History",
        "progress.empty" => "No photos yet.",
        "progress.pose_front" => "Front",
        "progress.pose_side" => "Side",
        "progress.pose_back" => "Back",











        "paywall.loading" => "Loading…",
        "paywall.contacting_payment" => "Contacting the payment system…",
        "paywall.status_trial" => "Trial",
        "paywall.status_paid" => "Subscription active",
        "paywall.status_expired" => "Subscription expired",
        "paywall.days_left" => "days left",
        "paywall.choose_plan" => "Choose a plan",
        "paywall.pay_button" => "Subscribe",
        "paywall.paying" => "Redirecting…",
        "paywall.per_month" => "/ month",
        "paywall.per_year" => "/ year",
        "paywall.checkout_error" => "Couldn't start checkout. Please try again.",
        "paywall.not_configured" => "Payments aren't available yet — check back soon.",
        "paywall.success" => "Thank you! Your subscription is active.",
        "paywall.back_to_story" => "Back to the Story",
        "paywall.welcome_title" => "You're subscribed 🎉",
        "paywall.welcome_body" => "Payment went through. You can manage your subscription anytime in Settings → Subscription — see when it renews, the price, and cancel.",
        "paywall.welcome_manage" => "Open Settings → Subscription",
        "paywall.onb_title" => "Full access to re:Norma",
        "paywall.later" => "Later",
        "paywall.then" => "then",
        "paywall.trial_left" => "{n} trial days left",
        "paywall.trial_expired" => "Your trial period has ended",
        "paywall.price_line" => "Subscribe for {price} per month",
        "paywall.rule1" => "Try the app for 7 days. After that a subscription is required.",
        "paywall.rule2" => "All features are available during the 7 days.",
        "paywall.rule3" => "You can cancel the subscription at any time.",
        "paywall.subscribe" => "Subscribe",
        "paywall.skip" => "Skip",
        "paywall.promo_placeholder" => "Promo code (optional)",

        _ => "???",
    }
}

fn ru(key: &str) -> &'static str {
    match key {
        // Навигация
        "nav.dashboard" => "Главная",
        "dashboard.persona_setup_title" => "Настройте персону",
        "dashboard.persona_setup_hint" => "Рост, возраст, пол и цель",
        "dashboard.persona_title" => "Персона",
        "dashboard.notifications_title" => "Уведомления",
        "errors.title" => "Ошибки",
        "errors.hint" => "В фоне что-то пошло не так. Нажмите на пункт, чтобы скопировать.",
        "errors.none" => "Ошибок нет.",
        "errors.copied" => "Скопировано ✓",
        "errors.clear" => "Очистить",
        "dashboard.close" => "Готово",
        "dashboard.sex" => "Пол",
        "dashboard.sex_male" => "Мужской",
        "dashboard.sex_female" => "Женский",
        "dashboard.height" => "Рост, см",
        "dashboard.birth_year" => "Год рождения",
        "dashboard.goal" => "Цель",
        "dashboard.goal_lose" => "Похудеть",
        "dashboard.goal_gain" => "Набрать",
        "dashboard.goal_maintain" => "Сохранить",
        "dashboard.progress.word_lose" => "похудения",
        "dashboard.progress.word_gain" => "массонабора",
        "dashboard.progress.word_maintain" => "поддержания веса",
        "dashboard.progress.intro" => "Очень скоро начнётся процесс вашего {word}. Наш алгоритм сам рассчитает вам необходимое количество калорий, которые вы должны будете употреблять ежедневно. Для того чтобы расчёт был точным, вам надо вносить всю еду в программу, каждый день взвешиваться и записывать ваши шаги.",
        "dashboard.progress.nutrition" => "Питание",
        "dashboard.progress.calculate" => "Рассчитать мою планку",
        "dashboard.progress.recalc_needed" => "Цель изменилась — планка должна быть пересчитана.",
        "dashboard.progress.recalc" => "Пересчитать планку",
        "dashboard.progress.done_title" => "Ваша дневная планка",
        "dashboard.progress.gate_title" => "Держите эти индикаторы зелёными в течение недели.",
        "dashboard.progress.gate_progress" => "Ещё: {n}/7 дней.",
        "dashboard.progress.kcal_day" => "ккал/день",
        "dashboard.progress.done_hint" => "Мы будем корректировать её по мере наблюдений.",
        "dashboard.progress.help_1" => "Наш алгоритм поможет рассчитать вам вашу планку по калориям.",
        "dashboard.progress.help_2" => "Для того чтобы он начал работать, необходимо ежедневно вносить еду.",
        "dashboard.progress.help_3" => "Нажмите на вопросик, чтобы понять, как это сделать.",
        "help.back" => "Назад",
        "help.food.title" => "Как вносить еду",
        "help.food.intro" => "Чтобы алгоритм рассчитал вашу планку по калориям, еду нужно вносить каждый день. Ниже — как это сделать.",
        "help.food.where_title" => "Где дневник и кнопка «+»",
        "help.food.where_text" => "Откройте вкладку «Дневник» в нижнем меню. Внизу справа есть зелёная круглая кнопка «+» — нажмите её, чтобы добавить еду.",
        "help.food.no_base" => "Глобальной базы продуктов нет. Продукты вы вносите сами — вручную по описанию, с помощью ИИ или распознавания по фото. Так постепенно собирается ваша личная база продуктов, которые вы едите.",
        "help.food.new_how_title" => "Как открыть форму",
        "help.food.new_how1" => "На дневнике нажмите «+» и начните искать продукт по названию:",
        "help.food.new_how2" => "Если подходящего продукта в вашей базе нет — внизу списка нажмите «Новая еда». Откроется форма нового продукта:",
        "help.food.methods_title" => "Способы внести еду",
        "help.food.search_title" => "Поиск по своей базе",
        "help.food.search_text" => "Начните вводить название — программа найдёт продукт в вашей личной базе. Выберите его и укажите вес.",
        "help.food.ai_title" => "ИИ-запрос",
        "help.food.ai_text" => "На вкладке «По названию» введите название или описание продукта и нажмите «Заполнить пищевую ценность» — ИИ сам заполнит калории и БЖУ. Останется проверить и сохранить.",
        "help.food.photo_title" => "Фото и распознавание",
        "help.food.photo_text" => "На вкладке «По фото» добавьте фото еды или этикетки и нажмите «Определить калорийность» — ИИ распознает продукт и заполнит калории и БЖУ.",
        "help.food.more_title" => "Ещё",
        "help.link.food_search" => "Поиск по базе",
        "help.link.food_ai" => "ИИ-запрос",
        "help.link.food_photo" => "Фото и распознавание",
        "help.link.copy_day" => "Как скопировать еду из прошлого дня?",
        "help.link.recipes" => "Как сделать приготовленную еду — рецепты",
        "help.link.delete_food" => "Как удалить еду из дневника",
        "help.link.edit_weight" => "Как изменить вес введённой еды?",
        "help.link.rename_food" => "Как изменить неудобное название введённой еды",
        "help.link.diary" => "Как вести дневник",
        "help.link.food_diary" => "Дневник питания",
        "help.link.weigh" => "Ваши ежедневные взвешивания",
        "help.link.steps" => "Количество шагов",
        "help.shot.diary_fab" => "скриншот: дневник и кнопка «+»",
        "help.shot.search" => "скриншот: поиск по базе",
        "help.shot.ai" => "скриншот: ИИ-запрос",
        "help.shot.photo" => "скриншот: фото и распознавание",
        "help.article.stub" => "Подробное описание скоро добавим.",
        "help.demo.search_query" => "гречка",
        "help.demo.food1_name" => "Гречка варёная",
        "help.demo.food2_name" => "Гречка, сухая",
        "help.demo.ai_query" => "Омлет из двух яиц и тост",
        "help.demo.ai1_name" => "Омлет из 2 яиц",
        "help.demo.ai2_name" => "Тост",
        "help.demo.ai_button" => "Разобрать",
        "help.demo.photo_button" => "Сфотографировать",
        "help.demo.photo_name" => "Сардины в томатном соусе",
        "help.demo.recipe1_name" => "Овсяные хлопья",
        "help.demo.recipe2_name" => "Творог 5%",
        "help.article.copy_day.p1" => "Откройте нужный прошлый день стрелками ‹ › вверху дневника.",
        "help.article.copy_day.p2" => "У каждой записи прошлого дня справа есть кнопка повтора (круговые стрелки). Нажмите её и выберите «Повторить сегодня» — еда скопируется в сегодняшний день.",
        "help.article.recipes.p1" => "Откройте вкладку «Рецепты» и нажмите «+ Новый».",
        "help.article.recipes.p2" => "Добавьте ингредиенты кнопкой «+ Добавить ингредиент», каждый со своим весом, затем нажмите «Завершить» и укажите итоговый вес готового блюда — программа посчитает КБЖУ на 100 г.",
        "help.article.recipes.p3" => "Готовое блюдо потом вносится в дневник через поиск: начните вводить его название и выберите, как любую другую еду.",
        "help.article.delete_food.p1" => "Нажмите «⋮» у записи в дневнике и выберите «Удалить».",
        "help.article.edit_weight.p1" => "Нажмите на число с весом (например «150 г») у записи в дневнике.",
        "help.article.edit_weight.p2" => "В открывшемся окне поменяйте вес — КБЖУ пересчитаются автоматически.",
        "help.article.rename_food.p1" => "Нажмите «⋮» у записи и выберите «Изменить», затем поменяйте название.",
        "help.article.rename_food.p2" => "Это удобно, когда ИИ ошибся с названием — такое иногда случается при распознавании по фото.",
        "help.article.diary.intro" => "Ежедневно нужно заполнять три параметра:",
        "help.article.weigh.intro" => "Взвешивайтесь каждый день — так алгоритм видит ТРЕНД веса, а не случайные скачки. Чтобы значения были сопоставимы день ото дня, соблюдайте одинаковые условия:",
        "help.article.weigh.p1" => "Взвешивайтесь утром, сразу после пробуждения.",
        "help.article.weigh.p2" => "До еды и питья.",
        "help.article.weigh.p3" => "После туалета.",
        "help.article.weigh.p4" => "До душа и умывания.",
        "help.article.weigh.p5" => "Без одежды (или каждый раз в одинаковой лёгкой одежде).",
        "help.article.weigh.record" => "Записывайте вес на главном экране — виджет веса, кнопка «+». Одна запись в день, её можно изменить.",
        "help.article.weigh.how_title" => "Как открыть форму",
        "help.article.weigh.open1" => "На главном экране нажмите на виджет веса:",
        "help.article.weigh.open1b" => "Если вы уже записывали вес, виджет выглядит как график — нажмите на него так же:",
        "help.article.weigh.open2" => "Откроется окно с графиком веса и историей. Внизу нажмите «Взвеситься»:",
        "help.article.weigh.open3" => "Впишите вес, отметьте выполненные условия и нажмите «Сохранить». Одна запись в день — её можно изменить.",
        "help.article.weigh.fluct" => "Вес колеблется день ото дня из-за воды, соли, а у женщин — из-за менструального цикла. Это нормально: алгоритм сам учитывает эти колебания и смотрит на тренд, поэтому просто взвешивайтесь каждый день и не переживайте из-за одного значения.",
        "help.article.steps.intro" => "Каждый день записывайте, сколько шагов вы прошли. Шаги — это ежедневная активность, которая тратит калории без спорта.",
        "help.article.steps.p1" => "Берите число из шагомера телефона или приложения здоровья (Apple Health, Google Fit, «Здоровье»).",
        "help.article.steps.p2" => "Вносите его на главном экране — виджет шагов, кнопка «+».",
        "help.article.steps.p3" => "Раз в день: вечером за сегодня или утром за вчера.",
        "help.article.steps.p4" => "Ориентир — не меньше 7000 шагов в день: это уже даёт заметное улучшение здоровья.",
        "help.article.steps.how_title" => "Как открыть форму",
        "help.article.steps.open1" => "На главном экране нажмите на виджет шагов:",
        "help.article.steps.open1b" => "Если вы уже записывали шаги, виджет выглядит как график — нажмите на него так же:",
        "help.article.steps.open2" => "Откроется окно с графиком шагов. Нажмите «Записать шаги»:",
        "help.article.steps.open3" => "Выберите день (сегодня / вчера), впишите число шагов и нажмите «Сохранить».",
        "cycle.title" => "Цикл",
        "cycle.day_label" => "День",
        "cycle.not_set" => "—",
        "cycle.first_day" => "Первый день цикла",
        "cycle.set_first_day" => "Задать первый день цикла",
        "cycle.set_prompt" => "Задайте первый день цикла, чтобы отслеживать фазы.",
        "cycle.weight_heading" => "Вес",
        "cycle.training_heading" => "Тренировки",
        "cycle.save" => "Сохранить",
        "cycle.cancel" => "Отмена",
        "cycle.phase.menstrual.name" => "Менструальная фаза",
        "cycle.phase.menstrual.desc" => "Начало цикла: идёт менструация, уровень гормонов на минимуме.",
        "cycle.phase.menstrual.weight" => "В начале менструации тело задерживает воду и возможно вздутие, поэтому вес на весах бывает выше обычного — это не жир. Ближе к концу фазы вода уходит и вес падает; эти колебания алгоритм уже учитывает сам.",
        "cycle.phase.menstrual.training" => "Самочувствие часто снижено — уменьшите интенсивность и больше отдыхайте. Лёгкая активность, ходьба и растяжка подойдут лучше тяжёлых нагрузок.",
        "cycle.phase.follicular.name" => "Фолликулярная фаза",
        "cycle.phase.follicular.desc" => "Организм готовится к овуляции: растёт эстроген, прибавляется энергия.",
        "cycle.phase.follicular.weight" => "В эту фазу вода почти не задерживается, и цифра на весах обычно спокойная, без резких скачков. Если вес немного «гуляет» — это нормальные суточные колебания, алгоритм их сглаживает.",
        "cycle.phase.follicular.training" => "Энергия и восстановление на подъёме — хорошее время для силовых и интенсивных тренировок. Можно повышать нагрузку и идти на личные рекорды.",
        "cycle.phase.ovulation.name" => "Овуляция",
        "cycle.phase.ovulation.desc" => "Середина цикла: выход яйцеклетки, пик эстрогена и энергии.",
        "cycle.phase.ovulation.weight" => "На пике гормонов возможна лёгкая задержка воды, поэтому вес может слегка подрасти на день-два. Это временно и на реальный прогресс не влияет — алгоритм это учитывает.",
        "cycle.phase.ovulation.training" => "Пик силы и выносливости — отличный день для тяжёлых тренировок. Связки в этот период чуть более расслаблены, поэтому уделите больше внимания разминке.",
        "cycle.phase.luteal.name" => "Лютеиновая фаза",
        "cycle.phase.luteal.desc" => "Вторая половина цикла: растёт прогестерон, тело склонно задерживать воду.",
        "cycle.phase.luteal.weight" => "Во второй половине цикла тело задерживает больше воды — вес на весах может подскочить на 0,5–2 кг без всякой причины. Это вода, а не жир: после начала менструации она уйдёт, и алгоритм уже закладывает эти колебания.",
        "cycle.phase.luteal.training" => "Энергия снижается, восстановление замедляется — уменьшите объём и добавьте отдыха. Возможна тяга к еде: держите фокус на белке и планке по калориям.",
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
        "chat.context" => "Контекст (вызовы тулов)",
        "chat.mic_denied" => "Доступ к микрофону запрещён",
        "chat.mode_ai" => "ИИ",
        "chat.mode_live" => "Живой человек",
        "chat.live_empty" => "Сообщений пока нет. Напишите живому оператору поддержки — он ответит здесь.",
        "chat.live_sending" => "отправка…",
        "chat.live_retry" => "не отправлено, нажмите чтобы повторить",

        // Запрос данных куратора: панель + отправка
        "curator.request_title" => "Запрос куратора",
        "curator.request_body" => "Куратор запрашивает у вас параметры тела",
        "curator.request_food" => "Куратор запрашивает у вас ваш дневник питания",
        "curator.request_weight" => "Куратор запрашивает у вас ваш дневник веса",
        "curator.request_steps" => "Куратор запрашивает у вас ваш дневник шагов",
        "curator.request_all" => "Куратор запрашивает у вас все ваши данные",
        "curator.share" => "Поделиться",
        "curator.sharing" => "Отправка…",
        "curator.shared_done" => "Данные отправлены",
        "curator.shared_body" => "Данные отправлены: параметры тела",
        "curator.shared_food" => "Данные отправлены: дневник питания",
        "curator.shared_weight" => "Данные отправлены: дневник веса",
        "curator.shared_steps" => "Данные отправлены: дневник шагов",
        "curator.shared_all" => "Данные отправлены: все ваши данные",

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

        // Connectivity warning (dashboard triangle)
        "net.offline_title" => "Не удаётся подключиться к серверу",
        "net.offline_body_vpn" => "Данные сохраняются на устройстве. Попробуйте включить или выключить VPN.",
        "net.degraded_title" => "Часть сервисов недоступна",
        "net.degraded_body" => "Данные сохраняются локально; временно недоступно:",
        "net.worker.ai" => "ИИ",
        "net.worker.sync" => "синхронизация",
        "net.worker.auth" => "вход",
        "net.worker.payment" => "подписка",
        "net.worker.ocr" => "распознавание этикеток",
        "net.worker.bug" => "отчёты об ошибках",
        "net.worker.support" => "чат поддержки",
        "net.worker.push" => "уведомления",
        "offline_gate.title" => "Нет подключения",
        "offline_gate.body" => "Не удаётся связаться с сервером, чтобы завершить настройку. Это проблема с сетью — проверьте интернет или VPN и повторите.",
        "offline_gate.retry" => "Повторить",
        "dashboard.calories_title" => "Калории",
        "chart.average" => "среднее",
        "chart.no_data" => "Пока нет данных",
        "chart.hint" => "Коснитесь графика, чтобы увидеть день",

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
        "recipes.change_weight" => "Изменить окончательный вес",
        "recipes.complete" => "Готов",
        "recipes.in_progress" => "Готовится",

        // Детали рецепта
        "recipe.loading" => "Загрузка...",
        "recipe.back" => "\u{2190} Рецепты",
        "recipe.name_placeholder" => "Название блюда",
        "recipe.name_required" => "Введите название блюда",
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
        "settings.version_check" => "Проверить обновление",
        "settings.version_checking" => "Проверяю…",
        "settings.dev" => "Разработка",
        "settings.dev_refresh" => "Обновить лог",
        "settings.dev_copy" => "Скопировать",
        "settings.dev_clear" => "Очистить",
        "settings.dev_empty" => "Пока нет диагностики. Нажмите «Проверить уведомления», тапните пуш, затем «Обновить лог».",
        "settings.subscription" => "Подписка",
        "settings.sub_active" => "Подписка активна",
        "settings.sub_trial" => "Пробный период",
        "settings.sub_expired" => "Подписка истекла",
        "settings.sub_cancelled" => "Отменена — активна до конца периода",
        "settings.sub_cancel" => "Отменить подписку",
        "settings.sub_cancel_confirm" => "Отменить автопродление? Доступ сохранится до конца текущего периода.",
        "settings.sub_cancel_msg" => "Отменить подписку? Доступ сохранится ещё {n}.",
        "settings.sub_refund" => "Запросить возврат",
        "settings.sub_refund_title" => "Запросить возврат?",
        "settings.sub_refund_warn" => "Запрос возврата сразу прервёт доступ к приложению.",
        "settings.sub_refund_amount" => "Сумма возврата",
        "settings.sub_refund_processing" => "На обработку запроса нужна неделя, плюс время на возврат банковского платежа.",
        "settings.sub_refund_confirm" => "Запросить возврат",
        "settings.sub_refund_error" => "Не удалось создать запрос на возврат. Попробуйте ещё раз.",
        "settings.sub_cancel_note" => "Отменить можно в любой момент — здесь, по ссылке в письмах lava или написав на info@renorma.app. Вход в приложение не требуется.",
        "settings.sub_buy_on_site" => "Подписка не активна. Оформить её можно на сайте.",
        "settings.sub_open_site" => "Открыть сайт",
        "settings.sub_renew_after" => "Возобновить подписку можно будет через {n} — когда истечёт текущий доступ.",
        "settings.sub_buy_in_tg" => "Подписка оформляется в Telegram.",
        "settings.sub_open_tg" => "Открыть в Telegram",
        "settings.sub_manage" => "Управление подпиской",
        "settings.sub_since" => "Подписан с",
        "settings.sub_until" => "Действует до",
        "settings.sub_access_left" => "Доступ ещё",
        "settings.sub_cost" => "Стоимость",
        "settings.account" => "Аккаунт",
        "settings.backup" => "Резервный доступ",
        "backup.title" => "Резервный доступ",
        "backup.back" => "Настройки",
        "backup.desc" => "Резервная фраза позволяет войти на новом устройстве без passkey. Храните её в тайне — любой, у кого она есть, получит доступ к аккаунту.",
        "backup.generate" => "Создать резервную фразу",
        "backup.regenerate" => "Сгенерировать новую фразу",
        "backup.generating" => "Генерирую…",
        "backup.your_phrase" => "Ваша фраза",
        "backup.warning" => "Сохраните фразу в надёжном месте. Новая фраза заменит старую.",
        "backup.retry_failed" => "Не удалось создать фразу — попробуйте ещё раз",
        "settings.logout" => "Выйти",
        "settings.logout_confirm" => "Выйти из аккаунта? Данные синхронизированы и остаются на устройстве — после входа всё вернётся.",
        "settings.danger_zone" => "Опасные дела",
        "settings.danger_delete_diary" => "Удалить данные дневника",
        "settings.danger_delete_old" => "Удалить данные старше 1 года",
        "settings.danger_confirm_old" => "Удалить записи дневника старше 1 года? Это необратимо.",
        "settings.danger_delete_all" => "Удалить все данные",
        "settings.danger_confirm_all" => "Удалить ВСЕ записи дневника? Это необратимо.",
        "settings.nutrient_placeholder" => "Omega 3, Fiber...",

        // Редактор продукта
        "food_editor.product_name" => "Название или описание блюда",
        "food_editor.name_field" => "Название",
        "food_editor.name_field_ph" => "Название продукта",
        "food_editor.recommended_abbr" => "реком.",
        "ai.extracted_from_label" => "Извлечено с этикетки",
        "food_editor.add_photo" => "Добавить фото этикетки",
        "food_editor.add_more_photo" => "Добавить ещё фото",
        "food_editor.add_photo_short" => "Фото",
        "food_editor.detect_food" => "Определить еду",
        "food_editor.photo_hint" => "Снимайте таблицу КБЖУ крупно, чтобы она занимала весь кадр — мелкий или далёкий текст распознаётся плохо.",
        "food_editor.ai_uploading" => "Загрузка фото\u{2026}",
        "food_editor.ai_queue" => "В очереди:",
        "food_editor.ai_recognizing" => "Распознаётся\u{2026}",
        "food_editor.ai_timeout" => "Распознавание не успело — попробуйте позже.",
        "food_editor.filling" => "Заполняю...",
        "food_editor.fill_info" => "Заполнить питательную ценность",
        "food_editor.tab_by_name" => "По описанию",
        "food_editor.tab_by_photo" => "По этикетке",
        "food_editor.tab_by_food_photo" => "По фото еды",
        "food_editor.food_photo_soon" => "Распознавание готового блюда по фото — перечислит продукты и их вес. Скоро.",
        "food_editor.food_photo_hint" => "Снимайте всю тарелку сверху, с ориентиром масштаба (вилка, рука). Вес — оценка, поправьте его у каждого продукта.",
        "food_editor.detected_title" => "На фото распознано",
        "food_editor.auto_tag" => "авто",
        "food_editor.suggested_tag" => "проверьте",
        "food_editor.no_food_detected" => "На фото не распозналась еда — попробуйте снимок чётче.",
        "food_editor.total" => "Итого",
        "food_editor.add_all" => "Добавить все продукты",
        "food_editor.detect_by_name" => "Заполнить пищевую ценность",
        "food_editor.detect_short" => "Заполнить",
        "food_editor.detect_by_photo" => "Определить калорийность",
        "food_editor.calories" => "Калории",
        "food_editor.protein" => "Белки",
        "food_editor.fat" => "Жиры",
        "food_editor.carbs" => "Углеводы",
        "food_editor.add" => "Добавить",
        "food_editor.paywall_title" => "Подписка не активна",
        "food_editor.paywall_body" => "Автоматическое распознавание КБЖУ доступно по активной подписке.",
        "food_editor.paywall_pay" => "Оплатить подписку",
        "food_editor.paywall_dismiss" => "Не сейчас",

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
        "common.unit.steps" => "шагов",

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
        "settings.height" => "Рост",
        "settings.height_label" => "Рост, см",
        "settings.height_why" => "Зачем это нужно: вместе с весом рост даёт ваш ИМТ — грубую оценку того, насколько много жира в массе тела.",
        "settings.bmi" => "Ваш ИМТ: {n}",
        "settings.goal" => "Цель курса",
        "settings.goal_lose" => "Похудение",
        "settings.goal_maintain" => "Поддержка",
        "settings.goal_why" => "На что нацелена вся дисциплина. На поддержке мы никогда не предлагаем снижать вашу планку по калориям.",
        "settings.birth_year" => "Год рождения",
        "settings.birth_year_label" => "Год рождения",
        "settings.birth_year_why" => "Зачем это нужно: возраст нужен, чтобы оценить, сколько калорий тратит ваше тело, и рассчитать обоснованную рекомендацию.",

        // Карточка еженедельной рекомендации

        // Онбординг (после оплаты на лендинге: регистрация → привязка подписки)
        "onboard.title" => "Создайте аккаунт",
        "onboard.subtitle" => "Оплата прошла. Создайте аккаунт — мы привяжем к нему вашу подписку.",
        "onboard.claiming" => "Привязываем подписку…",
        "onboard.pending_title" => "Подтверждаем оплату…",
        "onboard.pending_body" => "Это может занять немного времени. Мы продолжим проверять автоматически.",
        "onboard.retry" => "Повторить",
        "onboard.error_title" => "Не удалось привязать подписку",
        "onboard.error_body" => "Возможно, этот платёж уже привязан к другому аккаунту. Если это ошибка, напишите на info@renorma.app.",
        "onboard.link_unavailable" => "Ссылка недействительна или уже использована. Оформите подписку заново.",
        "onboard.have_account" => "Уже есть аккаунт? Войти",
        "onboard.success" => "Готово! Открываем приложение…",

        // Авторизация
        "auth.main_description" => "Это приложение работает локально на вашем устройстве и не хранит данные на удалённых серверах. Однако для некоторых функций — таких как синхронизация между устройствами или ИИ — необходимо авторизоваться.",
        "auth.create_account" => "Зарегистрироваться",
        "auth.already_used" => "Я уже пользовался этим приложением:",
        "auth.creating" => "Создаю...",
        "auth.authenticating" => "Вхожу...",
        "locked.title" => "Нужна подписка",
        "locked.body" => "У этого аккаунта нет активной подписки. Подписка оформляется на сайте. Если у вас есть другой аккаунт — войдите ниже.",
        "auth.login_title" => "Войти",
        "auth.login_have_device" => "Если у вас есть другое устройство, где вы вошли:",
        "auth.login_option1_hint" => "На другом устройстве: Настройки → Подключить устройство → Сканировать QR-код. Затем нажмите здесь:",
        "auth.login_option2_hint" => "На другом устройстве: Настройки → Подключить устройство → Показать QR-код. Затем нажмите здесь:",
        "auth.login_no_device" => "Если у вас нет залогиненного устройства:",
        "auth.try_passkey" => "Попробовать войти с ключом входа",
        "auth.tagline" => "Норма веса, питания и образа жизни.",
        "auth.sign_in" => "Войти",
        "auth.register" => "Регистрация",
        "auth.phrase_login" => "Войти по фразе",
        "auth.phrase_title" => "Вход по фразе",
        "auth.phrase_hint" => "Введите резервную фразу, чтобы войти на этом устройстве.",
        "auth.phrase_placeholder" => "ваши пять слов",
        "auth.phrase_back" => "Назад",
        "auth.phrase_invalid" => "Такая фраза не подходит ни к одному аккаунту.",
        "auth.phrase_rate_limited" => "Слишком много попыток. Попробуйте позже.",
        "auth.add_device" => "Добавить устройство",
        "auth.add_device_hint" => "На устройстве, где вы уже вошли: Настройки → Подключить устройство → Сканировать QR, затем наведите камеру на этот код.",
        "auth.scan_instead" => "Отсканировать QR вместо этого",
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
        "privacy.add_passkey" => "Добавить passkey на это устройство",
        "privacy.add_passkey_busy" => "Добавляю…",
        "privacy.add_passkey_done" => "Passkey добавлен ✓",

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
        "settings.notif_disable" => "Отключить уведомления",
        "settings.notif_enabled" => "Уведомления включены",
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
        "weight.empty_prompt" => "Нажмите сюда, чтобы записать вес",
        "weight.widget_placeholder" => "Здесь будет график вашего веса. Пока что график не изобразить, потому что слишком мало данных. Когда появится хотя бы три измерения, график будет нарисован.",
        "weight.trend.title" => "Тренд · 14 дней",
        "weight.trend.down" => "Снижается",
        "weight.trend.up" => "Растёт",
        "weight.trend.stable" => "Вес стоит на месте",
        "weight.trend.insufficient" => "Недостаточно данных для тренда",
        "weight.trend.preliminary" => "предварительно",
        "weight.trend.week" => "нед",
        "weight.trend.confidence" => "достоверность",
        "weight.trend.weak_down" => "Скорее снижается",
        "weight.trend.weak_up" => "Скорее растёт",
        "weight.trend.low_confidence" => "слабая уверенность",
        "weight.cycle.label" => "Месячные",
        "weight.cycle.none" => "цикл не обнаружен",
        "weight.cycle.insufficient" => "пока недостаточно данных",
        "weight.cycle.day_short" => "дн",
        "weight.cycle.decycled" => "Вес без месячных",

        "steps.title" => "Шаги",
        "steps.empty_prompt" => "Нажмите сюда, чтобы записать шаги",
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

        // История — глава 2 «Аппетит», секция 1: основная ошибка

        // История — глава 2, секция 2: фрукты и овощи

        // История — глава 2, секция 3: белок

        // История — глава 2, секция 4: низкокалорийная закусь

        // История — глава 2, секция 5: соки и газировка

        // История — глава 2, секция 6: сколько раз в день есть

        // История — глава 2, секция 7: еда на ночь

        // История — глава 3 «Начинаем худеть»
        // Глава 3, секция 1: Ищем дефицит (текст перед виджетом планки)
        // Глава 3, секция 2: Почему не уходит вес
        // Глава 3, секция 3: Калория
        // Глава 3, секция 4: Подруга ест много, но худая
        // Глава 3, секция 5: Сон
        // Глава 3, секция 6: Ходим больше
        // Глава 3, секция 7: Меняем ужасные привычки на плохие

        // История — глава 3, секция 1: зачем нам вообще жир

        // История — глава 3, секция 2: как выглядит красивый человек


        // История — глава 3, секция 3: физиологический минимум жира

        // История — глава 3, секция 4: худой против обезжиренного

        // История — глава 3, секция 5: жизнь без жира

        // История — глава 1, введение
        "progress.title" => "Фото прогресса",
        "progress.subtitle" => "Спереди, сбоку и со спины. Хранятся только на вашем устройстве.",
        "progress.capture" => "Сделать фото",
        "progress.tips_title" => "Рекомендации",
        "progress.tip_bg" => "Постарайтесь снимать на однотонном фоне.",
        "progress.tip_height" => "Разместите камеру на уровне груди.",
        "progress.history" => "История",
        "progress.empty" => "Пока нет фото.",
        "progress.pose_front" => "Прямо",
        "progress.pose_side" => "Сбоку",
        "progress.pose_back" => "Со спины",

        // История — глава 1, настроим приложение

        // История — глава 1, бухгалтерия

        // История — глава 1, первые записи еды

        // История — глава 1, активность и вес

        // История — глава 1, я готовлю

        // История — глава 1, моя еда с костями

        // История — глава 1, праздник или ресторан

        // История — глава 1, зачем вести дневник?

        // История — глава 1, облегчаем подсчёт

        // История — глава 1, подписка (онбординг-paywall)

        // История — глава 1, что дальше (paywall)
        "paywall.loading" => "Загрузка…",
        "paywall.contacting_payment" => "Обращаемся к платёжной системе…",
        "paywall.status_trial" => "Пробный период",
        "paywall.status_paid" => "Подписка активна",
        "paywall.status_expired" => "Подписка истекла",
        "paywall.days_left" => "дн. осталось",
        "paywall.choose_plan" => "Выберите план",
        "paywall.pay_button" => "Оформить подписку",
        "paywall.paying" => "Переход к оплате…",
        "paywall.per_month" => "/ мес",
        "paywall.per_year" => "/ год",
        "paywall.checkout_error" => "Не удалось начать оплату. Попробуйте ещё раз.",
        "paywall.not_configured" => "Оплата пока недоступна — загляните позже.",
        "paywall.success" => "Спасибо! Подписка активна.",
        "paywall.back_to_story" => "Назад к Истории",
        "paywall.welcome_title" => "Подписка оформлена 🎉",
        "paywall.welcome_body" => "Оплата прошла. Управлять подпиской можно в любой момент в «Настройки → Подписка» — там видно дату продления, стоимость и кнопка отмены.",
        "paywall.welcome_manage" => "Открыть «Настройки → Подписка»",
        "paywall.onb_title" => "Полный доступ к re:Norma",
        "paywall.later" => "Позже",
        "paywall.then" => "затем",
        "paywall.trial_left" => "Осталось: {n} дн. ознакомительного использования",
        "paywall.trial_expired" => "Ознакомительный период закончился",
        "paywall.price_line" => "Оформите подписку за {price} в месяц",
        "paywall.rule1" => "Попробуйте программу в течение 7 дней. После этого необходимо оформить подписку.",
        "paywall.rule2" => "В течение 7 дней вам доступен весь функционал.",
        "paywall.rule3" => "Подписку можно отменить в любое время.",
        "paywall.subscribe" => "Оформить",
        "paywall.skip" => "Пропустить",
        "paywall.promo_placeholder" => "Промокод (необязательно)",

        _ => "???",
    }
}
