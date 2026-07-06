//! Menstrual-cycle detection from weight alone (no logged period dates).
//!
//! Female-only, separate from [`super::weight_trend`]. The monthly cycle adds a
//! water-retention oscillation (~0.3–2 kg, period ~21–35 days) on top of the
//! linear fat trend; over a short window it masquerades as a rise/fall. Here we
//! fit, over a long window, a trend PLUS a single sinusoid whose period is found
//! by scanning, then judge whether that cyclic component is real:
//!
//!   y = α + β·(x − x̄) + γ·sin(2πx/P) + δ·cos(2πx/P)
//!
//! - `β` is the **cycle-free** trend (use it instead of the naive slope).
//! - `A = √(γ²+δ²)` is the cycle amplitude; the phase gives the current
//!   deviation from the cyclic baseline.
//! - Detection gate (avoid inventing a cycle from noise): enough data (≥ ~2
//!   cycles), amplitude above a floor, AND an F-test of the cycle term passing a
//!   strict threshold (strict because scanning many periods inflates the
//!   false-alarm rate — the amplitude floor is the main guard).
//!
//! This is the "harmonic regression with period scan" approach (mathematically
//! the Lomb–Scargle periodogram), chosen because it works on irregular sampling
//! and drops straight onto the existing least-squares machinery.

use api_types::WeightEntry;

use super::weight_trend::{betai, daily_means};

/// Long window for cycle detection — needs to span at least ~2 cycles.
pub const CYCLE_WINDOW_DAYS: i64 = 90;

const P_MIN: f64 = 21.0;
const P_MAX: f64 = 35.0;
const P_STEP: f64 = 0.25;
/// Need at least this many distinct measurement days and this much date span.
const MIN_DAYS: usize = 21;
const MIN_SPAN_DAYS: f64 = 2.0 * P_MIN;
/// Cycle amplitude floor — below this it's indistinguishable from noise.
const MIN_AMPLITUDE_KG: f64 = 0.3;
/// F-test threshold (strict to offset multiple-period scanning).
const ALPHA: f64 = 0.01;

#[derive(Debug, Clone, PartialEq)]
pub struct CycleFit {
    /// Detected cycle length in days.
    pub period_days: f64,
    /// Half peak-to-trough water swing, kg.
    pub amplitude_kg: f64,
    /// Cyclic offset of the latest measurement vs the cycle baseline (+ above).
    pub current_deviation_kg: f64,
    /// Cycle-free linear trend (use instead of the naive slope), kg/week.
    pub trend_kg_per_week: f64,
    /// Directional confidence of the cycle-free trend, [0.5, 1].
    pub trend_confidence: f64,
    /// F-test p-value of the cyclic component (smaller = more real).
    pub p_value: f64,
}

#[derive(Debug, Clone, PartialEq)]
pub enum CycleResult {
    /// Not enough history to even attempt detection — fall back to weight_trend.
    Insufficient { days: usize },
    /// Ran the fit, but no significant cycle — fall back to weight_trend.
    NotDetected { days: usize },
    /// A cycle was found.
    Detected(CycleFit),
}

