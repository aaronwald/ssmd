//! Shared, defensive price/quantity converters used by BOTH the live ticker
//! path (`consumer.rs`) and the startup reconciler (`reconcile.rs`).
//!
//! These are the single source of truth for turning untrusted exchange / DB
//! strings into native Kalshi cents (prices) or contract counts (volume, open
//! interest). Keeping them here prevents the two ingest paths from drifting —
//! previously `reconcile.rs` carried its own unclamped copy, so a bad secmaster
//! row could write an out-of-domain immutable settlement object before the
//! correct lifecycle record ever ran (and create-if-absent can't repair it).
//!
//! Invariants enforced here:
//! - Prices clamp to the valid Kalshi domain `[0, 100]` cents.
//! - Volume / open interest clamp to non-negative (no upper bound).
//! - Malformed / non-finite input yields `None` — never a panic on bad bytes.

/// Convert a fractional-dollar string like `"0.9990"` to native Kalshi cents,
/// rounding to the nearest cent, then clamping to the valid price domain
/// `[0, 100]`. Returns `None` for a non-numeric or non-finite string so one
/// malformed field never poisons the whole record (or panics on untrusted
/// input). The clamp rejects absurd values (e.g. `"9e99"` → `i64::MAX`, or a
/// negative) into the boundary instead of persisting garbage, and keeps the
/// NO-side complement (`100 - yes`) overflow-proof.
pub fn dollars_to_cents(s: &str) -> Option<i64> {
    let dollars: f64 = s.trim().parse().ok()?;
    if !dollars.is_finite() {
        return None;
    }
    Some(((dollars * 100.0).round() as i64).clamp(0, 100))
}

/// Convert a fixed-point string like `"2233487.48"` to a rounded, non-negative
/// integer count (volume / open interest). Defensive like [`dollars_to_cents`]:
/// malformed or non-finite input yields `None`, never a panic. No upper bound —
/// volume / open interest are legitimately large.
pub fn fp_to_i64(s: &str) -> Option<i64> {
    let val: f64 = s.trim().parse().ok()?;
    if !val.is_finite() {
        return None;
    }
    Some((val.round() as i64).max(0))
}

/// Clamp an already-integer price cent (e.g. the legacy integer wire field, or a
/// DB integer count read as a price) into the valid Kalshi domain `[0, 100]`.
pub fn clamp_price_cents(c: i64) -> i64 {
    c.clamp(0, 100)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dollars_to_cents_rounds_in_range() {
        assert_eq!(dollars_to_cents("0.9990"), Some(100)); // 99.9 -> 100
        assert_eq!(dollars_to_cents("0.9700"), Some(97));
        assert_eq!(dollars_to_cents("0.0200"), Some(2));
        assert_eq!(dollars_to_cents("1.0000"), Some(100));
        assert_eq!(dollars_to_cents("0.0000"), Some(0));
        assert_eq!(dollars_to_cents("0.005"), Some(1)); // 0.5c rounds to 1
        assert_eq!(dollars_to_cents("0.004"), Some(0));
    }

    #[test]
    fn dollars_to_cents_clamps_out_of_domain() {
        // Above $1.00 clamps to 100; negative clamps to 0.
        assert_eq!(dollars_to_cents("2.5000"), Some(100));
        assert_eq!(dollars_to_cents("-0.5000"), Some(0));
        // Absurd magnitudes clamp to the boundary, never overflow.
        assert_eq!(dollars_to_cents("9e99"), Some(100));
        assert_eq!(dollars_to_cents("-1e300"), Some(0));
    }

    #[test]
    fn dollars_to_cents_rejects_malformed_and_non_finite() {
        assert_eq!(dollars_to_cents(""), None);
        assert_eq!(dollars_to_cents("not-a-number"), None);
        assert_eq!(dollars_to_cents("inf"), None);
        assert_eq!(dollars_to_cents("-inf"), None);
        assert_eq!(dollars_to_cents("nan"), None);
    }

    #[test]
    fn fp_to_i64_rounds_and_floors_at_zero() {
        assert_eq!(fp_to_i64("2233487.48"), Some(2233487));
        assert_eq!(fp_to_i64("618700.14"), Some(618700));
        assert_eq!(fp_to_i64("0.00"), Some(0));
        // Negative counts are impossible — floor at 0.
        assert_eq!(fp_to_i64("-5.0"), Some(0));
    }

    #[test]
    fn fp_to_i64_rejects_malformed_and_non_finite() {
        assert_eq!(fp_to_i64(""), None);
        assert_eq!(fp_to_i64("garbage"), None);
        assert_eq!(fp_to_i64("inf"), None);
        assert_eq!(fp_to_i64("nan"), None);
    }

    #[test]
    fn clamp_price_cents_bounds_domain() {
        assert_eq!(clamp_price_cents(-50), 0);
        assert_eq!(clamp_price_cents(0), 0);
        assert_eq!(clamp_price_cents(50), 50);
        assert_eq!(clamp_price_cents(100), 100);
        assert_eq!(clamp_price_cents(150), 100);
        assert_eq!(clamp_price_cents(i64::MIN), 0);
        assert_eq!(clamp_price_cents(i64::MAX), 100);
    }
}
