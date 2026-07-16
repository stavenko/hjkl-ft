//! Pure meal-grouping algorithm ported from munch-monitor.
//!
//! Given the diary entries for a single day, derive which "meal" each entry
//! belongs to (the meal is DERIVED from clock time + gaps, NOT from the stored
//! `meal_label`), and group them.
//!
//! This module is intentionally free of `web_sys` so it can be unit-tested on
//! the host. `chrono` is used only for parsing the RFC3339 `created_at`
//! timestamps used for gap computation.

use chrono::{DateTime, FixedOffset, Timelike};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MealType {
    Breakfast,
    PreLunchSnack,
    Lunch,
    PreDinnerSnack,
    Dinner,
    NightSnack,
}

impl MealType {
    pub fn sort_order(self) -> u8 {
        match self {
            MealType::Breakfast => 0,
            MealType::PreLunchSnack => 1,
            MealType::Lunch => 2,
            MealType::PreDinnerSnack => 3,
            MealType::Dinner => 4,
            MealType::NightSnack => 5,
        }
    }

    pub fn i18n_key(self) -> &'static str {
        match self {
            MealType::Breakfast => "meal.breakfast",
            MealType::PreLunchSnack => "meal.snack_morning",
            MealType::Lunch => "meal.lunch",
            MealType::PreDinnerSnack => "meal.snack_afternoon",
            MealType::Dinner => "meal.dinner",
            MealType::NightSnack => "meal.snack_night",
        }
    }

    /// Muted per-meal accent colour (6-digit `#rrggbb`), used to tint the diary
    /// panel border + header so each meal reads distinctly. Mid-tone shades so
    /// they stay legible on both light and dark schemes; the three snack buckets
    /// share one neutral slate.
    pub fn accent(self) -> &'static str {
        match self {
            MealType::Breakfast => "#D99A2B",  // warm amber — morning
            MealType::Lunch => "#4FA96A",      // green — midday
            MealType::Dinner => "#6C7DC4",     // indigo — evening
            MealType::PreLunchSnack | MealType::PreDinnerSnack | MealType::NightSnack => "#8A97A6", // slate — snacks
        }
    }
}

pub struct MealGroup {
    pub meal: MealType,
    pub entries: Vec<api_types::DiaryEntry>,
}

// Gap thresholds (seconds) replicated from munch-monitor.
const CONTINUATION_SECS: i64 = 3600; // < 1h => continuation of previous meal
const NEW_MEAL_SECS: i64 = 10800; // > 3h => possibly a new major meal

/// Classify a wall-clock (hour, minute) into a base meal type using the
/// time windows, with the munch-monitor fallback for times outside all
/// windows.
///
/// Windows (local). Aligned with the 04:00 logical day boundary
/// (see `local::DAY_START_HOUR`): a day runs 04:00→03:59, so the earliest entry
/// of a day is a morning one, and "night" only covers the late 22:00–03:59 tail.
///   Breakfast 04:00–10:59
///   Lunch     11:00–15:59
///   Dinner    16:00–21:59
///   Night     22:00–03:59 (wraps midnight)
fn determine_meal_type(hour: u32, _minute: u32) -> MealType {
    // First window that contains the time.
    if (4..=10).contains(&hour) {
        MealType::Breakfast
    } else if (11..=15).contains(&hour) {
        MealType::Lunch
    } else if (16..=21).contains(&hour) {
        MealType::Dinner
    } else if hour >= 22 || hour < 4 {
        MealType::NightSnack
    } else {
        // Outside all windows fallback (unreachable given windows cover all
        // hours, but kept faithful to the source algorithm).
        if hour < 11 {
            MealType::Breakfast
        } else if hour < 16 {
            MealType::Lunch
        } else {
            MealType::Dinner
        }
    }
}

/// Snack bucket derived from the previous major meal.
fn snack_type(prev: Option<MealType>) -> MealType {
    match prev {
        None | Some(MealType::Breakfast) => MealType::PreLunchSnack,
        Some(MealType::Lunch) => MealType::PreDinnerSnack,
        Some(MealType::Dinner) | Some(MealType::PreDinnerSnack) => MealType::NightSnack,
        _ => MealType::NightSnack,
    }
}

