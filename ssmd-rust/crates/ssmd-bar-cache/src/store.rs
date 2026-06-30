//! Redis ring writer for 1-minute OHLCV bars.
//!
//! Each `(feed, symbol)` pair keeps the most recent `ring_len` [`Bar`]s under a
//! single key `ohlcv_1m:{feed}:{sym}`, stored as a JSON array ordered oldest →
//! newest. Writes are a read-modify-write: GET the existing array, fold in the
//! new (or revised forming-minute) bar via the pure [`apply_bar`] transform,
//! then `SET` + `EXPIRE` in one pipelined round trip.
//!
//! The array transform is factored out as [`apply_bar`] so the OHLCV ring
//! semantics (in-place replace of the forming minute, append + trim, ordering)
//! are unit-tested without a live Redis.

use tracing::warn;

use crate::agg::Bar;

/// Build the Redis key for a feed/symbol ring.
pub fn ring_key(feed: &str, sym: &str) -> String {
    format!("ohlcv_1m:{feed}:{sym}")
}

/// Fold `bar` into `existing`, returning the new ring contents.
///
/// Pure and side-effect free:
/// - If a bar with the same `start_ts_ms` is already present, it is replaced in
///   place (the forming minute updates without growing the ring).
/// - Otherwise the bar is appended.
/// - The result is sorted ascending by `start_ts_ms` and trimmed to the newest
///   `ring_len` entries.
pub fn apply_bar(mut existing: Vec<Bar>, bar: Bar, ring_len: usize) -> Vec<Bar> {
    match existing
        .iter_mut()
        .find(|b| b.start_ts_ms == bar.start_ts_ms)
    {
        Some(slot) => *slot = bar,
        None => existing.push(bar),
    }

    existing.sort_by_key(|b| b.start_ts_ms);

    // Keep only the newest `ring_len` bars (drop from the front / oldest).
    if existing.len() > ring_len {
        let overflow = existing.len() - ring_len;
        existing.drain(0..overflow);
    }

    existing
}

