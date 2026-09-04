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

/// Есть ли перевод ключа на ОБА языка.
///
/// Ключи, которые код собирает из кусков, легко разъезжаются со словарём, а на
/// экране это «???» вместо объяснения. Проверяется тестами.
#[cfg(test)]
pub(crate) fn translated_everywhere(key: &str) -> bool {
    en(key) != "???" && ru(key) != "???"
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
        "sync.pending_body" => "Syncing your data to the server is not finished because of network problems. You may lose data.",
        "sync.pending_retry" => "Continue syncing",
        "sync.pending_sending" => "Sending…",
        "errors.none" => "No errors.",
        "errors.copied" => "Copied ✓",
        "errors.clear" => "Clear",
        "mail.title" => "Inbox",
        "mail.empty" => "No messages yet.",
        "letters.recompute_now" => "Recalculate now",
        "chat.peer_support" => "Support",
        "chat.peer_curator" => "Curator",
        "dashboard.close" => "Done",
        "dashboard.sex" => "Sex",
        "dashboard.sex_male" => "Male",
        "dashboard.sex_female" => "Female",
        "dashboard.height" => "Height, cm",
        "dashboard.birth_year" => "Year of birth",
        "persona.intro" => "re:Norma is a weight-loss app. For it to work properly it needs to know a few things about you:",
        "persona.need_sex" => "Choose your sex.",
        "persona.need_height" => "Add your height.",
        "persona.need_year" => "Enter your year of birth.",
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
        "dashboard.progress.gate_title" => "Keep the calories, fruit/veg and protein indicators green to unlock the {week}.",
        "dashboard.progress.steps_gate_title" => "Keep the steps indicator green to unlock the {week}.",
        "dashboard.progress.calcium_gate_title" => "Keep the calcium indicator green to unlock the {week}.",
        "dashboard.progress.iron_gate_title" => "Meet the iron target to unlock the {week}.",
        "dashboard.progress.iron_done_title" => "You have met the iron target.",
        "dashboard.progress.iron_done_progress" => "{week} opens in {n} {w}.",
        "dashboard.progress.fat_gate_title" => "Meet the omega-3 target to unlock the {week}.",
        "dashboard.progress.fat_done_title" => "You have met the omega-3 target.",
        "dashboard.progress.red_meat_gate_title" => "Keep red meat within the weekly limit to unlock the {week}.",
        "dashboard.progress.red_meat_over_title" => "You have eaten too much red meat this week. We will try again next week.",
        "dashboard.progress.egg_gate_title" => "Eat seven eggs this week to unlock the {week}.",
        "dashboard.progress.fiber_gate_title" => "Keep calories, protein, steps and fruit/veg green.",
        "dashboard.progress.fiber_optional_progress" => "Fibre is optional: {g} g a week if you want it.",
        "dashboard.progress.days_left_progress" => "Left: {n} {w}.",
        "dashboard.progress.week_steps" => "steps week",
        "dashboard.progress.week_calcium" => "calcium week",
        "dashboard.progress.week_iron" => "iron week",
        "dashboard.progress.week_fat" => "fat week",
        "dashboard.progress.week_red_meat" => "red meat week",
        "dashboard.progress.week_egg" => "egg week",
        "dashboard.progress.week_fiber" => "fibre week",
        "dashboard.progress.week_fat_nom" => "The fat week",
        "dashboard.progress.week_red_meat_nom" => "The red meat week",
        "dashboard.progress.kcal_day" => "kcal/day",
        "dashboard.progress.done_hint" => "We'll adjust it as observations come in.",
        "dashboard.progress.help_1" => "Our algorithm will calculate your calorie target for you.",
        "dashboard.progress.help_2" => "For it to start working, you need to log your food every day.",
        "dashboard.progress.help_3" => "Tap the question mark to see how to do it.",
        "help.back" => "Back",
        "help.food.title" => "How to log food",
        "help.food.intro" => "For the algorithm to calculate your calorie target, food has to be logged every day. Here's how.",
        "help.food.where_title" => "Where to add food",
        "help.food.where_text" => "Open the «Diary» tab in the bottom menu. It has three meal panels — Breakfast, Lunch and Dinner. Tap the «+» on a meal (or its title) to add food to that meal.",
        "help.food.no_base" => "There's no global food database. You enter foods yourself — by hand, with an AI request, or by photo recognition. This gradually builds your own personal database of the foods you eat.",
        "help.food.new_how_title" => "How to open the form",
        "help.food.new_how1" => "On the diary, tap the meal's «+» and start searching for the product by name:",
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
        "chat.input_placeholder" => "",
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
        // ── Кураторские директивы: тексты собираются НА УСТРОЙСТВЕ ──
        "planka.label.calories" => "Calories",
        "planka.label.protein" => "Protein",
        "planka.label.steps" => "Steps",
        "planka.label.veg_fruit" => "Vegetables and fruit",
        "planka.label.calcium" => "Calcium",
        "planka.label.fiber" => "Fibre",
        "planka.label.iron" => "Iron",
        "planka.label.heme" => "Heme iron",
        "planka.label.epa_dha" => "Omega-3 (EPA+DHA)",
        "planka.label.fat_ratio" => "Fat balance",
        "planka.label.red_meat" => "Red meat",
        "planka.label.egg" => "Eggs",
        "planka.name.calories" => "calorie target",
        "planka.name.protein" => "protein target",
        "planka.name.steps" => "step target",
        "planka.name.veg_fruit" => "vegetables & fruit target",
        "planka.name.calcium" => "calcium target",
        "planka.name.fiber" => "fibre target",
        "planka.name.iron" => "iron target",
        "planka.name.heme" => "heme iron target",
        "planka.name.epa_dha" => "omega-3 (EPA+DHA) target",
        "planka.name.fat_ratio" => "fat balance target",
        "planka.name.red_meat" => "red meat limit",
        "planka.name.egg" => "egg target",
        "planka.unit.kcal" => "kcal",
        "planka.unit.g" => "g",
        "planka.unit.mg" => "mg",
        "planka.unit.steps" => "steps",
        "planka.unit.portions" => "portions/week",
        "planka.unit.pieces" => "pcs/week",
        "curator.note.planka_set" => "Your curator set your {what}: {value}",
        "curator.note.week_open" => "Your curator opened a new topic — {what}",
        "curator.note.week_open_plain" => "Your curator opened your next topic",
        "curator.letter.planka_set" => "Your curator set your {what}: {value}.\n\nIt already applies.",
        "curator.letter.unbound" => "You and your curator are no longer working together.\n\nThe app is leading your targets again: the constant ones are back to our own figures, while calories and steps stay as your curator left them until the next weekly recalculation. You can recalculate them right now instead of waiting.",
        "curator.letter.unbound_list" => "Here is what applies from now on:",
        "curator.letter.week_open" => "Your curator opened your next topic — {what}.\n\nThe new scales and badges are already on the main screen, and the story about it is waiting in the tray at the top.",
        "curator.week.activity" => "activity and steps",
        "curator.week.calcium" => "calcium",
        "curator.week.iron" => "iron",
        "curator.week.fats" => "fats",
        "curator.week.red_meat" => "red meat",
        "curator.invite.ask" => "{name} wants to add you to their client list",
        "curator.invite.explain" => "Your curator will be able to ask you for your data and adjust your targets. Your data stays on your device — nothing leaves it until you send a report yourself.",
        "curator.invite.replaces" => "You already have a curator. Accepting will end that connection.",
        "curator.invite.accept" => "Accept",
        "curator.invite.decline" => "Not now",
        "curator.invite.done" => "{name} is now your curator",
        "curator.invite.done_body" => "An upload button has appeared on your dashboard — that is how you send reports.",
        "curator.invite.dead_title" => "This invitation is no longer valid",
        "curator.invite.dead_body" => "It has already been used, or the link is wrong. Ask your curator for a new one.",
        "curator.invite.need_app_title" => "First set up the app",
        "curator.invite.need_app_body" => "Invitations are for people who already use the app. Set it up, then open the link again from the installed app.",
        "curator.invite.need_app_cta" => "Get the app",
        "curator.invite.to_app" => "Open the app",
        "curator.invite.failed" => "Could not open the invitation",
        "curator.request_title" => "Curator's request",
        "curator.request_body" => "The curator is asking you for your body parameters",
        "curator.request_food" => "The curator is asking you for your food diary",
        "curator.request_weight" => "The curator is asking you for your weight diary",
        "curator.request_steps" => "The curator is asking you for your steps diary",
        "curator.request_all" => "The curator is asking you for all of your data",
        "curator.request_system" => "The curator is asking you for your device and browser info",
        "curator.share" => "Share",
        "curator.sharing" => "Sharing…",
        "curator.shared_done" => "Data sent",
        "curator.shared_body" => "Data sent: body parameters",
        "curator.shared_food" => "Data sent: food diary",
        "curator.shared_weight" => "Data sent: weight diary",
        "curator.shared_steps" => "Data sent: steps diary",
        "curator.shared_all" => "Data sent: all your data",
        "curator.shared_system" => "Data sent: device and browser info",
        "curator.report.title" => "Report to curator",
        "curator.report.your_curator" => "Your curator",
        "curator.report.requested" => "Your curator is asking for your data.",
        "curator.report.last_sent" => "Last report sent on {date}.",
        "curator.report.never_sent" => "You have not sent a report yet.",
        "curator.report.send" => "Send report",
        "curator.report.what" => "What to send",
        "curator.report.only_new" => "Only what is new",
        "curator.report.only_new_hint" => "Everything after {date} — the last day of your previous report.",
        "curator.report.everything" => "Everything",
        "curator.report.through_hint" => "Today is never included: the day is still being filled in.",
        "curator.report.unbind_hint" => "You can stop working with your curator at any time. Your targets stay as they are and will be recalculated automatically in a week.",
        "curator.report.unbind" => "Disconnect from curator",
        "curator.report_sent" => "Report sent",

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
        "chart.planka" => "goal",
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
        "diary.move" => "Move",
        "diary.move_to" => "Move to meal",
        "diary.duplicate" => "Duplicate",
        "diary.edit" => "Edit",
        "diary.edit_product" => "Edit product",
        "diary.repeat_today" => "Repeat today",
        "diary.collapse" => "Collapse",
        "diary.expand" => "Expand",
        "diary.duplicate_to" => "Duplicate to…",
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
        "diary_add.other_food" => "Other food",
        "photo_crop.done" => "Done",
        "photo_crop.reset" => "Reset",
        "photo_crop.delete" => "Delete photo",
        "photo_crop.hint" => "Drag the frame corners. Drag the photo to move it, pinch to zoom",
        "other_food.photo_title" => "Photo",
        "other_food.photo_hint" => "Photograph the food on the plate, or the product label",
        "other_food.photo_how" => "How do I do that?",
        "other_food.add_photo" => "Add a photo",
        "other_food.photo_more" => "One more",
        "other_food.open_photo" => "Open photo",
        "other_food.description_title" => "Description",
        "other_food.description_hint" => "Say what you ate: the product names and how much of each. Anything the photo doesn't show.",
        "other_food.description_placeholder" => "For example: 150 g buckwheat and a cutlet",
        "other_food.description_empty" => "No description",
        "other_food.add" => "Add",
        "other_food.hint" => "The entry appears in the diary right away and recognises itself once there is a network",
        "other_food.not_recognised" => "Not recognised yet",
        "lazy_edit.top_title" => "Photos and description",
        "lazy_edit.bottom_title" => "What was recognised",
        "lazy_edit.will_reset" => "Photos or description changed — the entry will be recognised again",
        "lazy_edit.nothing_yet" => "Nothing yet: the entry has not been recognised",
        "lazy_edit.unknown_food" => "Food not found",
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
        "recipe.total_weight" => "Current ingredients weight:",
        "recipe.final_weight_label" => "Final weight of the finished dish",
        "recipe.final_weight_required" => "Enter the final weight of the dish",
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
        "food_editor.ai_filling_kbju" => "Looking up nutrition\u{2026}",
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
        "common.save" => "Save",
        "common.edit" => "Edit",
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
        "onboard.installed_title" => "re:Norma is installed as an app on your home screen.",
        "onboard.installed_body" => "Close the browser and open the app by tapping its icon on the home screen.",
        "onboard.installed_wait" => "Installing the app and putting the icon on the home screen can take a few minutes — please wait a little.",
        "onboard.installed_missing" => "If the app never showed up, something may have gone wrong because the VPN dropped. Try installing the app a second time.",
        "onboard.installed_show" => "Show the instructions",
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

        "auth.error_key_unknown" => "We cannot find your key on the server. You will have to register.",

        // --- Fallback when the key did not work ---
        "auth.passkey_trouble" => "It looks like we can't sign you in with your passkey.\n\nYou can sign in another way:",
        "auth.tg_login" => "Sign in with a Telegram code",
        "auth.checking_account" => "Checking the account…",
        "auth.state_unknown" => "Could not check the account. Check your internet and try again.",
        "auth.no_access_title" => "There is a problem with your account",
        "auth.no_access_body" => "Go back to the Telegram bot to pay.",
        "auth.open_bot" => "Open the Telegram bot",

        // --- PassKey failure reasons ---
        "pk.unsupported" => "This browser cannot work with PassKeys. Open the app in Safari or Chrome.",
        "pk.insecure" => "The page is not on a secure connection, so a PassKey cannot be created. This is on us — please contact support.",
        "pk.offline" => "No internet connection. A PassKey lives in the keychain and cannot be created offline. Reconnect and try again.",
        "pk.offline_note" => "The device is offline right now.",
        "pk.create.cancelled" => "PassKey creation was cancelled.",
        "pk.create.blocked" => "The system refused to create a PassKey without even asking you. This usually means the keychain is unavailable: check that iCloud Keychain (or Google sync on Android) is on and that you have internet.",
        "pk.create.timeout" => "Time ran out while creating the PassKey. Try again and confirm on your device.",
        "pk.create.exists" => "This device already has a key for this account. Do not create a new one — sign in with the existing key.",
        "pk.create.unsupported_algo" => "This device does not support the required key type. Tell us your device and browser.",
        "pk.create.origin" => "The page address does not match the domain the key is issued for. This is a configuration error on our side — please contact support.",
        "pk.create.no_screen_lock" => "A PassKey requires device protection. Turn on Face ID, Touch ID, fingerprint or a passcode and try again.",
        "pk.create.aborted" => "PassKey creation was interrupted. Try again.",
        "pk.create.storage" => "The keychain could not create the PassKey. Try again; if it repeats, restart the device.",
        "pk.create.bad_options" => "The server sent invalid key parameters. This is on us — please contact support.",
        "pk.create.unknown" => "The PassKey could not be created, reason unknown.",
        "pk.get.cancelled" => "PassKey sign-in was cancelled.",
        "pk.get.blocked" => "The system refused to present a PassKey without even asking you. Most likely there is no key on this device, or the keychain is unavailable.",
        "pk.get.timeout" => "Time ran out while confirming the PassKey. Try again.",
        "pk.get.no_key" => "There is no PassKey on this device. Sign in with a code or your recovery phrase.",
        "pk.get.unsupported_algo" => "This device does not support the required key type. Tell us your device and browser.",
        "pk.get.origin" => "The page address does not match the key's domain. This is a configuration error on our side — please contact support.",
        "pk.get.no_screen_lock" => "PassKey sign-in requires device protection. Turn on Face ID, Touch ID, fingerprint or a passcode and try again.",
        "pk.get.aborted" => "PassKey sign-in was interrupted. Try again.",
        "pk.get.storage" => "The keychain could not present the PassKey. Try again; if it repeats, restart the device.",
        "pk.get.bad_options" => "The server sent invalid sign-in parameters. This is on us — please contact support.",
        "pk.get.unknown" => "PassKey sign-in failed, reason unknown.",
        "pk.net.register_begin" => "Could not reach the server to start registration. No key was created yet — check your internet and try again.",
        "pk.net.login_begin" => "Could not reach the server to start sign-in. Check your internet and try again.",
        "pk.net.add_begin" => "Could not reach the server to add a key. No key was created yet — check your internet and try again.",
        "pk.net.pair_begin" => "Could not reach the server to link this device. Check your internet and try again.",
        "pk.net.register_finish" => "The PassKey was created on this device, but the server never learned about it: the connection dropped. Check your internet and try signing in with that key; if that fails, delete the re:Norma key in your password settings and register again.",
        "pk.net.login_finish" => "The PassKey was confirmed, but the server did not answer: the connection dropped. Check your internet and sign in again.",
        "pk.net.add_finish" => "The PassKey was created on this device, but the server never learned about it: the connection dropped. Check your internet and retry — if the key ends up added twice, delete the spare in your password settings.",
        "pk.net.pair_finish" => "The PassKey was created on this device, but the server never learned about it: the connection dropped. Check your internet and link the device again.",
        "pk.srv.register_begin" => "The server refused to start registration",
        "pk.srv.register_finish" => "The server rejected the created PassKey",
        "pk.srv.login_begin" => "The server refused to start sign-in",
        "pk.srv.login_finish" => "The server rejected the presented PassKey",
        "pk.srv.add_begin" => "The server refused to add the key",
        "pk.srv.add_finish" => "The server rejected the added PassKey",
        "pk.srv.pair_begin" => "The server refused to link the device",
        "pk.srv.pair_finish" => "The server rejected the new device's key",
        "auth.recovery_link" => "Recover access with password",
        "auth.recovery_title" => "Recover access",
        "auth.recovery_hint" => "Enter your recovery password to regain access to your account.",
        "auth.back" => "Back",
        "auth.name_placeholder" => "Your name",
        "auth.name_label" => "Display name",

        // PWA prompt
        "pwa.description" => "re:Norma has to be installed on your home screen. It will be a separate icon.",
        "pwa.title.ios" => "How to install on iPhone:",
        "pwa.title.android" => "How to install on Android:",
        "pwa.title.macos" => "How to install on Mac:",
        "pwa.title.desktop" => "How to install:",
        // iOS Safari
        // iOS Chrome/Firefox
        // Android Chrome
        // Android Samsung
        // Android Firefox
        // Android Yandex
        // System-browser hop screen (Android browsers that can't install a PWA).
        "pwa.sysbrowser.text" => "re:Norma works best in the system browser.",
        "pwa.sysbrowser.open" => "Open in the system browser",
        "pwa.sysbrowser.stay" => "I want to keep using this browser",
        "pwa.mi.title" => "The re:Norma app works in the Chrome browser.",
        "pwa.mi.open" => "Open in Chrome",
        "pwa.unknown.title" => "We don't know how to work with this browser.",
        "pwa.unknown.signal" => "Our development team got a signal that you tried to use our app in this browser, and we'll try to do something about it.",
        "pwa.unknown.chrome" => "It's best if you open the app in Chrome.",
        "pwa.unknown.safari" => "It's best if you open the app in Safari.",
        "pwa.unknown.step1" => "Copy this address — tap it.",
        "pwa.unknown.step2" => "Launch Chrome.",
        "pwa.unknown.step2_safari" => "Launch Safari.",
        "pwa.unknown.step3" => "Type the address into the search bar.",
        "pwa.unknown.copied" => "Address copied",
        "pwa.yandex.title" => "re:Norma works best in Chrome",
        "pwa.yandex.lead" => "You can use it in Yandex Browser. But it is inconvenient",
        "pwa.yandex.step1" => "To open it in Chrome, tap this little button at the bottom.",
        "pwa.yandex.step2" => "Then you pick a browser",
        // macOS Safari
        // Chrome (desktop & macOS)
        // Edge
        // Firefox desktop
        "pwa.desktop.mobile_first" => "This app is made for mobile devices.",
        "pwa.desktop.if_phone" => "If you are seeing this on a phone, «Desktop site» is turned on. Uncheck it in the browser menu.",
        "pwa.desktop.if_desktop" => "If you want to use the app on a computer, press the button below.",
        "pwa.use_browser" => "Use in the browser on desktop",

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
        "sync.pending_body" => "Синхронизация ваших данных с сервером не закончена из-за сетевых проблем. Вы можете потерять данные.",
        "sync.pending_retry" => "Продолжить синхронизацию",
        "sync.pending_sending" => "Отправляем…",
        "errors.none" => "Ошибок нет.",
        "errors.copied" => "Скопировано ✓",
        "errors.clear" => "Очистить",
        "mail.title" => "Письма",
        "mail.empty" => "Сообщений пока нет.",
        "letters.recompute_now" => "Пересчитать сейчас",
        "chat.peer_support" => "Поддержка",
        "chat.peer_curator" => "Куратор",
        "dashboard.close" => "Готово",
        "dashboard.sex" => "Пол",
        "dashboard.sex_male" => "Мужской",
        "dashboard.sex_female" => "Женский",
        "dashboard.height" => "Рост, см",
        "dashboard.birth_year" => "Год рождения",
        // Первый заход: объяснение и разбор формы по кнопке «Готово».
        "persona.intro" => "re:Norma — это приложение для похудения. Для того, чтобы оно работало корректно, ему необходимо знать некоторые данные о вас:",
        "persona.need_sex" => "Укажите пол.",
        "persona.need_height" => "Добавьте свой рост.",
        "persona.need_year" => "Введите год рождения.",
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
        // ЗАДАНИЕ НЕДЕЛИ — одной формой на все главы: что держать, ради чего, и
        // сколько осталось. «Чтобы открыть {week}» подставляется рядом с заданием, а
        // не отдельным предложением: цель без задания не читается.
        "dashboard.progress.gate_title" => "Держите индикаторы калорий, фруктов/овощей и белка зелёными, чтобы открыть {week}.",
        "dashboard.progress.steps_gate_title" => "Держите индикатор планки по шагам зелёным, чтобы открыть {week}.",
        "dashboard.progress.calcium_gate_title" => "Держите индикатор кальция зелёным, чтобы открыть {week}.",
        "dashboard.progress.iron_gate_title" => "Выполните планку по железу, чтобы открыть {week}.",
        // Планка закрыта, но неделя ещё идёт: человеку надо сказать, что он своё
        // сделал и ждёт только календаря, — иначе молчание читается как «не засчитано».
        "dashboard.progress.iron_done_title" => "Вы закрыли планку по железу.",
        // Глава названа по имени и здесь: «следующая история» ничего человеку не
        // говорила. Название стоит подлежащим, поэтому берётся ИМЕНИТЕЛЬНАЯ форма —
        // отдельными строками ниже; винительная («чтобы открыть неделю жиров») сюда
        // не годится.
        "dashboard.progress.iron_done_progress" => "{week} откроется через {n} {w}.",
        // Жиры: гейт закрывается по МОРСКИМ омега-3 (1.75 г за неделю), баланс жира в
        // условие не входит, — поэтому и зовём к омега-3, а не к «жирам вообще».
        "dashboard.progress.fat_gate_title" => "Наберите норму омега-3, чтобы открыть {week}.",
        "dashboard.progress.fat_done_title" => "Вы набрали норму омега-3.",
        // Красное мясо: планка обратная, её не выполняют, а не превышают.
        "dashboard.progress.red_meat_gate_title" => "Удержите красное мясо в пределах недельной планки, чтобы открыть {week}.",
        "dashboard.progress.red_meat_over_title" => "Вы съели слишком много красного мяса на этой неделе. На следующей неделе попробуем ещё раз.",
        // Яйца: планка снова ПРЯМАЯ — семь штук за неделю, это минимум. Следующей
        // главы за ними пока нет, поэтому цели в задании нет.
        "dashboard.progress.egg_gate_title" => "Наберите за неделю семь яиц, чтобы открыть {week}.",
        "dashboard.progress.fiber_gate_title" => "Держите зелёными калории, белок, шаги и фрукты/овощи.",
        "dashboard.progress.fiber_optional_progress" => "Клетчатка — по желанию: {g} г за неделю.",
        // Срок задания — одной строкой на все главы. У недельных глав это дни до
        // конца недели, у гейтов — недостающие ЗЕЛЁНЫЕ дни (семь штук в скользящем
        // окне восьми суток), но человеку они называются просто днями: разбираться в
        // механике окна ему незачем.
        "dashboard.progress.days_left_progress" => "Осталось: {n} {w}.",
        // Названия недель — в винительном падеже: они подставляются в «чтобы открыть».
        "dashboard.progress.week_steps" => "неделю шагов",
        "dashboard.progress.week_calcium" => "неделю кальция",
        "dashboard.progress.week_iron" => "неделю железа",
        "dashboard.progress.week_fat" => "неделю жиров",
        "dashboard.progress.week_red_meat" => "неделю красного мяса",
        "dashboard.progress.week_egg" => "неделю яиц",
        "dashboard.progress.week_fiber" => "неделю клетчатки",
        // Именительные формы — только у тех двух глав, что подставляются подлежащим
        // в строку про открытие: за железом идут жиры, за жирами — красное мясо.
        "dashboard.progress.week_fat_nom" => "Неделя жиров",
        "dashboard.progress.week_red_meat_nom" => "Неделя красного мяса",
        "dashboard.progress.kcal_day" => "ккал/день",
        "dashboard.progress.done_hint" => "Мы будем корректировать её по мере наблюдений.",
        "dashboard.progress.help_1" => "Наш алгоритм поможет рассчитать вам вашу планку по калориям.",
        "dashboard.progress.help_2" => "Для того чтобы он начал работать, необходимо ежедневно вносить еду.",
        "dashboard.progress.help_3" => "Нажмите на вопросик, чтобы понять, как это сделать.",
        "help.back" => "Назад",
        "help.food.title" => "Как вносить еду",
        "help.food.intro" => "Чтобы алгоритм рассчитал вашу планку по калориям, еду нужно вносить каждый день. Ниже — как это сделать.",
        "help.food.where_title" => "Где добавлять еду",
        "help.food.where_text" => "Откройте вкладку «Дневник» в нижнем меню. Там три панели приёмов пищи — Завтрак, Обед и Ужин. Нажмите «+» на нужном приёме (или на его названии), чтобы добавить туда еду.",
        "help.food.no_base" => "Глобальной базы продуктов нет. Продукты вы вносите сами — вручную по описанию, с помощью ИИ или распознавания по фото. Так постепенно собирается ваша личная база продуктов, которые вы едите.",
        "help.food.new_how_title" => "Как открыть форму",
        "help.food.new_how1" => "На дневнике нажмите «+» на приёме пищи и начните искать продукт по названию:",
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
        "chat.input_placeholder" => "",
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
        // ── Кураторские директивы: тексты собираются НА УСТРОЙСТВЕ ──
        "planka.label.calories" => "Калории",
        "planka.label.protein" => "Белок",
        "planka.label.steps" => "Шаги",
        "planka.label.veg_fruit" => "Овощи и фрукты",
        "planka.label.calcium" => "Кальций",
        "planka.label.fiber" => "Клетчатка",
        "planka.label.iron" => "Железо",
        "planka.label.heme" => "Гемовое железо",
        "planka.label.epa_dha" => "Омега-3 (EPA+DHA)",
        "planka.label.fat_ratio" => "Баланс жиров",
        "planka.label.red_meat" => "Красное мясо",
        "planka.label.egg" => "Яйца",
        "planka.name.calories" => "планку по калориям",
        "planka.name.protein" => "планку по белку",
        "planka.name.steps" => "планку по шагам",
        "planka.name.veg_fruit" => "планку по овощам и фруктам",
        "planka.name.calcium" => "планку по кальцию",
        "planka.name.fiber" => "планку по клетчатке",
        "planka.name.iron" => "планку по железу",
        "planka.name.heme" => "планку по гемовому железу",
        "planka.name.epa_dha" => "планку по омега-3 (EPA+DHA)",
        "planka.name.fat_ratio" => "планку по балансу жиров",
        "planka.name.red_meat" => "предел по красному мясу",
        "planka.name.egg" => "планку по яйцам",
        "planka.unit.kcal" => "ккал",
        "planka.unit.g" => "г",
        "planka.unit.mg" => "мг",
        "planka.unit.steps" => "шагов",
        "planka.unit.portions" => "порций в неделю",
        "planka.unit.pieces" => "шт. в неделю",
        "curator.note.planka_set" => "Куратор установил вам {what}: {value}",
        "curator.note.week_open" => "Куратор открыл вам новую тему — {what}",
        "curator.note.week_open_plain" => "Куратор открыл вам следующую тему",
        "curator.letter.planka_set" => "Ваш куратор установил вам {what}: {value}.\n\nОна уже применена.",
        "curator.letter.unbound" => "Работа с куратором прекращена.\n\nПланки снова ведёт приложение: постоянные нормы вернулись к нашим, а калории и шаги останутся кураторскими до ближайшего недельного пересчёта. Можно не ждать и пересчитать прямо сейчас.",
        "curator.letter.unbound_list" => "Вот что теперь соблюдать:",
        "curator.letter.week_open" => "Ваш куратор открыл вам следующую тему — {what}.\n\nНовые шкалы и значки уже на главном экране, а история про эту тему ждёт вас в ленте наверху.",
        "curator.week.activity" => "активность и шаги",
        "curator.week.calcium" => "кальций",
        "curator.week.iron" => "железо",
        "curator.week.fats" => "жиры",
        "curator.week.red_meat" => "красное мясо",
        "curator.invite.ask" => "{name} хочет добавить вас в список своих клиентов",
        "curator.invite.explain" => "Куратор сможет запрашивать у вас данные и корректировать ваши планки. Данные остаются на вашем устройстве — без вашей отправки они никуда не уходят.",
        "curator.invite.replaces" => "У вас уже есть куратор. Согласие прекратит прежнюю связь.",
        "curator.invite.accept" => "Согласен",
        "curator.invite.decline" => "Не сейчас",
        "curator.invite.done" => "{name} теперь ваш куратор",
        "curator.invite.done_body" => "На дашборде появилась кнопка отправки — ею вы будете отправлять отчёты.",
        "curator.invite.dead_title" => "Приглашение больше не действует",
        "curator.invite.dead_body" => "Им уже воспользовались, либо ссылка неверна. Попросите у куратора новую.",
        "curator.invite.need_app_title" => "Сначала заведите приложение",
        "curator.invite.need_app_body" => "Приглашения — для тех, кто уже пользуется приложением. Заведите его, а потом откройте ссылку ещё раз из установленного приложения.",
        "curator.invite.need_app_cta" => "Завести приложение",
        "curator.invite.to_app" => "Открыть приложение",
        "curator.invite.failed" => "Не удалось открыть приглашение",
        "curator.request_title" => "Запрос куратора",
        "curator.request_body" => "Куратор запрашивает у вас параметры тела",
        "curator.request_food" => "Куратор запрашивает у вас ваш дневник питания",
        "curator.request_weight" => "Куратор запрашивает у вас ваш дневник веса",
        "curator.request_steps" => "Куратор запрашивает у вас ваш дневник шагов",
        "curator.request_all" => "Куратор запрашивает у вас все ваши данные",
        "curator.request_system" => "Куратор запрашивает у вас данные об устройстве и браузере",
        "curator.share" => "Поделиться",
        "curator.sharing" => "Отправка…",
        "curator.shared_done" => "Данные отправлены",
        "curator.shared_body" => "Данные отправлены: параметры тела",
        "curator.shared_food" => "Данные отправлены: дневник питания",
        "curator.shared_weight" => "Данные отправлены: дневник веса",
        "curator.shared_steps" => "Данные отправлены: дневник шагов",
        "curator.shared_all" => "Данные отправлены: все ваши данные",
        "curator.shared_system" => "Данные отправлены: данные об устройстве",
        "curator.report.title" => "Отчёт куратору",
        "curator.report.your_curator" => "Ваш куратор",
        "curator.report.requested" => "Куратор запрашивает ваши данные.",
        "curator.report.last_sent" => "Последний отчёт отправлен {date}.",
        "curator.report.never_sent" => "Вы ещё не отправляли отчёт.",
        "curator.report.send" => "Отправить отчёт",
        "curator.report.what" => "Что отправить",
        "curator.report.only_new" => "Только новое",
        "curator.report.everything" => "Все данные",
        "curator.report.only_new_hint" => "Всё, что после {date} — последнего дня прошлого отчёта.",
        "curator.report.through_hint" => "Сегодняшний день не отправляется: он ещё заполняется.",
        "curator.report.unbind_hint" => "Прекратить работу с куратором можно в любой момент. Планки останутся как есть и через неделю пересчитаются автоматически.",
        "curator.report.unbind" => "Отвязаться от куратора",
        "curator.report_sent" => "Отчёт отправлен",

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
        "chart.planka" => "планка",
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
        // Перенос в другой приём пищи. «Перенести», а не «Переместить»: короче и
        // не путается с переносом на другой день, которого здесь нет.
        "diary.move" => "Перенести",
        "diary.move_to" => "Перенести в приём",
        "diary.duplicate" => "Дублировать",
        "diary.edit" => "Изменить",
        "diary.edit_product" => "Изменить продукт",
        "diary.repeat_today" => "Повторить сегодня",
        "diary.collapse" => "Свернуть",
        "diary.expand" => "Развернуть",
        "diary.duplicate_to" => "Дублировать в…",
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
        // Новый способ записи (features::LAZY_FOOD). Кнопка названа «Другая еда», а
        // не «Новая»: человек не заводит продукт в справочник, он записывает то, что
        // съел, и что из этого станет новым продуктом — наша забота.
        "diary_add.other_food" => "Другая еда",
        // Две зоны экрана. Каждая называет СВОЙ предмет, а подсказка под ней говорит,
        // что именно от человека нужно: снимок без объяснения выходит не тот (стол
        // целиком вместо тарелки), а описание без объяснения выходит в одно слово.
        // Просмотр и обрезка снимка. «Готово», а не «Сохранить»: снимок ещё не
        // записан никуда, человек просто закончил с ним возиться.
        "photo_crop.done" => "Готово",
        "photo_crop.reset" => "Сбросить",
        "photo_crop.delete" => "Удалить снимок",
        "photo_crop.hint" => "Тяните за углы рамки. Снимок двигается пальцем, двумя — приближается",
        "other_food.photo_title" => "Фотография",
        "other_food.photo_hint" => "Сфотографируйте еду на тарелке или этикетку продукта",
        "other_food.photo_how" => "Как это сделать?",
        "other_food.add_photo" => "Добавить снимок",
        "other_food.photo_more" => "Ещё снимок",
        "other_food.open_photo" => "Открыть снимок",
        "other_food.description_title" => "Описание",
        "other_food.description_hint" => "Опишите, что вы съели: наименования продуктов, их количество. Уточняющие комментарии к фотографии",
        "other_food.description_placeholder" => "Например: гречка 150 г и котлета",
        "other_food.description_empty" => "Описания не было",
        "other_food.add" => "Добавить",
        // «Добавить», а не «Распознать»: запись попадает в дневник сразу, а разбор
        // идёт фоном и может подождать сети.
        "other_food.hint" => "Запись появится в дневнике сразу, а распознается сама, когда будет сеть",
        "other_food.not_recognised" => "Ещё не распознано",
        "lazy_edit.top_title" => "Снимки и описание",
        "lazy_edit.bottom_title" => "Что распозналось",
        "lazy_edit.will_reset" => "Снимки или описание изменились — запись распознается заново",
        "lazy_edit.nothing_yet" => "Пока ничего: запись ещё не распознана",
        "lazy_edit.unknown_food" => "Продукт не найден",
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
        "recipe.total_weight" => "Текущий вес ингредиентов:",
        "recipe.final_weight_label" => "Окончательный вес готового блюда",
        "recipe.final_weight_required" => "Введите окончательный вес продукта",
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
        "food_editor.ai_filling_kbju" => "Определяем КБЖУ\u{2026}",
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
        "common.save" => "Сохранить",
        "common.edit" => "Изменить",
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
        "onboard.installed_title" => "re:Norma установлено как приложение на ваш рабочий стол.",
        "onboard.installed_body" => "Закройте браузер и откройте приложение, тапнув по иконке на рабочем столе.",
        "onboard.installed_wait" => "Установка приложения и появление иконки могут занять несколько минут — немного подождите.",
        "onboard.installed_missing" => "Если приложение так и не появилось, возможно, где-то произошла проблема из-за прервавшегося VPN. Попробуйте второй раз установить приложение.",
        "onboard.installed_show" => "Показать инструкцию",
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

        "auth.error_key_unknown" => "Мы не можем найти вашего ключа на сервере. Вам придётся зарегистрироваться.",

        // --- Обходной путь, когда ключ не сработал ---
        "auth.passkey_trouble" => "Кажется, мы не можем авторизовать вас в приложении через PassKey.\n\nВы можете авторизоваться другим способом:",
        "auth.tg_login" => "Войти по коду из Телеграм",
        "auth.checking_account" => "Проверяем аккаунт…",
        "auth.state_unknown" => "Не удалось проверить аккаунт. Проверьте интернет и попробуйте снова.",
        "auth.no_access_title" => "Возникла ошибка с вашим аккаунтом",
        "auth.no_access_body" => "Вернитесь в телеграм-бота, чтобы оплатить.",
        "auth.open_bot" => "Открыть телеграм-бота",

        // --- PassKey: причина отказа ---
        // Состояние страницы и устройства
        "pk.unsupported" => "Этот браузер не умеет работать с PassKey. Откройте приложение в Safari или Chrome.",
        "pk.insecure" => "Страница открыта не по защищённому протоколу — создать PassKey нельзя. Это наша ошибка, сообщите в поддержку.",
        "pk.offline" => "Нет связи с интернетом. PassKey хранится в связке ключей и без сети не создаётся. Подключитесь и попробуйте снова.",
        "pk.offline_note" => "Устройство сейчас без сети.",
        // Создание ключа
        "pk.create.cancelled" => "Создание PassKey отменено.",
        "pk.create.blocked" => "Система не дала создать PassKey — вас даже не спросили. Обычно это значит, что связка ключей недоступна: проверьте, что в настройках включена «Связка ключей iCloud» (на Android — синхронизация Google), и что есть интернет.",
        "pk.create.timeout" => "Время на создание PassKey истекло. Попробуйте ещё раз и подтвердите на устройстве.",
        "pk.create.exists" => "На этом устройстве уже есть ключ для этого аккаунта. Не создавайте новый — войдите по существующему.",
        "pk.create.unsupported_algo" => "Устройство не поддерживает нужный тип ключа. Сообщите нам, какое у вас устройство и браузер.",
        "pk.create.origin" => "Адрес страницы не совпадает с доменом, для которого выдаётся ключ. Это ошибка настройки на нашей стороне — сообщите в поддержку.",
        "pk.create.no_screen_lock" => "Для PassKey нужна защита устройства. Включите Face ID, Touch ID, отпечаток или код-пароль и повторите.",
        "pk.create.aborted" => "Создание PassKey прервалось. Попробуйте ещё раз.",
        "pk.create.storage" => "Хранилище ключей не смогло создать PassKey. Попробуйте ещё раз; если повторится — перезагрузите устройство.",
        "pk.create.bad_options" => "Сервер прислал негодные параметры для ключа. Это наша ошибка — сообщите в поддержку.",
        "pk.create.unknown" => "Не удалось создать PassKey по неизвестной причине.",
        // Предъявление ключа
        "pk.get.cancelled" => "Вход по PassKey отменён.",
        "pk.get.blocked" => "Система не дала предъявить PassKey — вас даже не спросили. Скорее всего, на этом устройстве ключа нет либо связка ключей недоступна.",
        "pk.get.timeout" => "Время на подтверждение PassKey истекло. Попробуйте ещё раз.",
        "pk.get.no_key" => "На этом устройстве нет PassKey для входа. Войдите по коду или по фразе восстановления.",
        "pk.get.unsupported_algo" => "Устройство не поддерживает нужный тип ключа. Сообщите нам, какое у вас устройство и браузер.",
        "pk.get.origin" => "Адрес страницы не совпадает с доменом ключа. Это ошибка настройки на нашей стороне — сообщите в поддержку.",
        "pk.get.no_screen_lock" => "Для входа по PassKey нужна защита устройства. Включите Face ID, Touch ID, отпечаток или код-пароль и повторите.",
        "pk.get.aborted" => "Вход по PassKey прервался. Попробуйте ещё раз.",
        "pk.get.storage" => "Хранилище ключей не смогло предъявить PassKey. Попробуйте ещё раз; если повторится — перезагрузите устройство.",
        "pk.get.bad_options" => "Сервер прислал негодные параметры для входа. Это наша ошибка — сообщите в поддержку.",
        "pk.get.unknown" => "Не удалось войти по PassKey по неизвестной причине.",
        // Связь с сервером: до создания ключа
        "pk.net.register_begin" => "Не удалось связаться с сервером, чтобы начать регистрацию. Ключ ещё не создан — проверьте интернет и попробуйте снова.",
        "pk.net.login_begin" => "Не удалось связаться с сервером, чтобы начать вход. Проверьте интернет и попробуйте снова.",
        "pk.net.add_begin" => "Не удалось связаться с сервером, чтобы добавить ключ. Ключ ещё не создан — проверьте интернет и попробуйте снова.",
        "pk.net.pair_begin" => "Не удалось связаться с сервером, чтобы подключить устройство. Проверьте интернет и попробуйте снова.",
        // Связь с сервером: ключ УЖЕ создан или предъявлен
        "pk.net.register_finish" => "PassKey создан на устройстве, но сервер о нём не узнал: связь прервалась. Проверьте интернет и попробуйте войти по этому ключу; если вход не удастся — удалите ключ re:Norma в настройках паролей и зарегистрируйтесь заново.",
        "pk.net.login_finish" => "PassKey подтверждён, но сервер не ответил: связь прервалась. Проверьте интернет и повторите вход.",
        "pk.net.add_finish" => "PassKey создан на устройстве, но сервер о нём не узнал: связь прервалась. Проверьте интернет и повторите — если ключ добавится дважды, лишний можно удалить в настройках паролей.",
        "pk.net.pair_finish" => "PassKey создан на устройстве, но сервер о нём не узнал: связь прервалась. Проверьте интернет и повторите подключение.",
        // Сервер ответил отказом
        "pk.srv.register_begin" => "Сервер не дал начать регистрацию",
        "pk.srv.register_finish" => "Сервер отклонил созданный PassKey",
        "pk.srv.login_begin" => "Сервер не дал начать вход",
        "pk.srv.login_finish" => "Сервер отклонил предъявленный PassKey",
        "pk.srv.add_begin" => "Сервер не дал добавить ключ",
        "pk.srv.add_finish" => "Сервер отклонил добавленный PassKey",
        "pk.srv.pair_begin" => "Сервер не дал подключить устройство",
        "pk.srv.pair_finish" => "Сервер отклонил ключ нового устройства",
        "auth.recovery_link" => "Восстановить доступ по паролю",
        "auth.recovery_title" => "Восстановление доступа",
        "auth.recovery_hint" => "Введите пароль восстановления для доступа к аккаунту.",
        "auth.back" => "Назад",
        "auth.name_placeholder" => "Ваше имя",
        "auth.name_label" => "Имя",

        // PWA
        "pwa.description" => "re:Norma необходимо установить на рабочий стол. Это будет отдельная иконка.",
        "pwa.title.ios" => "Как установить на iPhone:",
        "pwa.title.android" => "Как установить на Android:",
        "pwa.title.macos" => "Как установить на Mac:",
        "pwa.title.desktop" => "Как установить:",
        "pwa.sysbrowser.text" => "re:Norma лучше всего работает в системном браузере.",
        "pwa.sysbrowser.open" => "Открыть в системном браузере",
        "pwa.sysbrowser.stay" => "Я хочу продолжать использовать в этом браузере",
        "pwa.mi.title" => "Приложение re:Norma работает в браузере Chrome.",
        "pwa.mi.open" => "Открыть в Chrome",
        "pwa.unknown.title" => "Мы не умеем работать с этим браузером.",
        "pwa.unknown.signal" => "Наша команда разработки получила сигнал, что вы пытались воспользоваться нашим приложением с этим браузером, и мы попробуем с этим что-то сделать.",
        "pwa.unknown.chrome" => "Лучше всего если вы откроете приложение в браузере Chrome.",
        "pwa.unknown.safari" => "Лучше всего если вы откроете приложение в браузере Safari.",
        "pwa.unknown.step1" => "Скопируйте этот адрес — нажмите на него.",
        "pwa.unknown.step2" => "Запустите Chrome.",
        "pwa.unknown.step2_safari" => "Запустите Safari.",
        "pwa.unknown.step3" => "Введите адрес в строку поиска.",
        "pwa.unknown.copied" => "Адрес скопирован",
        "pwa.yandex.title" => "re:Norma лучше всего работает в браузере Chrome",
        "pwa.yandex.lead" => "Можно использовать его в Яндекс браузере. Но это неудобно",
        "pwa.yandex.step1" => "Для того чтобы открыть его в Chrome, нажмите на вот эту кнопочку внизу.",
        "pwa.yandex.step2" => "Затем вы выберете браузер",
        "pwa.desktop.mobile_first" => "Приложение предназначено для мобильных устройств.",
        "pwa.desktop.if_phone" => "Если вы открываете это на телефоне, значит у вас включена «Версия для ПК». Уберите эту галочку в меню браузера.",
        "pwa.desktop.if_desktop" => "Если вы хотите пользоваться приложением на компьютере — нажмите кнопку ниже.",
        "pwa.use_browser" => "Использовать в браузере на Desktop",

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

#[cfg(test)]
mod tests {
    use super::{en, ru};

    /// Строки кураторского пути обязаны быть в ОБЕИХ таблицах.
    ///
    /// Их около сорока, вписаны они руками в два далеко разнесённых `match`, и
    /// пропажа в одной таблице означала бы «???» ровно у половины людей — причём
    /// в текстах, которые человек видит в момент, когда решает, пускать ли к себе
    /// куратора.
    #[test]
    fn kuratorskie_stroki_est_v_oboih_tablicah() {
        for key in [
            // Согласие на куратора
            "curator.invite.ask",
            "curator.invite.explain",
            "curator.invite.replaces",
            "curator.invite.accept",
            "curator.invite.decline",
            "curator.invite.done",
            "curator.invite.done_body",
            "curator.invite.dead_title",
            "curator.invite.dead_body",
            "curator.invite.need_app_title",
            "curator.invite.need_app_body",
            "curator.invite.need_app_cta",
            "curator.invite.to_app",
            "curator.invite.failed",
            // Виджет отчёта
            "curator.report.title",
            "curator.report.your_curator",
            "curator.report.requested",
            "curator.report.last_sent",
            "curator.report.never_sent",
            "curator.report.send", "curator.report.what", "curator.report.only_new",
            "curator.report.everything", "curator.report.through_hint",
            "curator.report.unbind_hint",
            "curator.report.unbind",
            "curator.report_sent",
            // Директивы: названия планок, единицы, плашки и письма
            "planka.name.calories",
            "planka.name.protein",
            "planka.name.steps",
            "planka.name.veg_fruit",
            "planka.name.calcium",
            "planka.name.fiber",
            "planka.name.iron",
            "planka.name.heme",
            "planka.name.epa_dha",
            "planka.name.fat_ratio",
            "planka.name.red_meat",
            "planka.name.egg",
            "planka.unit.kcal",
            "planka.unit.g",
            "planka.unit.mg",
            "planka.unit.steps",
            "planka.unit.portions",
            "planka.unit.pieces",
            "curator.note.planka_set",
            "curator.note.week_open",
            "curator.note.week_open_plain",
            "curator.letter.planka_set",
            "curator.letter.unbound",
            "curator.letter.unbound_list",
            "curator.letter.week_open",
            "curator.week.activity",
            "curator.week.calcium",
            "curator.week.iron",
            "curator.week.fats",
            "curator.week.red_meat",
            // Чат и почта
            "chat.peer_support",
            "chat.peer_curator",
            "letters.recompute_now",
        ] {
            assert_ne!(ru(key), "???", "нет русской строки для {key}");
            assert_ne!(en(key), "???", "нет английской строки для {key}");
        }
    }

    /// Подстановки в шаблонах обязаны совпадать: русский текст без `{value}`
    /// молча потерял бы число, ради которого письмо и писалось.
    #[test]
    fn podstanovki_sovpadayut_v_oboih_yazykah() {
        for (key, marks) in [
            ("curator.invite.ask", &["{name}"][..]),
            ("curator.invite.done", &["{name}"][..]),
            ("curator.note.planka_set", &["{what}", "{value}"][..]),
            ("curator.note.week_open", &["{what}"][..]),
            ("curator.letter.planka_set", &["{what}", "{value}"][..]),
            ("curator.letter.week_open", &["{what}"][..]),
            ("curator.report.last_sent", &["{date}"][..]),
            ("curator.report.only_new_hint", &["{date}"][..]),
        ] {
            for m in marks {
                assert!(ru(key).contains(m), "в русском {key} нет подстановки {m}");
                assert!(en(key).contains(m), "в английском {key} нет подстановки {m}");
            }
        }
    }

    /// Каждый ключ, который просит приложение, обязан существовать.
    ///
    /// Иначе он молча выходит на экран как «???» — именно так на кнопку сохранения
    /// в правке ленивой записи уехал несуществующий `common.save`, и заметили это
    /// только на снимке. Проверка идёт по исходникам: сами вызовы `t("…")` и есть
    /// список того, что должно быть переведено.
    #[test]
    fn vse_kljuchi_iz_koda_perevedeny() {
        use std::path::PathBuf;
        fn walk(dir: &PathBuf, out: &mut Vec<PathBuf>) {
            let Ok(rd) = std::fs::read_dir(dir) else { return };
            for e in rd.flatten() {
                let p = e.path();
                if p.is_dir() {
                    walk(&p, out);
                } else if p.extension().is_some_and(|x| x == "rs") {
                    out.push(p);
                }
            }
        }
        let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("src");
        let mut files = Vec::new();
        walk(&root, &mut files);
        assert!(files.len() > 10, "исходников не нашлось — проверка бесполезна");

        let mut missing: Vec<String> = Vec::new();
        for f in &files {
            let Ok(text) = std::fs::read_to_string(f) else { continue };
            for (i, _) in text.match_indices("t(\"") {
                let rest = &text[i + 3..];
                let Some(end) = rest.find('"') else { continue };
                let key = &rest[..end];
                // Только ключи с точкой: `t("…")` встречается и с переменной внутри.
                if !key.contains('.') || !key.chars().all(|c| c.is_ascii_alphanumeric() || c == '.' || c == '_') {
                    continue;
                }
                if ru(key) == "???" || en(key) == "???" {
                    let rel = f.strip_prefix(&root).unwrap_or(f).display();
                    let line = text[..i].matches('\n').count() + 1;
                    missing.push(format!("{key} ({rel}:{line})"));
                }
            }
        }
        missing.sort();
        missing.dedup();
        assert!(missing.is_empty(), "нет перевода у ключей:\n  {}", missing.join("\n  "));
    }
}