/// Parse the local (hour, minute) for an entry.
///
/// Prefers the local `time` "HH:MM" field when present and valid; otherwise
/// falls back to the hour/minute of the (UTC) `created_at` RFC3339 timestamp.
fn local_hm(entry: &api_types::DiaryEntry) -> (u32, u32) {
    if let Some(t) = &entry.time {
        if let Some((h, m)) = parse_hhmm(t) {
            return (h, m);
        }
    }
    if let Some(dt) = parse_rfc3339(&entry.created_at) {
        return (dt.hour(), dt.minute());
    }
    // No usable clock at all: treat as start of day. determine_meal_type will
    // bucket this as NightSnack (hour 0), matching the "unknown" fallback.
    (0, 0)
}

fn parse_hhmm(s: &str) -> Option<(u32, u32)> {
    let (h_str, m_str) = s.split_once(':')?;
    let h: u32 = h_str.trim().parse().ok()?;
    let m: u32 = m_str.trim().parse().ok()?;
    if h < 24 && m < 60 {
        Some((h, m))
    } else {
        None
    }
}

fn parse_rfc3339(s: &str) -> Option<DateTime<FixedOffset>> {
    DateTime::parse_from_rfc3339(s).ok()
}

/// Group the day's diary entries into derived meals.
///
/// Returns groups sorted by [`MealType::sort_order`], with empty groups
/// omitted, and entries within each group kept in chronological order.
pub fn group_by_meal(entries: &[api_types::DiaryEntry]) -> Vec<MealGroup> {
    if entries.is_empty() {
        return Vec::new();
    }

    // Build (sorted_index, timestamp, entry) tuples. The timestamp is the
    // parsed created_at; entries that don't parse get None and are ordered by
    // their incoming index (and created_at string tie-break below).
    let mut ordered: Vec<(usize, Option<i64>, &api_types::DiaryEntry)> = entries
        .iter()
        .enumerate()
        .map(|(i, e)| (i, parse_rfc3339(&e.created_at).map(|d| d.timestamp()), e))
        .collect();

    // Stable sort ascending by timestamp; unparseable timestamps (None) sort
    // last. Tie-break by the created_at string for determinism.
    ordered.sort_by(|a, b| {
        let key_a = (a.1.is_none(), a.1.unwrap_or(0), a.2.created_at.as_str());
        let key_b = (b.1.is_none(), b.1.unwrap_or(0), b.2.created_at.as_str());
        key_a.cmp(&key_b)
    });

    // Per-entry bucket assignment.
    let mut buckets: Vec<MealType> = Vec::with_capacity(ordered.len());

    let mut previous_meal: Option<MealType> = None;
    let mut first_ts: Option<i64> = None; // timestamp of first entry of the current major-meal bucket

    for (idx, (_orig_idx, ts, entry)) in ordered.iter().enumerate() {
        let (hour, minute) = local_hm(entry);

        if idx == 0 {
            let m = determine_meal_type(hour, minute);
            buckets.push(m);
            previous_meal = Some(m);
            first_ts = *ts;
            continue;
        }

        // Gap from the first entry of the previous major meal.
        let gap: Option<i64> = match (ts, first_ts) {
            (Some(cur), Some(base)) => Some(cur - base),
            // Unknown timestamp on either side: treat the gap as large/unknown
            // and reclassify by the clock.
            _ => None,
        };

        match gap {
            Some(g) if g < CONTINUATION_SECS => {
                // Continuation of the previous meal.
                let m = previous_meal.unwrap_or_else(|| determine_meal_type(hour, minute));
                buckets.push(m);
            }
            Some(g) if g > NEW_MEAL_SECS => {
                let m = determine_meal_type(hour, minute);
                if m == MealType::Lunch || m == MealType::Dinner {
                    buckets.push(m);
                    previous_meal = Some(m);
                    first_ts = *ts;
                } else {
                    buckets.push(snack_type(previous_meal));
                }
            }
            Some(_) => {
                // 1h..=3h gap: snack of the current chain.
                buckets.push(snack_type(previous_meal));
            }
            None => {
                // Unknown gap -> reclassify by clock.
                let m = determine_meal_type(hour, minute);
                if m == MealType::Lunch || m == MealType::Dinner {
                    buckets.push(m);
                    previous_meal = Some(m);
                    first_ts = *ts;
                } else {
                    buckets.push(snack_type(previous_meal));
                }
            }
        }
    }

    // Collect into groups, preserving chronological (sorted) order within each.
    let mut groups: Vec<MealGroup> = Vec::new();
    for (idx, (_orig_idx, _ts, entry)) in ordered.iter().enumerate() {
        let meal = buckets[idx];
        if let Some(g) = groups.iter_mut().find(|g| g.meal == meal) {
            g.entries.push((*entry).clone());
        } else {
            groups.push(MealGroup {
                meal,
                entries: vec![(*entry).clone()],
            });
        }
    }

    groups.sort_by_key(|g| g.meal.sort_order());
    groups
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(id: &str, time: Option<&str>, created_at: &str) -> api_types::DiaryEntry {
        api_types::DiaryEntry {
            id: id.to_string(),
            food_id: format!("food-{id}"),
            date: "2026-06-05".to_string(),
            time: time.map(|s| s.to_string()),
            grams: 100.0,
            waste_grams: 0.0,
            meal_label: None,
            deleted: false,
            created_at: created_at.to_string(),
            updated_at: created_at.to_string(),
        }
    }

    // created_at strings far apart so gaps never accidentally continue.
    fn ts(hhmm: &str) -> String {
        format!("2026-06-05T{hhmm}:00+00:00")
    }

    fn solo(time: &str) -> Vec<api_types::DiaryEntry> {
        vec![entry("e", Some(time), &ts(time))]
    }

    fn classify_solo(time: &str) -> MealType {
        let g = group_by_meal(&solo(time));
        assert_eq!(g.len(), 1);
        g[0].meal
    }

    #[test]
    fn only_dinner_first_entry_is_one_dinner_group() {
        // Brand-new user logs a single evening meal — exactly one Dinner group.
        let g = group_by_meal(&solo("19:30"));
        assert_eq!(g.len(), 1);
        assert_eq!(g[0].meal, MealType::Dinner);
        assert_eq!(g[0].entries.len(), 1);
        // A second dinner-time entry within the hour stays in the same Dinner group.
        let two = vec![entry("a", Some("19:00"), &ts("19:00")), entry("b", Some("19:40"), &ts("19:40"))];
        let g2 = group_by_meal(&two);
        assert_eq!(g2.len(), 1);
        assert_eq!(g2[0].meal, MealType::Dinner);
        assert_eq!(g2[0].entries.len(), 2);
    }

    #[test]
    fn window_edges_pure_clock() {
        // Windows align with the 04:00 day boundary: breakfast starts at 04:00,
        // night only covers the 22:00–03:59 tail.
        assert_eq!(classify_solo("04:00"), MealType::Breakfast);
        assert_eq!(classify_solo("04:59"), MealType::Breakfast);
        assert_eq!(classify_solo("05:00"), MealType::Breakfast);
        assert_eq!(classify_solo("10:59"), MealType::Breakfast);
        assert_eq!(classify_solo("11:00"), MealType::Lunch);
        assert_eq!(classify_solo("15:59"), MealType::Lunch);
        assert_eq!(classify_solo("16:00"), MealType::Dinner);
        assert_eq!(classify_solo("21:59"), MealType::Dinner);
        assert_eq!(classify_solo("22:00"), MealType::NightSnack);
        assert_eq!(classify_solo("23:30"), MealType::NightSnack);
        assert_eq!(classify_solo("03:00"), MealType::NightSnack);
        assert_eq!(classify_solo("03:59"), MealType::NightSnack);
    }

    #[test]
    fn early_morning_start_not_swallowed_into_night() {
        // Reported bug: a very early entry was classified NightSnack, which then
        // swallowed the following morning meals into NightSnack too. With
        // breakfast starting at 04:00 an 04:30 entry is Breakfast, and nothing
        // in the day lands in a night-snack bucket.
        let entries = vec![
            entry("a", Some("04:30"), &ts("04:30")),
            entry("b", Some("08:00"), &ts("08:00")),
        ];
        let g = group_by_meal(&entries);
        assert_eq!(g[0].meal, MealType::Breakfast);
        assert!(
            g.iter().all(|grp| grp.meal != MealType::NightSnack),
            "nothing should be night snack, got {:?}",
            g.iter().map(|x| x.meal).collect::<Vec<_>>()
        );
    }

    #[test]
    fn fallback_hours() {
        // 00:00 -> hour 0 -> night window.
        assert_eq!(classify_solo("00:00"), MealType::NightSnack);
        // determine_meal_type fallback path is covered by the window logic;
        // verify a couple of representative hours.
        assert_eq!(determine_meal_type(7, 0), MealType::Breakfast);
        assert_eq!(determine_meal_type(13, 0), MealType::Lunch);
        assert_eq!(determine_meal_type(19, 0), MealType::Dinner);
    }

    #[test]
    fn empty_input() {
        let g = group_by_meal(&[]);
        assert!(g.is_empty());
    }

    #[test]
    fn continuation_within_1h() {
        // Breakfast at 08:00, second entry 30 min later -> same Breakfast bucket.
        let entries = vec![
            entry("a", Some("08:00"), &ts("08:00")),
            entry("b", Some("08:30"), &ts("08:30")),
        ];
        let g = group_by_meal(&entries);
        assert_eq!(g.len(), 1);
        assert_eq!(g[0].meal, MealType::Breakfast);
        assert_eq!(g[0].entries.len(), 2);
        // chronological order preserved
        assert_eq!(g[0].entries[0].id, "a");
        assert_eq!(g[0].entries[1].id, "b");
    }

    #[test]
    fn over_3h_to_lunch_starts_new_major_meal() {
        // Breakfast 07:00, then 12:00 (5h later) at a lunch hour -> new Lunch meal.
        let entries = vec![
            entry("a", Some("07:00"), &ts("07:00")),
            entry("b", Some("12:00"), &ts("12:00")),
        ];
        let g = group_by_meal(&entries);
        assert_eq!(g.len(), 2);
        assert_eq!(g[0].meal, MealType::Breakfast);
        assert_eq!(g[1].meal, MealType::Lunch);
        assert_eq!(g[0].entries.len(), 1);
        assert_eq!(g[1].entries.len(), 1);
    }

    #[test]
    fn over_3h_to_dinner_starts_new_major_meal() {
        // Lunch 12:00, then 17:00 (5h) at dinner hour -> new Dinner.
        let entries = vec![
            entry("a", Some("12:00"), &ts("12:00")),
            entry("b", Some("17:00"), &ts("17:00")),
        ];
        let g = group_by_meal(&entries);
        assert_eq!(g.len(), 2);
        assert_eq!(g[0].meal, MealType::Lunch);
        assert_eq!(g[1].meal, MealType::Dinner);
    }

    #[test]
    fn over_3h_to_nonmajor_hour_is_snack() {
        // Breakfast 06:00, then 10:00 (4h) -> 10:00 is still a breakfast hour
        // (not Lunch/Dinner) -> snack of breakfast chain = PreLunchSnack.
        let entries = vec![
            entry("a", Some("06:00"), &ts("06:00")),
            entry("b", Some("10:00"), &ts("10:00")),
        ];
        let g = group_by_meal(&entries);
        assert_eq!(g.len(), 2);
        assert_eq!(g[0].meal, MealType::Breakfast);
        assert_eq!(g[1].meal, MealType::PreLunchSnack);
    }

    #[test]
    fn snack_1_to_3h_breakfast_chain_morning() {
        // Breakfast 08:00, then 10:00 (2h later) -> PreLunchSnack (morning).
        let entries = vec![
            entry("a", Some("08:00"), &ts("08:00")),
            entry("b", Some("10:00"), &ts("10:00")),
        ];
        let g = group_by_meal(&entries);
        assert_eq!(g.len(), 2);
        assert_eq!(g[0].meal, MealType::Breakfast);
        assert_eq!(g[1].meal, MealType::PreLunchSnack);
    }

    #[test]
    fn snack_1_to_3h_lunch_chain_afternoon() {
        // Lunch 12:00, then 14:00 (2h) -> PreDinnerSnack (afternoon).
        let entries = vec![
            entry("a", Some("12:00"), &ts("12:00")),
            entry("b", Some("14:00"), &ts("14:00")),
        ];
        let g = group_by_meal(&entries);
        assert_eq!(g.len(), 2);
        assert_eq!(g[0].meal, MealType::Lunch);
        assert_eq!(g[1].meal, MealType::PreDinnerSnack);
    }

    #[test]
    fn snack_1_to_3h_dinner_chain_night() {
        // Dinner 18:00, then 20:00 (2h) -> NightSnack.
        let entries = vec![
            entry("a", Some("18:00"), &ts("18:00")),
            entry("b", Some("20:00"), &ts("20:00")),
        ];
        let g = group_by_meal(&entries);
        assert_eq!(g.len(), 2);
        assert_eq!(g[0].meal, MealType::Dinner);
        assert_eq!(g[1].meal, MealType::NightSnack);
    }

    #[test]
    fn fallback_to_created_at_when_time_missing() {
        // No `time`; created_at hour 13 UTC -> Lunch.
        let entries = vec![entry("a", None, &ts("13:00"))];
        let g = group_by_meal(&entries);
        assert_eq!(g.len(), 1);
        assert_eq!(g[0].meal, MealType::Lunch);
    }

    #[test]
    fn group_ordering_and_empty_group_omission() {
        // Out-of-order input spanning Breakfast, Lunch, Dinner. No snacks.
        let entries = vec![
            entry("dinner", Some("18:00"), &ts("18:00")),
            entry("breakfast", Some("08:00"), &ts("08:00")),
            entry("lunch", Some("12:00"), &ts("12:00")),
        ];
        let g = group_by_meal(&entries);
        // Three non-empty groups, sorted by sort_order; snack groups omitted.
        assert_eq!(g.len(), 3);
        assert_eq!(g[0].meal, MealType::Breakfast);
        assert_eq!(g[1].meal, MealType::Lunch);
        assert_eq!(g[2].meal, MealType::Dinner);
        assert!(g.iter().all(|grp| grp.meal.sort_order() == grp.meal.sort_order()));
        // Verify monotonic sort_order.
        assert!(g.windows(2).all(|w| w[0].meal.sort_order() < w[1].meal.sort_order()));
        // Each entry routed to the right group.
        assert_eq!(g[0].entries[0].id, "breakfast");
        assert_eq!(g[1].entries[0].id, "lunch");
        assert_eq!(g[2].entries[0].id, "dinner");
    }

    #[test]
    fn i18n_keys_and_sort_order() {
        assert_eq!(MealType::Breakfast.i18n_key(), "meal.breakfast");
        assert_eq!(MealType::PreLunchSnack.i18n_key(), "meal.snack_morning");
        assert_eq!(MealType::Lunch.i18n_key(), "meal.lunch");
        assert_eq!(MealType::PreDinnerSnack.i18n_key(), "meal.snack_afternoon");
        assert_eq!(MealType::Dinner.i18n_key(), "meal.dinner");
        assert_eq!(MealType::NightSnack.i18n_key(), "meal.snack_night");

        assert_eq!(MealType::Breakfast.sort_order(), 0);
        assert_eq!(MealType::PreLunchSnack.sort_order(), 1);
        assert_eq!(MealType::Lunch.sort_order(), 2);
        assert_eq!(MealType::PreDinnerSnack.sort_order(), 3);
        assert_eq!(MealType::Dinner.sort_order(), 4);
        assert_eq!(MealType::NightSnack.sort_order(), 5);
    }
}