/// Attempt to detect a menstrual weight cycle over the last `window_days`.
pub fn weight_cycle(entries: &[WeightEntry], window_days: i64) -> CycleResult {
    let points = daily_means(entries, window_days);
    let days = points.len();

    let span = match (points.first(), points.last()) {
        (Some((x0, _)), Some((x1, _))) => x1 - x0,
        _ => 0.0,
    };
    if days < MIN_DAYS || span < MIN_SPAN_DAYS {
        return CycleResult::Insufficient { days };
    }

    let n = days as f64;
    let mean_x = points.iter().map(|(x, _)| x).sum::<f64>() / n;
    let mean_y = points.iter().map(|(_, y)| y).sum::<f64>() / n;

    // Linear-only residual (the null model: trend without a cycle).
    let s_xx: f64 = points.iter().map(|(x, _)| (x - mean_x).powi(2)).sum();
    let s_xy: f64 = points.iter().map(|(x, y)| (x - mean_x) * (y - mean_y)).sum();
    let lin_beta = s_xy / s_xx;
    let rss0: f64 = points
        .iter()
        .map(|(x, y)| (y - (mean_y + lin_beta * (x - mean_x))).powi(2))
        .sum();

    // Scan periods for the best (lowest-residual) harmonic fit.
    let mut best: Option<(f64, Harmonic)> = None;
    let mut p = P_MIN;
    while p <= P_MAX {
        if let Some(h) = fit_harmonic(&points, mean_x, p) {
            if best.as_ref().map_or(true, |(_, b)| h.rss < b.rss) {
                best = Some((p, h));
            }
        }
        p += P_STEP;
    }
    let Some((period_days, h)) = best else {
        return CycleResult::NotDetected { days };
    };

    let amplitude_kg = (h.gamma * h.gamma + h.delta * h.delta).sqrt();

    // F-test: does the 2-parameter cycle term explain significantly more than
    // the linear model? d1 = 2 added params, d2 = n − 4.
    let d1 = 2.0;
    let d2 = n - 4.0;
    let p_value = if d2 <= 0.0 || h.rss <= 0.0 || rss0 <= h.rss {
        if rss0 <= h.rss { 1.0 } else { 0.0 }
    } else {
        let f = ((rss0 - h.rss) / d1) / (h.rss / d2);
        // P(F_{d1,d2} > f) via the regularized incomplete beta.
        betai(d2 / 2.0, d1 / 2.0, d2 / (d2 + d1 * f))
    };

    // Cycle-free trend and its confidence (Student-t on β within the full model).
    let sigma_sq = if d2 > 0.0 { h.rss / d2 } else { 0.0 };
    let se_beta = (sigma_sq * h.inv_beta).sqrt();
    let trend_confidence = directional_confidence(h.beta, se_beta, d2);

    let x_latest = points.last().map(|(x, _)| *x).unwrap_or(0.0);
    let current_deviation_kg =
        h.gamma * (TAU * x_latest / period_days).sin() + h.delta * (TAU * x_latest / period_days).cos();

    if amplitude_kg >= MIN_AMPLITUDE_KG && p_value < ALPHA {
        CycleResult::Detected(CycleFit {
            period_days,
            amplitude_kg,
            current_deviation_kg,
            trend_kg_per_week: h.beta * 7.0,
            trend_confidence,
            p_value,
        })
    } else {
        CycleResult::NotDetected { days }
    }
}

const TAU: f64 = std::f64::consts::TAU;

struct Harmonic {
    beta: f64,    // slope (coeff of centred x)
    gamma: f64,   // sin coeff
    delta: f64,   // cos coeff
    rss: f64,
    inv_beta: f64, // (XᵀX)⁻¹ diagonal entry for β, for SE(β)
}

/// Fit `y = c0 + c1·(x−x̄) + c2·sin(2πx/P) + c3·cos(2πx/P)` by least squares.
fn fit_harmonic(points: &[(f64, f64)], mean_x: f64, period: f64) -> Option<Harmonic> {
    // Normal equations XᵀX (4×4) and Xᵀy (4).
    let mut xtx = [[0.0f64; 4]; 4];
    let mut xty = [0.0f64; 4];
    for &(x, y) in points {
        let w = TAU * x / period;
        let row = [1.0, x - mean_x, w.sin(), w.cos()];
        for i in 0..4 {
            for j in 0..4 {
                xtx[i][j] += row[i] * row[j];
            }
            xty[i] += row[i] * y;
        }
    }
    let inv = invert4(xtx)?;
    let mut c = [0.0f64; 4];
    for i in 0..4 {
        for j in 0..4 {
            c[i] += inv[i][j] * xty[j];
        }
    }
    let rss: f64 = points
        .iter()
        .map(|&(x, y)| {
            let w = TAU * x / period;
            let yhat = c[0] + c[1] * (x - mean_x) + c[2] * w.sin() + c[3] * w.cos();
            (y - yhat).powi(2)
        })
        .sum();
    Some(Harmonic {
        beta: c[1],
        gamma: c[2],
        delta: c[3],
        rss,
        inv_beta: inv[1][1],
    })
}

