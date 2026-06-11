//! Time-based market activation reconcile.
//!
//! Kalshi transitions a market from `initialized` (pre-open) to `active` (open for
//! trading) **time-based on `open_time`, with NO `market_lifecycle_v2` event** — the
//! `activated` event only fires when a *paused* market is re-enabled, never for the
//! ordinary window-open transition (confirmed against Kalshi docs + our own
//! `market_lifecycle_events`: the sequence is `created → metadata_updated → determined
//! → settled`, no `activated`). `metadata_updated` happens to land at window-open for
//! 15M markets only because Kalshi sets the final strike then — undocumented and
//! fragile, so we must NOT key activation off it.
//!
//! Because there is no event to react to, the cache replicates Kalshi's time-based
//! transition itself: a lightweight periodic sweep promotes any market whose trading
//! window is currently open (`open_time <= now < close_time`) but is still
//! `initialized` to `active`. The status UPDATE is captured by CDC, which the
//! connector consumes to subscribe to the now-open market via its existing
//! `status='active'` path. This keeps `markets.status` correct for *all* consumers
//! (monitor UI, cache warmer), not just the connector.
//!
//! Without this, only the hourly `ssmd-secmaster-crypto` REST sync flips the
//! currently-open window to `active`, so the fast-rolling 15M crypto markets that
//! open at :15/:30/:45 never get subscribed and the connector sits at zero markets
//! for ~45 min/hour (the "connector subscribed to zero markets" CRITICAL alert).

use deadpool_postgres::Pool;
use crate::{Result, Error};

/// Promote Kalshi markets whose trading window is open right now but are still
/// `initialized`. The predicate mirrors [`window_is_open`] exactly (single source of
/// truth for the rule — keep them in sync).
///
/// Scope: no feed filter is needed — every row in `markets` is a Kalshi market and
/// `initialized` is a Kalshi-only status. The `NOW() < close_time` guard prevents
/// activating already-closed windows; `open_time IS NOT NULL` prevents activating
/// markets with no known open time (fail-safe: leave them `initialized` rather than
/// guess). Demotion is intentionally NOT done here — terminal lifecycle events
/// (`determined`/`settled`) drive the connector to unsubscribe at window close.
///
/// A `close_time IS NOT NULL` guard is intentionally absent: in Postgres
/// `NOW() < NULL` evaluates to NULL, which is falsy in a WHERE clause, so rows with a
/// NULL `close_time` are already excluded. Adding the explicit guard would be correct
/// but redundant.
///
/// Predicate order is irrelevant to correctness (all clauses are ANDed) — do not
/// assume the [`window_is_open`] mirror or any test depends on ordering.
const ACTIVATE_OPEN_WINDOWS_SQL: &str = "\
    UPDATE markets \
       SET status = 'active', updated_at = NOW() \
     WHERE status = 'initialized' \
       AND open_time IS NOT NULL \
       AND open_time <= NOW() \
       AND NOW() < close_time";

/// The activation rule, as a pure predicate: a market should be promoted
/// `initialized` → `active` iff its trading window is open right now
/// (`open_time <= now < close_time`). Mirrors [`ACTIVATE_OPEN_WINDOWS_SQL`].
///
/// Generic over any ordered timestamp representation so it is trivially unit-testable
/// without a database or datetime dependency. A missing `open_time` or `close_time`
/// means "window unknown" → not open (fail-safe).
pub fn window_is_open<T: PartialOrd>(open_time: Option<T>, close_time: Option<T>, now: T) -> bool {
    match (open_time, close_time) {
        (Some(open), Some(close)) => open <= now && now < close,
        _ => false,
    }
}

/// Periodically replicates Kalshi's time-based `initialized` → `active` transition.
pub struct MarketActivator {
    pool: Pool,
}

impl MarketActivator {
    pub fn new(pool: Pool) -> Self {
        Self { pool }
    }

