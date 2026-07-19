//! Startup reconciliation backfill (Task 10).
//!
//! On startup, scan the secmaster `markets` table for recently-settled 15-minute
//! crypto markets that have a result, and write a (lower-fidelity) settlement
//! record for any that lack a GCS object. This closes the lifecycle-stream
//! retention gap after a restart — the same "reload from the source of truth"
//! principle the connector and cache follow.
//!
//! Records produced here carry `snap_source = Secmaster`: the final snap fields
//! come from the `markets` row (a daily/secmaster cadence), not the race-free
//! in-process last-tick map, so the model layer can weight them accordingly.

use std::sync::Arc;

use anyhow::{Context as _, Result};
use deadpool_postgres::Pool;
use futures_util::TryStreamExt;

use crate::gcs::{GcsWriter, WriteOutcome};
use crate::record::{SettlementRecord, SettlementTrigger, SnapSource};
use crate::ticker::LastTick;

/// How far back to scan for settled markets to backfill.
const LOOKBACK: &str = "2 hours";

/// A `markets` row projected for backfill. Prices arrive as text (NUMERIC dollar
/// columns cast to text in SQL) and timestamps as RFC3339 text (timestamptz cast
/// to text — the known chrono/NaiveDateTime deserialization gotcha).
#[derive(Debug, Clone)]
pub struct MarketRow {
    pub market_ticker: String,
    pub event_ticker: Option<String>,
    pub result: Option<String>,
    pub yes_bid_dollars: Option<String>,
    pub yes_ask_dollars: Option<String>,
    pub no_bid_dollars: Option<String>,
    pub no_ask_dollars: Option<String>,
    pub last_price_dollars: Option<String>,
    pub volume: Option<i64>,
    pub open_interest: Option<i64>,
    /// `close_time` as epoch seconds (timestamptz cast to text, parsed). Used as
    /// both close and (approximate) determination time in the backfill record.
    pub close_ts: Option<i64>,
}

/// Convert an optional NUMERIC dollar string (e.g. "0.9700") to clamped integer
/// cents (97). Delegates to the shared, defensive [`crate::price::dollars_to_cents`]
/// so the reconcile path shares ONE converter with the live ticker path — the
/// same finite guard and `[0, 100]` clamp apply. Returns `None` for
/// absent/unparseable/non-finite values; never panics or persists out-of-domain.
fn dollars_to_cents(dollars: Option<&str>) -> Option<i64> {
    dollars.and_then(crate::price::dollars_to_cents)
}

/// Parse an RFC3339 timestamptz text value to epoch seconds.
pub fn ts_text_to_epoch(text: Option<&str>) -> Option<i64> {
    let s = text?;
    chrono::DateTime::parse_from_rfc3339(s.trim())
        .ok()
        .map(|dt| dt.timestamp())
}

/// Build a Secmaster-sourced settlement record from a `markets` row. Pure and
/// unit-testable. The snap fields come from the row; `snap_source = Secmaster`.
pub fn record_from_row(row: &MarketRow, now_ms: i64) -> SettlementRecord {
    let trigger = SettlementTrigger {
        market_ticker: row.market_ticker.clone(),
        event_ticker: row.event_ticker.clone(),
        result: row.result.clone(),
        // markets has no settlement_value column; the lifecycle path supplies it.
        settlement_value: None,
        close_ts: row.close_ts,
        // markets has no determination_ts; close_time is the best available
        // partition key for the backfill (flagged via snap_source = Secmaster).
        determination_ts: row.close_ts,
        // No settled_ts in the reconcile backfill; determination_ts (= close_ts)
        // already provides the partition date.
        settled_ts: None,
        nats_lifecycle_seq: -1,
    };

    let tick = LastTick {
        yes_bid: dollars_to_cents(row.yes_bid_dollars.as_deref()),
        yes_ask: dollars_to_cents(row.yes_ask_dollars.as_deref()),
        no_bid: dollars_to_cents(row.no_bid_dollars.as_deref()),
        no_ask: dollars_to_cents(row.no_ask_dollars.as_deref()),
        last_price: dollars_to_cents(row.last_price_dollars.as_deref()),
        // Volume / open interest can't be negative — floor at 0, mirroring the
        // live ticker path (`price::fp_to_i64`). A bad negative DB value never
        // persists.
        volume: row.volume.map(|v| v.max(0)),
        open_interest: row.open_interest.map(|v| v.max(0)),
        ts: row.close_ts.unwrap_or(0),
    };

    SettlementRecord::build_with_source(&trigger, Some(tick), SnapSource::Secmaster, now_ms)
}

