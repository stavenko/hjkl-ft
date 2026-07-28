//! Program "letters" — in-app notifications the user reads at leisure. The first
//! (and currently only) producer is the WEEKLY calorie-planka recompute: one week
//! after the planka was set (and every week thereafter), the planka is recomputed
//! from the last 7 days and a letter announces the new value.
//!
//! Storage: a JSON blob in `app_flags` (per-user, per-device — NOT synced, like the
//! stories seen-set). A root signal drives the dashboard's mail widget reactively.

use std::cell::RefCell;

use leptos::{create_rw_signal, RwSignal, SignalUpdate};
use serde::{Deserialize, Serialize};

use crate::services::app_flags;

/// JSON array of [`Letter`] in `app_flags`.
const LETTERS_KEY: &str = "letters_v1";
/// Date (YYYY-MM-DD) of the last weekly planka recompute. Seeded from the calorie
/// goal's creation date so the first letter arrives one week after the planka was set.
const PLANKA_ANCHOR_KEY: &str = "planka_weekly_anchor";

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct Letter {
    pub id: String,
    /// ISO datetime the letter was created.
    pub created_at: String,
    /// Pre-rendered body (Russian). Kept as text so display needs no recomputation.
    pub body: String,
    #[serde(default)]
    pub read: bool,
}

thread_local! {
    /// Bumped whenever a letter is added or read, so the mail widget re-renders.
    static VERSION: RefCell<Option<RwSignal<u32>>> = const { RefCell::new(None) };
}

/// Create the root reactivity signal. Call once, at root scope, from `main()`.
pub fn init() {
    VERSION.with(|c| *c.borrow_mut() = Some(create_rw_signal(0u32)));
}

/// The root signal the mail widget subscribes to. Bumps whenever letters change.
pub fn version_signal() -> RwSignal<u32> {
    VERSION.with(|c| c.borrow().expect("letters::init() must run first"))
}

/// Record that the calorie planka was (re)computed today — restarts the weekly
/// clock. Called from [`crate::services::local::set_calorie_goal`], so EVERY planka
/// change resets the anchor: the manual «Пересчитать» button (e.g. after a course-
/// goal change) AND the automatic weekly path (which also goes through set_calorie_goal).
pub fn mark_planka_recomputed() {
    let today = chrono::Local::now().date_naive();
    app_flags::set(PLANKA_ANCHOR_KEY, &today.format("%Y-%m-%d").to_string());
}

fn bump() {
    VERSION.with(|c| {
        if let Some(s) = *c.borrow() {
            s.update(|v| *v += 1);
        }
    });
}

/// All letters, newest first.
pub fn all() -> Vec<Letter> {
    let mut v: Vec<Letter> = app_flags::get(LETTERS_KEY)
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default();
    v.sort_by(|a, b| b.created_at.cmp(&a.created_at));
    v
}

fn save(list: &[Letter]) {
    let s = serde_json::to_string(list).expect("serialize letters");
    app_flags::set(LETTERS_KEY, &s);
    bump();
}

pub fn unread_count() -> usize {
    all().iter().filter(|l| !l.read).count()
}

pub fn has_unread() -> bool {
    unread_count() > 0
}

/// Append a letter (deduped by id) and persist.
pub fn add(letter: Letter) {
    let mut list = all();
    if list.iter().any(|l| l.id == letter.id) {
        return;
    }
    list.push(letter);
    save(&list);
}

/// Mark every letter read (called when the inbox is opened). Clears the red dot.
pub fn mark_all_read() {
    let mut list = all();
    let mut changed = false;
    for l in list.iter_mut() {
        if !l.read {
            l.read = true;
            changed = true;
        }
    }
    if changed {
        save(&list);
    }
}

// ── Weekly calorie-planka recompute ──────────────────────────────────────────