    /// Promote every Kalshi market whose window is currently open but still
    /// `initialized` to `active`. Returns the number of markets promoted (0 on a
    /// quiet tick — the common case between window rollovers).
    ///
    /// Fails loud: any DB error propagates so the caller can crash the process
    /// (no limping — a cache that can't reconcile market status is broken and must
    /// be restarted by K8s, matching the synchronization-architecture contract).
    pub async fn activate_open_windows(&self) -> Result<u64> {
        let client = self.pool.get().await?;
        let promoted = client
            .execute(ACTIVATE_OPEN_WINDOWS_SQL, &[])
            .await
            .map_err(|e| Error::Database(format!("market activation reconcile UPDATE failed: {e}")))?;
        Ok(promoted)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn window_open_when_now_between_open_and_close() {
        // open=100, close=200, now=150 → open
        assert!(window_is_open(Some(100i64), Some(200i64), 150));
    }

    #[test]
    fn window_open_at_exact_open_boundary_is_open() {
        // open_time <= now → inclusive lower bound (market opens AT open_time)
        assert!(window_is_open(Some(100i64), Some(200i64), 100));
    }

    #[test]
    fn window_closed_at_exact_close_boundary() {
        // now < close_time → exclusive upper bound (market is closed AT close_time)
        assert!(!window_is_open(Some(100i64), Some(200i64), 200));
    }

    #[test]
    fn window_closed_before_open() {
        assert!(!window_is_open(Some(100i64), Some(200i64), 50));
    }

    #[test]
    fn window_closed_after_close() {
        assert!(!window_is_open(Some(100i64), Some(200i64), 250));
    }

    #[test]
    fn missing_open_time_is_not_open() {
        // Fail-safe: unknown open time → leave initialized.
        assert!(!window_is_open::<i64>(None, Some(200i64), 150));
    }

    #[test]
    fn missing_close_time_is_not_open() {
        // Fail-safe: unknown close time → leave initialized.
        assert!(!window_is_open::<i64>(Some(100i64), None, 150));
    }

    #[test]
    fn missing_both_bounds_is_not_open() {
        assert!(!window_is_open::<i64>(None, None, 150));
    }

    #[test]
    fn zero_length_window_is_never_open() {
        // open == close → no instant satisfies open <= now < close.
        assert!(!window_is_open(Some(100i64), Some(100i64), 100));
    }

    #[test]
    fn inverted_window_is_never_open() {
        // open_time > close_time is a data error (bad feed / botched migration);
        // must never activate. open=200, close=100, now=150 → open(200)<=150 is false.
        assert!(!window_is_open(Some(200i64), Some(100i64), 150));
    }

    #[test]
    fn handles_extreme_bounds() {
        // Exclusive upper bound holds even at the type's max.
        assert!(window_is_open(Some(i64::MIN), Some(i64::MAX), 0));
        assert!(!window_is_open(Some(i64::MAX - 1), Some(i64::MAX), i64::MAX));
        assert!(window_is_open(Some(i64::MAX - 1), Some(i64::MAX), i64::MAX - 1));
    }

    #[test]
    fn activation_sql_targets_initialized_open_windows_only() {
        // Guard against accidental edits that would broaden the sweep's scope:
        // it must only ever promote `initialized` rows within an open window, and
        // must never demote or touch terminal/active rows.
        assert!(ACTIVATE_OPEN_WINDOWS_SQL.contains("status = 'initialized'"));
        assert!(ACTIVATE_OPEN_WINDOWS_SQL.contains("SET status = 'active'"));
        assert!(ACTIVATE_OPEN_WINDOWS_SQL.contains("open_time <= NOW()"));
        assert!(ACTIVATE_OPEN_WINDOWS_SQL.contains("NOW() < close_time"));
        assert!(ACTIVATE_OPEN_WINDOWS_SQL.contains("open_time IS NOT NULL"));
        // Mutation guards: the sweep must never demote (set initialized) or delete.
        assert!(!ACTIVATE_OPEN_WINDOWS_SQL.contains("SET status = 'initialized'"));
        assert!(!ACTIVATE_OPEN_WINDOWS_SQL.contains("DELETE"));
    }
}