/// Backfill missed settlements from the secmaster `markets` table. Returns the
/// number of records actually written (existing objects are skipped). A query
/// or DB error propagates so the caller can decide (the run loop logs and
/// continues — reconciliation is a best-effort backstop, not the trigger).
pub async fn run(pool: &Pool, gcs: &Arc<GcsWriter>) -> Result<u64> {
    let client = pool.get().await.context("get DB client for reconcile")?;

    // Crypto 15M markets that have settled with a result in the recent window.
    // timestamptz -> text (chrono gotcha); NUMERIC dollar prices -> text.
    let query = format!(
        r#"
        SELECT m.ticker,
               m.event_ticker,
               m.result,
               m.yes_bid::text,
               m.yes_ask::text,
               m.no_bid::text,
               m.no_ask::text,
               m.last_price::text,
               m.volume,
               m.open_interest,
               m.close_time::text
        FROM markets m
        JOIN events e ON e.event_ticker = m.event_ticker
        WHERE e.category = 'Crypto'
          AND e.series_ticker LIKE '%15M'
          AND m.status = 'settled'
          AND m.result IS NOT NULL
          AND m.close_time IS NOT NULL
          AND m.close_time > NOW() - INTERVAL '{LOOKBACK}'
        "#
    );

    let row_stream = client
        .query_raw(query.as_str(), &[] as &[&str])
        .await
        .context("reconcile markets query")?;
    tokio::pin!(row_stream);

    let now_ms = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0);

    let mut scanned: u64 = 0;
    let mut written: u64 = 0;

    while let Some(row) = row_stream.try_next().await.context("reconcile row")? {
        scanned += 1;
        let market_row = MarketRow {
            market_ticker: row.get(0),
            event_ticker: row.get(1),
            result: row.get(2),
            yes_bid_dollars: row.get(3),
            yes_ask_dollars: row.get(4),
            no_bid_dollars: row.get(5),
            no_ask_dollars: row.get(6),
            last_price_dollars: row.get(7),
            volume: row.get(8),
            open_interest: row.get(9),
            close_ts: ts_text_to_epoch(row.get::<_, Option<String>>(10).as_deref()),
        };

        let record = record_from_row(&market_row, now_ms);
        match gcs.write_if_absent(&record).await {
            Ok(WriteOutcome::Written) => written += 1,
            Ok(WriteOutcome::Exists) => {}
            Err(e) => {
                // A single backfill write failure is logged and skipped — the
                // forward path / a later reconcile pass will retry. Do not abort
                // the whole backfill on one bad object.
                tracing::warn!(
                    market_ticker = %market_row.market_ticker,
                    error = %e,
                    "reconcile write failed (skipping)",
                );
            }
        }
    }

    tracing::info!(scanned, written, "reconcile scan complete");
    Ok(written)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::gcs::object_path;

    fn sample_row() -> MarketRow {
        MarketRow {
            market_ticker: "KXBTC15M-26JUN031415-15".to_string(),
            event_ticker: Some("KXBTC15M-26JUN031415".to_string()),
            result: Some("yes".to_string()),
            yes_bid_dollars: Some("0.9600".to_string()),
            yes_ask_dollars: Some("0.9800".to_string()),
            no_bid_dollars: Some("0.0200".to_string()),
            no_ask_dollars: Some("0.0400".to_string()),
            last_price_dollars: Some("0.9700".to_string()),
            volume: Some(1000),
            open_interest: Some(500),
            close_ts: Some(1780496100),
        }
    }

    #[test]
    fn dollars_to_cents_rounds() {
        assert_eq!(dollars_to_cents(Some("0.9700")), Some(97));
        assert_eq!(dollars_to_cents(Some("0.0200")), Some(2));
        assert_eq!(dollars_to_cents(Some("1.0000")), Some(100));
        assert_eq!(dollars_to_cents(Some("0.005")), Some(1)); // 0.5c rounds to 1
        assert_eq!(dollars_to_cents(Some("0.004")), Some(0));
    }

    #[test]
    fn dollars_to_cents_handles_missing_and_bad() {
        assert_eq!(dollars_to_cents(None), None);
        assert_eq!(dollars_to_cents(Some("")), None);
        assert_eq!(dollars_to_cents(Some("not-a-number")), None);
    }

    #[test]
    fn dollars_to_cents_clamps_out_of_domain_and_rejects_non_finite() {
        // Now delegates to the shared clamped converter: a bad secmaster row can
        // never produce an out-of-domain immutable settlement object.
        assert_eq!(dollars_to_cents(Some("-0.5000")), Some(0)); // negative -> 0
        assert_eq!(dollars_to_cents(Some("2.5000")), Some(100)); // > $1 -> 100
        assert_eq!(dollars_to_cents(Some("9e99")), Some(100)); // absurd -> 100
        assert_eq!(dollars_to_cents(Some("inf")), None); // non-finite -> None
        assert_eq!(dollars_to_cents(Some("nan")), None);
        assert_eq!(dollars_to_cents(Some("")), None);
    }

    #[test]
    fn record_from_row_clamps_out_of_domain_secmaster_prices() {
        let mut row = sample_row();
        row.yes_bid_dollars = Some("-0.5000".to_string()); // negative
        row.yes_ask_dollars = Some("2.5000".to_string()); // > $1.00
        row.last_price_dollars = Some("9e99".to_string()); // absurd magnitude
        row.no_bid_dollars = Some("inf".to_string()); // non-finite
        row.volume = Some(-1000); // impossible negative count
        row.open_interest = Some(-5);
        let rec = record_from_row(&row, 0);
        assert_eq!(rec.final_yes_bid, Some(0)); // clamped low
        assert_eq!(rec.final_yes_ask, Some(100)); // clamped high
        assert_eq!(rec.final_last, Some(100)); // clamped high
        assert_eq!(rec.final_no_bid, None); // non-finite dropped
        assert_eq!(rec.final_volume, Some(0)); // floored at 0
        assert_eq!(rec.final_open_interest, Some(0));
        // Every present price cent stays within the valid Kalshi domain.
        for v in [
            rec.final_yes_bid,
            rec.final_yes_ask,
            rec.final_no_bid,
            rec.final_no_ask,
            rec.final_last,
        ]
        .into_iter()
        .flatten()
        {
            assert!((0..=100).contains(&v), "price cent {v} out of range");
        }
    }

    #[test]
    fn ts_text_to_epoch_parses_rfc3339() {
        // 2026-06-03 14:15:00 UTC
        assert_eq!(
            ts_text_to_epoch(Some("2026-06-03T14:15:00+00:00")),
            Some(1780496100)
        );
        assert_eq!(ts_text_to_epoch(None), None);
        assert_eq!(ts_text_to_epoch(Some("garbage")), None);
    }

    #[test]
    fn record_from_row_uses_secmaster_source_and_cents() {
        let rec = record_from_row(&sample_row(), 1780496200000);
        assert_eq!(rec.snap_source, SnapSource::Secmaster);
        assert_eq!(rec.market_ticker, "KXBTC15M-26JUN031415-15");
        assert_eq!(rec.series_ticker, "KXBTC15M");
        assert_eq!(rec.coin, "BTC");
        assert_eq!(rec.result.as_deref(), Some("yes"));
        // dollars converted to cents
        assert_eq!(rec.final_yes_bid, Some(96));
        assert_eq!(rec.final_last, Some(97));
        assert_eq!(rec.final_no_bid, Some(2));
        assert_eq!(rec.final_volume, Some(1000));
        // close_ts used as determination partition key
        assert_eq!(rec.determination_ts, Some(1780496100));
        assert_eq!(rec.nats_lifecycle_seq, -1);
    }

    #[test]
    fn record_from_row_path_partitions_by_close_date() {
        let rec = record_from_row(&sample_row(), 1780496200000);
        assert_eq!(
            object_path(&rec),
            "settled/kalshi/crypto/2026-06-03/BTC/KXBTC15M-26JUN031415-15.json"
        );
    }

    #[test]
    fn record_from_row_tolerates_missing_prices() {
        let mut row = sample_row();
        row.last_price_dollars = None;
        row.yes_bid_dollars = None;
        let rec = record_from_row(&row, 0);
        assert_eq!(rec.final_last, None);
        assert_eq!(rec.final_yes_bid, None);
        // record is still built with the label intact
        assert_eq!(rec.result.as_deref(), Some("yes"));
        assert_eq!(rec.snap_source, SnapSource::Secmaster);
    }
}