/// 4×4 matrix inverse via Gauss–Jordan with partial pivoting. None if singular.
fn invert4(m: [[f64; 4]; 4]) -> Option<[[f64; 4]; 4]> {
    let mut a = [[0.0f64; 8]; 4];
    for i in 0..4 {
        for j in 0..4 {
            a[i][j] = m[i][j];
        }
        a[i][4 + i] = 1.0;
    }
    for col in 0..4 {
        // Partial pivot.
        let mut pivot = col;
        for r in (col + 1)..4 {
            if a[r][col].abs() > a[pivot][col].abs() {
                pivot = r;
            }
        }
        if a[pivot][col].abs() < 1e-12 {
            return None;
        }
        a.swap(col, pivot);
        let d = a[col][col];
        for j in 0..8 {
            a[col][j] /= d;
        }
        for r in 0..4 {
            if r != col {
                let f = a[r][col];
                for j in 0..8 {
                    a[r][j] -= f * a[col][j];
                }
            }
        }
    }
    let mut inv = [[0.0f64; 4]; 4];
    for i in 0..4 {
        for j in 0..4 {
            inv[i][j] = a[i][4 + j];
        }
    }
    Some(inv)
}

/// P(true slope shares β's sign): 1 − ½·I_x(dof/2, ½), x = dof/(dof + t0²).
fn directional_confidence(beta: f64, se: f64, dof: f64) -> f64 {
    if se <= 0.0 || !se.is_finite() {
        return if beta.abs() > 0.0 { 1.0 } else { 0.5 };
    }
    let t0 = beta.abs() / se;
    let x = dof / (dof + t0 * t0);
    1.0 - 0.5 * betai(dof / 2.0, 0.5, x)
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

    /// Deterministic, aperiodic pseudo-noise in [-amp, amp].
    fn noise(i: usize, amp: f64) -> f64 {
        let s = ((i as f64) * 12.9898).sin() * 43758.5453;
        ((s - s.floor()) * 2.0 - 1.0) * amp
    }

    /// `n` daily entries from 2026-01-01, weight = base + slope·day + cycle + noise.
    fn series(n: usize, base: f64, slope: f64, amp: f64, period: f64, noise_amp: f64) -> Vec<WeightEntry> {
        let start = chrono::NaiveDate::from_ymd_opt(2026, 1, 1).unwrap();
        (0..n)
            .map(|i| {
                let d = start + chrono::Duration::days(i as i64);
                let cyc = amp * (TAU * i as f64 / period).sin();
                entry(
                    &d.format("%Y-%m-%d").to_string(),
                    base + slope * i as f64 + cyc + noise(i, noise_amp),
                )
            })
            .collect()
    }

    #[test]
    fn detects_a_real_cycle() {
        // 90 days, slow loss + a clear 28-day, 1 kg cycle, modest noise.
        let v = series(90, 75.0, -0.05, 1.0, 28.0, 0.2);
        match weight_cycle(&v, CYCLE_WINDOW_DAYS) {
            CycleResult::Detected(f) => {
                assert!((25.0..31.0).contains(&f.period_days), "period {}", f.period_days);
                assert!((0.6..1.4).contains(&f.amplitude_kg), "amplitude {}", f.amplitude_kg);
                assert!((-0.6..-0.1).contains(&f.trend_kg_per_week), "trend {}", f.trend_kg_per_week);
                assert!(f.p_value < ALPHA, "p {}", f.p_value);
            }
            other => panic!("expected Detected, got {other:?}"),
        }
    }

    #[test]
    fn no_cycle_just_trend_and_noise_is_not_detected() {
        // Pure trend + noise, no periodicity -> must NOT invent a cycle.
        let v = series(90, 80.0, -0.04, 0.0, 28.0, 0.4);
        match weight_cycle(&v, CYCLE_WINDOW_DAYS) {
            CycleResult::NotDetected { .. } => {}
            other => panic!("expected NotDetected, got {other:?}"),
        }
    }

    #[test]
    fn short_history_is_insufficient() {
        let v = series(20, 70.0, -0.05, 1.0, 28.0, 0.2);
        assert!(matches!(weight_cycle(&v, CYCLE_WINDOW_DAYS), CycleResult::Insufficient { .. }));
    }

    #[test]
    fn detects_cycle_with_irregular_sampling() {
        // Real data is gappy. Drop ~1/3 of days; the cycle must still surface.
        let start = chrono::NaiveDate::from_ymd_opt(2026, 1, 1).unwrap();
        let v: Vec<WeightEntry> = (0..90)
            .filter(|i| i % 3 != 0) // keep ~60 of 90 days
            .map(|i| {
                let d = start + chrono::Duration::days(i as i64);
                let cyc = 1.0 * (TAU * i as f64 / 28.0).sin();
                entry(&d.format("%Y-%m-%d").to_string(), 75.0 - 0.04 * i as f64 + cyc + noise(i, 0.2))
            })
            .collect();
        match weight_cycle(&v, CYCLE_WINDOW_DAYS) {
            CycleResult::Detected(f) => {
                assert!((25.0..31.0).contains(&f.period_days), "period {}", f.period_days);
                assert!((0.6..1.4).contains(&f.amplitude_kg), "amplitude {}", f.amplitude_kg);
            }
            other => panic!("expected Detected with gaps, got {other:?}"),
        }
    }

    #[test]
    fn detects_period_at_band_edges() {
        for period in [22.0, 34.0] {
            let v = series(90, 78.0, -0.02, 1.0, period, 0.15);
            match weight_cycle(&v, CYCLE_WINDOW_DAYS) {
                CycleResult::Detected(f) => {
                    assert!((f.period_days - period).abs() < 3.0, "period {} for true {}", f.period_days, period);
                }
                other => panic!("expected Detected for P={period}, got {other:?}"),
            }
        }
    }

    #[test]
    fn borderline_amplitude_below_floor_is_not_detected() {
        // A real-but-tiny 0.2 kg swing (< MIN_AMPLITUDE_KG) buried in noise must
        // not be claimed — honest "no cycle" rather than a fragile detection.
        let v = series(90, 80.0, -0.03, 0.2, 28.0, 0.4);
        match weight_cycle(&v, CYCLE_WINDOW_DAYS) {
            CycleResult::NotDetected { .. } => {}
            other => panic!("expected NotDetected for sub-floor amplitude, got {other:?}"),
        }
    }

    #[test]
    fn decycled_weight_recovers_baseline() {
        // Flat trend + strong 30-day cycle: removing the cyclic offset from the
        // latest weight should land back on the ~80 kg baseline.
        let base = 80.0;
        let v = series(90, base, 0.0, 1.5, 30.0, 0.1);
        if let CycleResult::Detected(f) = weight_cycle(&v, CYCLE_WINDOW_DAYS) {
            let latest_kg = v.last().unwrap().weight_kg;
            let decycled = latest_kg - f.current_deviation_kg;
            assert!((decycled - base).abs() < 0.3, "decycled {decycled}, base {base}");
        } else {
            panic!("expected Detected");
        }
    }

    #[test]
    fn detected_trend_is_cycle_free() {
        // Strong cycle around a flat trend: the reported trend must be ~0, not
        // dragged by the cyclic phase.
        let v = series(90, 80.0, 0.0, 1.5, 30.0, 0.15);
        if let CycleResult::Detected(f) = weight_cycle(&v, CYCLE_WINDOW_DAYS) {
            assert!(f.trend_kg_per_week.abs() < 0.15, "trend {}", f.trend_kg_per_week);
        } else {
            panic!("expected Detected");
        }
    }
}
