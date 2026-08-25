//! Тексты кураторских директив — на языке ЧЕЛОВЕКА, а не отправителя.
//!
//! Директива несёт только данные: какой индикатор, какое число, стоит ли запрет.
//! Сам текст — и системная плашка в чате, и письмо в почтовый ящик — собирается
//! здесь, из строк приложения. Иначе язык переписки зависел бы от настроек
//! куратора: он выставил себе английский, а человек с русским приложением
//! получил бы английскую плашку.
//!
//! Поле `text` в сообщении остаётся как запасной вариант для сборок, которые
//! этого ещё не умеют, но ни одна из функций ниже на него не смотрит.

use crate::services::i18n::t;

/// Название индикатора на языке приложения.
pub fn planka_name(key: &str) -> String {
    let k = match key {
        "calories" => "planka.name.calories",
        "protein" => "planka.name.protein",
        "steps" => "planka.name.steps",
        "veg_fruit" => "planka.name.veg_fruit",
        "calcium" => "planka.name.calcium",
        "fiber" => "planka.name.fiber",
        "iron" => "planka.name.iron",
        "heme" => "planka.name.heme",
        "epa_dha" => "planka.name.epa_dha",
        "fat_ratio" => "planka.name.fat_ratio",
        "red_meat" => "planka.name.red_meat",
        "egg" => "planka.name.egg",
        _ => return key.to_string(),
    };
    t(k).to_string()
}

/// Единица измерения планки — та же, в которой её видит человек на шкале.
pub fn planka_unit(key: &str) -> String {
    let k = match key {
        "calories" => "planka.unit.kcal",
        "protein" | "veg_fruit" | "fiber" | "red_meat" => "planka.unit.g",
        "steps" => "planka.unit.steps",
        "calcium" | "iron" => "planka.unit.mg",
        "epa_dha" => "planka.unit.g",
        "heme" => "planka.unit.portions",
        "egg" => "planka.unit.pieces",
        // Отношение ненасыщенных к насыщенным — величина безразмерная.
        _ => return String::new(),
    };
    t(k).to_string()
}

/// Сколько знаков после запятой осмысленно у этой планки. Отношение жиров и
/// EPA+DHA — дробные, всё остальное считается целыми.
fn decimals(key: &str) -> usize {
    match key {
        "fat_ratio" | "epa_dha" | "heme" => 1,
        _ => 0,
    }
}

/// Число планки в её единицах, как его увидит человек.
pub fn planka_value(key: &str, amount: f64) -> String {
    let unit = planka_unit(key);
    let n = format!("{amount:.*}", decimals(key));
    if unit.is_empty() {
        n
    } else {
        format!("{n} {unit}")
    }
}

/// Системная плашка в чате: «Куратор установил планку …».
pub fn set_planka_note(key: &str, amount: Option<f64>) -> String {
    match amount {
        Some(a) => t("curator.note.planka_set")
            .replace("{what}", &planka_name(key))
            .replace("{value}", &planka_value(key, a)),
        None => t("curator.note.planka_changed").replace("{what}", &planka_name(key)),
    }
}

/// Системная плашка про запрет или разрешение автопересчёта.
pub fn lock_note(key: &str, locked: bool) -> String {
    let k = if locked { "curator.note.lock_on" } else { "curator.note.lock_off" };
    t(k).replace("{what}", &planka_name(key))
}

/// Письмо в почтовый ящик про новую планку.
pub fn set_planka_letter(key: &str, amount: f64) -> String {
    t("curator.letter.planka_set")
        .replace("{what}", &planka_name(key))
        .replace("{value}", &planka_value(key, amount))
}

/// Письмо про запрет/разрешение автопересчёта.
pub fn lock_letter(key: &str, locked: bool) -> String {
    let k = if locked { "curator.letter.lock_on" } else { "curator.letter.lock_off" };
    t(k).replace("{what}", &planka_name(key))
}

/// Ключ строки для темы по её номеру — тому же, что человек видит в ленте
/// историй. Отдельно от перевода, чтобы таблицу номеров можно было проверить
/// тестом: `t()` требует поднятого сигнала языка и вне приложения не работает.
pub fn week_key(week: u32) -> Option<&'static str> {
    match week {
        3 => Some("curator.week.activity"),
        4 => Some("curator.week.calcium"),
        5 => Some("curator.week.iron"),
        6 => Some("curator.week.fats"),
        7 => Some("curator.week.red_meat"),
        _ => None,
    }
}

/// Название темы на языке приложения.
pub fn week_name(week: u32) -> Option<String> {
    week_key(week).map(|k| t(k).to_string())
}

/// Системная плашка про открытую тему.
pub fn open_week_note(week: Option<u32>) -> String {
    match week.and_then(week_name) {
        Some(name) => t("curator.note.week_open").replace("{what}", &name),
        None => t("curator.note.week_open_plain").to_string(),
    }
}

/// Письмо про открытую тему.
pub fn open_week_letter(week: u32) -> String {
    let name = week_name(week).unwrap_or_default();
    t("curator.letter.week_open").replace("{what}", &name)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn chislo_planki_v_ee_edinicah() {
        // Единицы у каждой планки свои, и число округляется так, как его читают.
        assert_eq!(decimals("calories"), 0);
        assert_eq!(decimals("fat_ratio"), 1);
        assert_eq!(decimals("steps"), 0);
    }

    #[test]
    fn nomera_tem_te_zhe_chto_v_lente() {
        // Номера — те же, по которым темы открываются директивой open_week.
        for w in 3..=7 {
            assert!(week_key(w).is_some(), "тема {w} должна называться");
        }
        assert!(week_key(2).is_none());
        assert!(week_key(8).is_none());
    }
}
