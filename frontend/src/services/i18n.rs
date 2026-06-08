use std::cell::Cell;

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Lang {
    En,
    Ru,
}

thread_local! {
    static CURRENT_LANG: Cell<Lang> = const { Cell::new(Lang::Ru) };
}

pub fn set_lang(lang: Lang) {
    CURRENT_LANG.with(|l| l.set(lang));
}

pub fn get_lang() -> Lang {
    CURRENT_LANG.with(|l| l.get())
}

pub fn t(key: &str) -> &'static str {
    match get_lang() {
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

        // Diary: actions
        "diary.delete" => "Delete",
        "diary.repeat_today" => "Repeat today",
        "diary.no_entries" => "No entries for this day",
        "diary.per_week" => "per week",

        // Diary add modal
        "diary_add.title" => "Add to diary",
        "diary_add.search" => "Search",
        "diary_add.new" => "New",
        "diary_add.search_placeholder" => "Search food...",
        "diary_add.done" => "Done",
        "diary_add.how_much" => "How much?",
        "diary_add.add" => "Add",

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

        // Дневник: действия
        "diary.delete" => "Удалить",
        "diary.repeat_today" => "Повторить сегодня",
        "diary.no_entries" => "Нет записей за этот день",
        "diary.per_week" => "в неделю",

        // Модалка добавления в дневник
        "diary_add.title" => "Добавить в дневник",
        "diary_add.search" => "Поиск",
        "diary_add.new" => "Новый",
        "diary_add.search_placeholder" => "Найти продукт...",
        "diary_add.done" => "Готово",
        "diary_add.how_much" => "Сколько?",
        "diary_add.add" => "Добавить",

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

        _ => "???",
    }
}