/// Read-modify-write a single bar into the feed/symbol ring in Redis.
///
/// Pipelines `SET` + `EXPIRE` so the key never lingers without a TTL. The pure
/// ring math lives in [`apply_bar`]; this wrapper only does the I/O and the
/// JSON (de)serialization, so it is intentionally thin and left to integration
/// coverage (no test Redis is available in this crate).
pub async fn upsert_bar(
    conn: &redis::aio::MultiplexedConnection,
    feed: &str,
    ring_len: usize,
    ttl_secs: u64,
    bar: Bar,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let key = ring_key(feed, &bar.sym);
    let mut conn = conn.clone();

    let existing_raw: Option<Vec<u8>> = redis::cmd("GET")
        .arg(&key)
        .query_async(&mut conn)
        .await
        .map_err(|e| format!("GET {key}: {e}"))?;

    // Decode the existing ring. A corrupt/legacy value is recoverable: we log
    // loudly (so the signal is never silent) and start a fresh ring, then
    // overwrite the bad key on the SET below. Crashing the consumer over one
    // poisoned key would stall every symbol on the feed, which is worse.
    let existing: Vec<Bar> = match existing_raw {
        Some(bytes) => match serde_json::from_slice(&bytes) {
            Ok(bars) => bars,
            Err(e) => {
                warn!(
                    %key,
                    error = %e,
                    bytes = bytes.len(),
                    "corrupt ohlcv_1m ring value; reinitializing ring from this bar"
                );
                Vec::new()
            }
        },
        None => Vec::new(),
    };

    let updated = apply_bar(existing, bar, ring_len);
    let encoded = serde_json::to_vec(&updated).map_err(|e| format!("encode {key}: {e}"))?;

    redis::pipe()
        .set(&key, encoded)
        .expire(&key, ttl_secs as i64)
        .query_async::<()>(&mut conn)
        .await
        .map_err(|e| format!("SET/EXPIRE {key}: {e}"))?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn bar(start_ts_ms: i64, v: f64) -> Bar {
        Bar {
            sym: "BTC/USD".to_string(),
            o: 1.0,
            h: 2.0,
            l: 0.5,
            c: 1.5,
            v,
            trade_count: 0,
            taker_buy_volume: 0.0,
            taker_sell_volume: 0.0,
            market_order_volume: 0.0,
            quote_volume: 0.0,
            start_ts_ms,
            end_ts_ms: start_ts_ms + 60_000,
        }
    }

    #[test]
    fn key_schema() {
        assert_eq!(
            ring_key("kraken-spot", "BTC/USD"),
            "ohlcv_1m:kraken-spot:BTC/USD"
        );
        assert_eq!(ring_key("massive", "AAPL"), "ohlcv_1m:massive:AAPL");
    }

    #[test]
    fn appends_into_empty_ring() {
        let ring = apply_bar(Vec::new(), bar(0, 1.0), 60);
        assert_eq!(ring.len(), 1);
        assert_eq!(ring[0].start_ts_ms, 0);
    }

    #[test]
    fn sixty_one_minutes_keeps_newest_sixty_in_order() {
        let mut ring: Vec<Bar> = Vec::new();
        for i in 0..61i64 {
            ring = apply_bar(ring, bar(i * 60_000, i as f64), 60);
        }
        assert_eq!(ring.len(), 60, "trimmed to ring_len");
        // Oldest (minute 0) dropped; newest 60 are minutes 1..=60, ascending.
        assert_eq!(ring.first().unwrap().start_ts_ms, 60_000);
        assert_eq!(ring.last().unwrap().start_ts_ms, 60 * 60_000);
        for (idx, b) in ring.iter().enumerate() {
            assert_eq!(b.start_ts_ms, (idx as i64 + 1) * 60_000);
        }
    }

    #[test]
    fn reupsert_same_start_ts_replaces_in_place() {
        let ring = apply_bar(Vec::new(), bar(0, 1.0), 60);
        let ring = apply_bar(ring, bar(60_000, 2.0), 60);
        // Forming minute (start 60_000) updates its volume — len unchanged.
        let ring = apply_bar(ring, bar(60_000, 9.0), 60);

        assert_eq!(ring.len(), 2, "replace must not grow the ring");
        assert_eq!(ring[0].start_ts_ms, 0);
        assert_eq!(ring[1].start_ts_ms, 60_000);
        assert_eq!(ring[1].v, 9.0, "latest value wins for same start_ts_ms");
    }

    #[test]
    fn out_of_order_insert_sorts_ascending() {
        let ring = apply_bar(Vec::new(), bar(120_000, 1.0), 60);
        let ring = apply_bar(ring, bar(0, 2.0), 60);
        let ring = apply_bar(ring, bar(60_000, 3.0), 60);

        let starts: Vec<i64> = ring.iter().map(|b| b.start_ts_ms).collect();
        assert_eq!(starts, vec![0, 60_000, 120_000]);
    }

    #[test]
    fn replace_then_overflow_still_trims_correctly() {
        let mut ring: Vec<Bar> = Vec::new();
        for i in 0..60i64 {
            ring = apply_bar(ring, bar(i * 60_000, i as f64), 60);
        }
        // Re-upsert an existing minute (no growth) ...
        ring = apply_bar(ring, bar(0, 100.0), 60);
        assert_eq!(ring.len(), 60);
        assert_eq!(ring[0].v, 100.0);
        // ... then push a brand new minute, which should evict the oldest.
        ring = apply_bar(ring, bar(60 * 60_000, 1.0), 60);
        assert_eq!(ring.len(), 60);
        assert_eq!(
            ring.first().unwrap().start_ts_ms,
            60_000,
            "minute 0 evicted"
        );
        assert_eq!(ring.last().unwrap().start_ts_ms, 60 * 60_000);
    }
}
