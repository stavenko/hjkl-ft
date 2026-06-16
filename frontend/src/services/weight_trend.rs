//! Weight-trend detection: is the user's weight falling or rising, and how sure
//! are we?
//!
//! Method: ordinary least-squares line `weight ~ day` over a trailing window.
//! The slope's sign is the direction; its statistical significance (a Student-t
//! test on the slope, `dof = days − 2`) is the confidence. Deliberately simple:
//!
//! - No measurement-quality weighting — the `morning`/`no_food`/… flags are NOT
//!   used; every measurement counts equally.
//! - No synthetic backfill — sparse or short history simply produces a wide
//!   standard error → confidence near 0.5 ("unclear"), which is the honest answer
//!   rather than a fabricated flat trend.
//! - Multiple weigh-ins on the same day are averaged into one daily value, so a
//!   day is one piece of evidence (`days` drives the degrees of freedom).
//!
//! The window is anchored at the most recent measurement (not the wall clock),
//! keeping the function pure and deterministic.

use std::collections::BTreeMap;

use api_types::WeightEntry;

/// Default trailing window. 14 days balances noise suppression (longer = far
/// smaller slope standard error, since `SE(β) ∝ window^-1.5`) against lag.
pub const DEFAULT_WINDOW_DAYS: i64 = 14;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Direction {
    Down,
    Up,
}

#[derive(Debug, Clone, PartialEq)]
pub enum WeightTrend {
    /// Fewer than 2 distinct measurement days in the window — nothing to report.
    Insufficient { days: usize },
    /// Exactly 2 days: a slope and its sign exist, but with `dof = 0` there is no
    /// confidence. Present as preliminary.
    Tentative {
        direction: Direction,
        slope_kg_per_week: f64,
        days: usize,
    },
    /// ≥3 days: full estimate. `confidence` ∈ [0.5, 1] is the probability the
    /// true slope has the reported sign.
    Estimated {
        direction: Direction,
        slope_kg_per_week: f64,
        confidence: f64,
        days: usize,
    },
}

/// Energy-balance reading derived from the trend, for the weight widget colour:
/// losing = deficit, gaining = surplus, flat/unclear = maintenance.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BalanceState {
    Deficit,
    Surplus,
    Maintenance,
}

/// Confidence at/above which we commit to a direction (deficit/surplus / a named
/// trend). Below it the data is treated as maintenance / "unclear".
pub const CONFIDENT: f64 = 0.8;

impl WeightTrend {
    /// Map to deficit/surplus/maintenance. Only a confident Estimated trend
    /// colours; tentative / insufficient / low-confidence read as maintenance.
    pub fn balance(&self) -> BalanceState {
        match *self {
            WeightTrend::Estimated { direction, confidence, .. } if confidence >= CONFIDENT => {
                match direction {
                    Direction::Down => BalanceState::Deficit,
                    Direction::Up => BalanceState::Surplus,
                }
            }
            _ => BalanceState::Maintenance,
        }
    }
}

/// Estimate the weight trend over the last `window_days` of measurements.
pub fn weight_trend(entries: &[WeightEntry], window_days: i64) -> WeightTrend {
    // Aggregate to one mean weight per calendar day, bounded to the trailing
    // window ending at the latest measurement.
    let mut by_day: BTreeMap<chrono::NaiveDate, (f64, u32)> = BTreeMap::new();
    let mut latest: Option<chrono::NaiveDate> = None;
    for e in entries {
        let Ok(d) = chrono::NaiveDate::parse_from_str(&e.date, "%Y-%m-%d") else {
            continue;
        };
        latest = Some(latest.map_or(d, |l| l.max(d)));
        let slot = by_day.entry(d).or_insert((0.0, 0));
        slot.0 += e.weight_kg;
        slot.1 += 1;
    }

    let Some(latest) = latest else {
        return WeightTrend::Insufficient { days: 0 };
    };
    let window_start = latest - chrono::Duration::days(window_days - 1);

    // (x = day offset from window start, y = mean weight that day)
    let points: Vec<(f64, f64)> = by_day
        .into_iter()
        .filter(|(d, _)| *d >= window_start)
        .map(|(d, (sum, n))| ((d - window_start).num_days() as f64, sum / n as f64))
        .collect();

    let days = points.len();
    if days < 2 {
        return WeightTrend::Insufficient { days };
    }

    let n = days as f64;
    let mean_x = points.iter().map(|(x, _)| x).sum::<f64>() / n;
    let mean_y = points.iter().map(|(_, y)| y).sum::<f64>() / n;
    let s_xx: f64 = points.iter().map(|(x, _)| (x - mean_x).powi(2)).sum();
    let s_xy: f64 = points
        .iter()
        .map(|(x, y)| (x - mean_x) * (y - mean_y))
        .sum();

    // days >= 2 distinct days ⇒ x values differ ⇒ s_xx > 0.
    let beta = s_xy / s_xx;
    let slope_kg_per_week = beta * 7.0;
    let direction = if beta < 0.0 { Direction::Down } else { Direction::Up };

    if days == 2 {
        return WeightTrend::Tentative {
            direction,
            slope_kg_per_week,
            days,
        };
    }

    let alpha = mean_y - beta * mean_x;
    let resid_sq: f64 = points
        .iter()
        .map(|(x, y)| (y - (alpha + beta * x)).powi(2))
        .sum();
    let dof = n - 2.0;
    let sigma_sq = resid_sq / dof;
    let se = (sigma_sq / s_xx).sqrt();

    let confidence = if se <= 0.0 || !se.is_finite() {
        // Perfect fit: certain if there's any slope, otherwise undecidable.
        if beta.abs() > 0.0 { 1.0 } else { 0.5 }
    } else {
        let t0 = beta.abs() / se;
        // P(true slope shares the sign of β) = 1 − ½·I_x(dof/2, ½),
        // x = dof/(dof + t0²); I is the regularized incomplete beta.
        let x = dof / (dof + t0 * t0);
        1.0 - 0.5 * betai(dof / 2.0, 0.5, x)
    };

    WeightTrend::Estimated {
        direction,
        slope_kg_per_week,
        confidence,
        days,
    }
}

