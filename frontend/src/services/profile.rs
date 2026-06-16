//! Lightweight user-profile settings kept in localStorage. Currently just the
//! biological sex, collected to (1) soften some nutrient targets for women and
//! (2) detect cycle-related weight deviations — both planned; this only stores
//! the value and marks the story task.

const KEY_SEX: &str = "profile_sex";

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Sex {
    Male,
    Female,
}

fn storage() -> Option<web_sys::Storage> {
    web_sys::window().and_then(|w| w.local_storage().ok().flatten())
}

pub fn get_sex() -> Option<Sex> {
    let v = storage()?.get_item(KEY_SEX).ok().flatten()?;
    match v.as_str() {
        "male" => Some(Sex::Male),
        "female" => Some(Sex::Female),
        _ => None,
    }
}

pub fn set_sex(sex: Sex) {
    if let Some(s) = storage() {
        let v = match sex {
            Sex::Male => "male",
            Sex::Female => "female",
        };
        s.set_item(KEY_SEX, v).expect("write sex");
    }
}