/// One week after the planka was set (and weekly thereafter), recompute it and post
/// a letter. Safe to call on every launch/resume — it self-limits via the anchor.
pub async fn maybe_recompute_weekly_planka() {
    use crate::services::local;
    use crate::services::weight_trend::{self, DEFAULT_WINDOW_DAYS};

    // No planka yet → nothing to recompute (the week-2 gate hasn't fired).
    let goals = local::list_goals().await;
    let Some(goal) = goals.iter().find(|g| {
        g.nutrient == "Calories"
            && g.direction == api_types::GoalDirection::AtMost
            && g.amount > 0.0
    }) else {
        return;
    };

    let today = chrono::Local::now().date_naive();
    // Anchor: last recompute, else the planka's creation date (first cycle = +7d).
    let anchor = app_flags::get(PLANKA_ANCHOR_KEY)
        .and_then(|s| chrono::NaiveDate::parse_from_str(&s, "%Y-%m-%d").ok())
        .or_else(|| {
            goal.created_at
                .get(0..10)
                .and_then(|d| chrono::NaiveDate::parse_from_str(d, "%Y-%m-%d").ok())
        })
        .unwrap_or(today);

    if (today - anchor).num_days() < 7 {
        return;
    }

    // Require recent logging to justify a recompute at all — if the user hasn't
    // logged any food this week, defer WITHOUT advancing the anchor (retry next
    // launch). We no longer use the average as the planka base, only as an
    // activity gate.
    if local::avg_daily_kcal(7).await.is_none() {
        return;
    }

    // Base the new planka on the PREVIOUS planka (`goal.amount`), nudged by AT MOST
    // ±5% from the weight trend — NOT on raw average intake. This keeps the planka
    // moving at most one small step per week and stops a low-intake week (e.g.
    // anxiety undereating) from ratcheting the target downward: eating under the
    // planka no longer drags the base down, only a confirmed weight trend moves it.
    // Average intake seeds only the FIRST planka (`calorie_planka_suggestion`).
    let entries = local::list_weight_entries().await;
    let weight_kg = entries
        .iter()
        .max_by(|a, b| a.date.cmp(&b.date))
        .map(|e| e.weight_kg)
        .unwrap_or(0.0);
    let trend = weight_trend::weight_trend(&entries, DEFAULT_WINDOW_DAYS);
    let new_planka = local::calorie_planka(goal.amount, &trend, weight_kg);

    let avg_steps = local::avg_steps_last_days(7).await;

    // Apply the new planka (syncs like any goal edit). `set_calorie_goal` also
    // advances the weekly anchor to today via `mark_planka_recomputed`, so the next
    // letter is a full week out — no separate anchor write needed here.
    local::set_calorie_goal(new_planka).await;
    crate::services::sync::push_background();

    // Post the letter announcing the new planka.
    add(Letter {
        id: format!("planka-{}", today.format("%Y-%m-%d")),
        created_at: chrono::Local::now().to_rfc3339(),
        body: planka_letter_body(avg_steps, &trend, new_planka),
        read: false,
    });
}

fn planka_letter_body(
    avg_steps: Option<u32>,
    trend: &crate::services::weight_trend::WeightTrend,
    planka: f64,
) -> String {
    let steps = match avg_steps {
        Some(n) => format!("{} {}", fmt_thousands(n), plural_steps(n)),
        None => "нет данных".to_string(),
    };
    let trend_s = weight_trend_phrase(trend);
    format!(
        "Прошла очередная неделя, и мы выдаём вам новую планку по калориям:\n\n\
         Ваша активность: {steps}\n\
         Ваша динамика веса: {trend_s}\n\n\
         Ваша новая планка по калориям: {planka} ккал\n\n\
         Это письмо — это просто уведомление.",
        planka = planka as i64,
    )
}

/// A short Russian phrase for the detected weight trend.
fn weight_trend_phrase(t: &crate::services::weight_trend::WeightTrend) -> &'static str {
    use crate::services::weight_trend::{Direction, WeightTrend, CONFIDENT, WEAK};
    match *t {
        WeightTrend::Insufficient { .. } => "недостаточно данных",
        WeightTrend::Tentative { direction, .. } => match direction {
            Direction::Down => "скорее снижается",
            Direction::Up => "скорее растёт",
        },
        WeightTrend::Estimated { direction, confidence, .. } => {
            if confidence >= CONFIDENT {
                match direction {
                    Direction::Down => "снижается",
                    Direction::Up => "растёт",
                }
            } else if confidence >= WEAK {
                match direction {
                    Direction::Down => "скорее снижается",
                    Direction::Up => "скорее растёт",
                }
            } else {
                "стабилен"
            }
        }
    }
}

/// Group thousands with a thin space: 8200 → "8 200".
fn fmt_thousands(n: u32) -> String {
    let s = n.to_string();
    let bytes = s.as_bytes();
    let mut out = String::new();
    for (i, b) in bytes.iter().enumerate() {
        if i > 0 && (bytes.len() - i) % 3 == 0 {
            out.push('\u{202f}'); // narrow no-break space
        }
        out.push(*b as char);
    }
    out
}

/// Russian plural for «шаг»: 1 шаг / 2 шага / 5 шагов.
fn plural_steps(n: u32) -> &'static str {
    let n100 = n % 100;
    let n10 = n % 10;
    if (11..=14).contains(&n100) {
        "шагов"
    } else if n10 == 1 {
        "шаг"
    } else if (2..=4).contains(&n10) {
        "шага"
    } else {
        "шагов"
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn thousands_grouping() {
        assert_eq!(fmt_thousands(0), "0");
        assert_eq!(fmt_thousands(200), "200");
        assert_eq!(fmt_thousands(8200), "8\u{202f}200");
        assert_eq!(fmt_thousands(12345), "12\u{202f}345");
    }

    #[test]
    fn steps_plural() {
        assert_eq!(plural_steps(1), "шаг");
        assert_eq!(plural_steps(2), "шага");
        assert_eq!(plural_steps(4), "шага");
        assert_eq!(plural_steps(5), "шагов");
        assert_eq!(plural_steps(11), "шагов");
        assert_eq!(plural_steps(21), "шаг");
        assert_eq!(plural_steps(8200), "шагов");
    }
}