// --- Special functions (Numerical Recipes): regularized incomplete beta ---

fn gammln(xx: f64) -> f64 {
    const COF: [f64; 6] = [
        76.180_091_729_471_46,
        -86.505_320_329_416_77,
        24.014_098_240_830_91,
        -1.231_739_572_450_155,
        0.120_865_097_386_617_9e-2,
        -0.539_523_938_495_3e-5,
    ];
    let mut tmp = xx + 5.5;
    tmp -= (xx + 0.5) * tmp.ln();
    let mut ser = 1.000_000_000_190_015;
    let mut y = xx;
    for c in COF.iter() {
        y += 1.0;
        ser += c / y;
    }
    -tmp + (2.506_628_274_631_000_5 * ser / xx).ln()
}

fn betacf(a: f64, b: f64, x: f64) -> f64 {
    const MAXIT: u32 = 200;
    const EPS: f64 = 3.0e-12;
    const FPMIN: f64 = 1.0e-300;

    let qab = a + b;
    let qap = a + 1.0;
    let qam = a - 1.0;
    let mut c = 1.0;
    let mut d = 1.0 - qab * x / qap;
    if d.abs() < FPMIN {
        d = FPMIN;
    }
    d = 1.0 / d;
    let mut h = d;
    for m in 1..=MAXIT {
        let m = m as f64;
        let m2 = 2.0 * m;
        let mut aa = m * (b - m) * x / ((qam + m2) * (a + m2));
        d = 1.0 + aa * d;
        if d.abs() < FPMIN {
            d = FPMIN;
        }
        c = 1.0 + aa / c;
        if c.abs() < FPMIN {
            c = FPMIN;
        }
        d = 1.0 / d;
        h *= d * c;
        aa = -(a + m) * (qab + m) * x / ((a + m2) * (qap + m2));
        d = 1.0 + aa * d;
        if d.abs() < FPMIN {
            d = FPMIN;
        }
        c = 1.0 + aa / c;
        if c.abs() < FPMIN {
            c = FPMIN;
        }
        d = 1.0 / d;
        let del = d * c;
        h *= del;
        if (del - 1.0).abs() < EPS {
            break;
        }
    }
    h
}

/// Regularized incomplete beta function I_x(a, b).
fn betai(a: f64, b: f64, x: f64) -> f64 {
    if x <= 0.0 {
        return 0.0;
    }
    if x >= 1.0 {
        return 1.0;
    }
    let bt = (gammln(a + b) - gammln(a) - gammln(b) + a * x.ln() + b * (1.0 - x).ln()).exp();
    if x < (a + 1.0) / (a + b + 2.0) {
        bt * betacf(a, b, x) / a
    } else {
        1.0 - bt * betacf(b, a, 1.0 - x) / b
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(date: &str, kg: f64) -> WeightEntry {
        WeightEntry {
            id: date.to_string(),
            date: date.to_string(),
            weight_kg: kg,
            no_water: false,
            no_food: false,
            no_wash: false,
            used_toilet: false,
            morning: false,
            created_at: String::new(),
            updated_at: String::new(),
        }
    }

    /// dd in 1..=14 -> "2026-06-DD"
    fn day(dd: u32, kg: f64) -> WeightEntry {
        entry(&format!("2026-06-{dd:02}"), kg)
    }

    /// Fixed, realistic day-to-day water/food noise (~±0.7 kg, mean ≈ 0, low
    /// correlation with the day index) — deterministic so tests are stable.
    const NOISE14: [f64; 14] = [
        0.6, -0.8, 0.3, -0.5, 0.9, -1.1, 0.4, -0.2, 0.7, -0.9, 0.5, -0.6, 0.8, -0.4,
    ];

    /// 14 daily weigh-ins: `base` at day 1, drifting `slope_per_day` kg/day, with
    /// the fixed noise on top.
    fn noisy(base: f64, slope_per_day: f64) -> Vec<WeightEntry> {
        (1..=14u32)
            .map(|dd| day(dd, base + slope_per_day * (dd as f64 - 1.0) + NOISE14[(dd - 1) as usize]))
            .collect()
    }

    #[test]
    fn clean_descent_is_confident_down() {
        // ~0.1 kg/day loss (0.7 kg/week) over 14 days + tiny alternating noise.
        let mut v = Vec::new();
        for dd in 1..=14u32 {
            let noise = if dd % 2 == 0 { 0.05 } else { -0.05 };
            v.push(day(dd, 90.0 - 0.1 * dd as f64 + noise));
        }
        match weight_trend(&v, DEFAULT_WINDOW_DAYS) {
            WeightTrend::Estimated { direction, slope_kg_per_week, confidence, days } => {
                assert_eq!(direction, Direction::Down);
                assert_eq!(days, 14);
                assert!((slope_kg_per_week + 0.7).abs() < 0.1, "slope {slope_kg_per_week}");
                assert!(confidence > 0.99, "confidence {confidence}");
            }
            other => panic!("expected Estimated, got {other:?}"),
        }
    }

    #[test]
    fn plateau_is_unclear() {
        // No trend, just noise -> confidence near 0.5.
        let mut v = Vec::new();
        for dd in 1..=14u32 {
            let noise = if dd % 2 == 0 { 0.3 } else { -0.3 };
            v.push(day(dd, 80.0 + noise));
        }
        match weight_trend(&v, DEFAULT_WINDOW_DAYS) {
            WeightTrend::Estimated { confidence, .. } => {
                assert!(confidence < 0.75, "confidence {confidence} should be low");
            }
            other => panic!("expected Estimated, got {other:?}"),
        }
    }

    #[test]
    fn rising_is_detected() {
        let mut v = Vec::new();
        for dd in 1..=14u32 {
            v.push(day(dd, 70.0 + 0.08 * dd as f64));
        }
        match weight_trend(&v, DEFAULT_WINDOW_DAYS) {
            WeightTrend::Estimated { direction, confidence, .. } => {
                assert_eq!(direction, Direction::Up);
                assert!(confidence > 0.99);
            }
            other => panic!("expected Estimated, got {other:?}"),
        }
    }

    #[test]
    fn three_noisy_points_are_not_overconfident() {
        // A short, noisy run shouldn't claim near-certainty.
        let v = vec![day(12, 80.6), day(13, 80.1), day(14, 80.3)];
        match weight_trend(&v, DEFAULT_WINDOW_DAYS) {
            WeightTrend::Estimated { confidence, days, .. } => {
                assert_eq!(days, 3);
                assert!(confidence < 0.95, "confidence {confidence} too high for 3 noisy pts");
            }
            other => panic!("expected Estimated, got {other:?}"),
        }
    }

    #[test]
    fn two_points_are_tentative_only() {
        let v = vec![day(13, 81.0), day(14, 80.5)];
        match weight_trend(&v, DEFAULT_WINDOW_DAYS) {
            WeightTrend::Tentative { direction, days, .. } => {
                assert_eq!(direction, Direction::Down);
                assert_eq!(days, 2);
            }
            other => panic!("expected Tentative, got {other:?}"),
        }
    }

    #[test]
    fn one_day_is_insufficient() {
        let v = vec![day(14, 80.0), day(14, 80.4)]; // same day -> 1 distinct day
        assert!(matches!(weight_trend(&v, DEFAULT_WINDOW_DAYS), WeightTrend::Insufficient { days: 1 }));
        assert!(matches!(weight_trend(&[], DEFAULT_WINDOW_DAYS), WeightTrend::Insufficient { days: 0 }));
    }

    #[test]
    fn balance_maps_confident_trends() {
        // Confident descent -> deficit (green), ascent -> surplus (pink).
        assert_eq!(weight_trend(&noisy(90.0, -0.1), DEFAULT_WINDOW_DAYS).balance(), BalanceState::Deficit);
        assert_eq!(weight_trend(&noisy(70.0, 0.12), DEFAULT_WINDOW_DAYS).balance(), BalanceState::Surplus);
        // Maintenance and buried/short trends read as maintenance (black).
        assert_eq!(weight_trend(&noisy(80.0, 0.0), DEFAULT_WINDOW_DAYS).balance(), BalanceState::Maintenance);
        assert_eq!(weight_trend(&noisy(80.0, -0.03), DEFAULT_WINDOW_DAYS).balance(), BalanceState::Maintenance);
        assert_eq!(weight_trend(&[day(13, 81.0), day(14, 80.5)], DEFAULT_WINDOW_DAYS).balance(), BalanceState::Maintenance);
        assert_eq!(weight_trend(&[], DEFAULT_WINDOW_DAYS).balance(), BalanceState::Maintenance);
    }

    #[test]
    fn noisy_descent_is_detected() {
        // Real ~0.7 kg/week loss buried in ±0.7 kg daily noise — still Down,
        // and confidently so over 14 days.
        match weight_trend(&noisy(90.0, -0.1), DEFAULT_WINDOW_DAYS) {
            WeightTrend::Estimated { direction, slope_kg_per_week, confidence, .. } => {
                assert_eq!(direction, Direction::Down);
                assert!(confidence > 0.85, "confidence {confidence}");
                assert!((-1.5..-0.2).contains(&slope_kg_per_week), "slope {slope_kg_per_week}");
            }
            other => panic!("expected Estimated, got {other:?}"),
        }
    }

    #[test]
    fn noisy_ascent_is_detected() {
        match weight_trend(&noisy(70.0, 0.12), DEFAULT_WINDOW_DAYS) {
            WeightTrend::Estimated { direction, confidence, .. } => {
                assert_eq!(direction, Direction::Up);
                assert!(confidence > 0.85, "confidence {confidence}");
            }
            other => panic!("expected Estimated, got {other:?}"),
        }
    }

    #[test]
    fn noisy_maintenance_is_unclear() {
        // No real trend, only noise -> low confidence regardless of slope sign.
        match weight_trend(&noisy(80.0, 0.0), DEFAULT_WINDOW_DAYS) {
            WeightTrend::Estimated { confidence, .. } => {
                assert!(confidence < 0.75, "confidence {confidence} should be low");
            }
            other => panic!("expected Estimated, got {other:?}"),
        }
    }

    #[test]
    fn weak_trend_buried_in_noise_is_not_confident() {
        // ~0.2 kg/week against ±0.7 kg noise — too weak to call confidently.
        match weight_trend(&noisy(80.0, -0.03), DEFAULT_WINDOW_DAYS) {
            WeightTrend::Estimated { confidence, .. } => {
                assert!(confidence < 0.85, "confidence {confidence} too high for a buried trend");
            }
            other => panic!("expected Estimated, got {other:?}"),
        }
    }

    #[test]
    fn descent_survives_water_spike_outliers() {
        // Two big mid-window spikes (+2.5 kg water) don't flip a real descent.
        let mut v = noisy(90.0, -0.1);
        v[4].weight_kg += 2.5; // day 5
        v[9].weight_kg += 2.5; // day 10
        match weight_trend(&v, DEFAULT_WINDOW_DAYS) {
            WeightTrend::Estimated { direction, days, .. } => {
                assert_eq!(direction, Direction::Down);
                assert_eq!(days, 14);
            }
            other => panic!("expected Estimated, got {other:?}"),
        }
    }

    #[test]
    fn old_points_outside_window_are_ignored() {
        // A big drop 30 days ago must not affect the recent flat window.
        let mut v = vec![entry("2026-05-15", 95.0)];
        for dd in 10..=14u32 {
            v.push(day(dd, 80.0));
        }
        match weight_trend(&v, DEFAULT_WINDOW_DAYS) {
            WeightTrend::Estimated { days, slope_kg_per_week, .. } => {
                assert_eq!(days, 5, "old point should be excluded");
                assert!(slope_kg_per_week.abs() < 0.01, "flat, got {slope_kg_per_week}");
            }
            other => panic!("expected Estimated, got {other:?}"),
        }
    }

    #[test]
    fn same_day_weighins_average() {
        // Two weigh-ins on day 14 average to 80.0; with day 1 at 80.0 -> flat.
        let v = vec![day(1, 80.0), day(14, 79.5), day(14, 80.5)];
        match weight_trend(&v, DEFAULT_WINDOW_DAYS) {
            WeightTrend::Tentative { days, slope_kg_per_week, .. } => {
                assert_eq!(days, 2);
                assert!(slope_kg_per_week.abs() < 1e-9, "should be flat, got {slope_kg_per_week}");
            }
            other => panic!("expected Tentative, got {other:?}"),
        }
    }
}
