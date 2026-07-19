// The `#[path]`-included foundation modules expose more API than this single
// test binary exercises (e.g. `GcsWriter::from_env`, the Redis snap source);
// that surface is intentional and covered elsewhere.
#![allow(dead_code)]

//! Integration test for the record-build + GCS-write seam (Task 9).
//!
//! Drives the same path the lifecycle consumer takes — parse a `determined`
//! lifecycle message, build a labeled record from a matching last tick, and
//! write it conditionally — but against an in-memory `ObjectStore` so no NATS
//! or real GCS is required. The crate is a binary, so its modules are not a
//! library; the test re-declares the small set of modules it needs via
//! `#[path]` includes, exactly as the binary wires them.

#[path = "../src/symbology.rs"]
mod symbology;

// `lifecycle::settlement_value` deserialization reuses `crate::price`, so the
// price module must be declared at this crate root for that path to resolve.
#[path = "../src/price.rs"]
mod price;

#[path = "../src/lifecycle.rs"]
mod lifecycle;

#[path = "../src/ticker.rs"]
mod ticker;

#[path = "../src/record.rs"]
mod record;

#[path = "../src/gcs.rs"]
mod gcs;

use std::sync::Arc;

use object_store::memory::InMemory;
use object_store::{ObjectStore, ObjectStoreExt};

use gcs::{object_path, GcsWriter, WriteOutcome};
use lifecycle::{is_settlement_trigger, parse};
use record::{SettlementRecord, SettlementTrigger, SnapSource};
use symbology::is_15m;
use ticker::{LastTick, LastTickMap};

/// A realistic `determined` lifecycle event for a 15-minute BTC market.
/// determination_ts = 1780496105 → 2026-06-03 14:15:05 UTC.
const DETERMINED_JSON: &str = r#"{
    "type": "market_lifecycle_v2",
    "sid": 7,
    "msg": {
        "market_ticker": "KXBTC15M-26JUN031415-15",
        "event_ticker": "KXBTC15M-26JUN031415",
        "event_type": "determined",
        "close_ts": 1780496100,
        "determination_ts": 1780496105,
        "settled_ts": 1780496160,
        "result": "yes",
        "settlement_value": 100
    }
}"#;

fn matching_tick() -> LastTick {
    LastTick {
        yes_bid: Some(96),
        yes_ask: Some(98),
        no_bid: Some(2),
        no_ask: Some(4),
        last_price: Some(97),
        volume: Some(1000),
        open_interest: Some(500),
        ts: 1780496100,
    }
}

/// Build a record from a parsed lifecycle message + the final tick, mirroring
/// the consumer's seam (memory source).
fn build_record(payload: &[u8], map: &LastTickMap) -> Option<SettlementRecord> {
    let lc = parse(payload)?;
    if !is_15m(&lc.market_ticker) || !is_settlement_trigger(&lc.event_type) {
        return None;
    }
    let trigger = SettlementTrigger::from_lifecycle(&lc, 42);
    let (tick, source) = match map.get(&lc.market_ticker) {
        Some(t) => (Some(t), SnapSource::Memory),
        None => (None, SnapSource::Missing),
    };
    Some(SettlementRecord::build_with_source(
        &trigger,
        tick,
        source,
        1780496106000,
    ))
}

#[tokio::test]
async fn determined_event_writes_one_correct_object() {
    let store = Arc::new(InMemory::new());
    let writer = GcsWriter::with_store(store.clone());

    let map = LastTickMap::new();
    map.update("KXBTC15M-26JUN031415-15", matching_tick());

    let record = build_record(DETERMINED_JSON.as_bytes(), &map).expect("should build record");

    // Write the record.
    let outcome = writer.write_if_absent(&record).await.expect("write ok");
    assert_eq!(outcome, WriteOutcome::Written);

    // Exactly one object at the expected path.
    let expected_path = "settled/kalshi/crypto/2026-06-03/BTC/KXBTC15M-26JUN031415-15.json";
    assert_eq!(object_path(&record), expected_path);

    let path = object_store::path::Path::from(expected_path);
    let bytes = store
        .get(&path)
        .await
        .expect("object present")
        .bytes()
        .await
        .unwrap();
    let json: serde_json::Value = serde_json::from_slice(&bytes).expect("valid json");

    assert_eq!(json["market_ticker"], "KXBTC15M-26JUN031415-15");
    assert_eq!(json["series_ticker"], "KXBTC15M");
    assert_eq!(json["coin"], "BTC");
    assert_eq!(json["result"], "yes");
    assert_eq!(json["settlement_value"], 100);
    assert_eq!(json["final_yes_bid"], 96);
    assert_eq!(json["final_last"], 97);
    assert_eq!(json["final_ticker_ts"], 1780496100);
    // (1780496105 - 1780496100) * 1000 = 5000
    assert_eq!(json["snap_age_ms"], 5000);
    assert_eq!(json["snap_source"], "memory");
    assert_eq!(json["schema_version"], 1);
}

#[tokio::test]
async fn second_write_is_idempotent_no_duplicate() {
    let store = Arc::new(InMemory::new());
    let writer = GcsWriter::with_store(store.clone());

    let map = LastTickMap::new();
    map.update("KXBTC15M-26JUN031415-15", matching_tick());
    let record = build_record(DETERMINED_JSON.as_bytes(), &map).expect("should build record");

    assert_eq!(
        writer.write_if_absent(&record).await.expect("first write"),
        WriteOutcome::Written
    );
    // Redelivery / restart → same path, conditional-create returns Exists.
    assert_eq!(
        writer.write_if_absent(&record).await.expect("second write"),
        WriteOutcome::Exists
    );

    // Still exactly one object under the prefix.
    let prefix = object_store::path::Path::from("settled/kalshi/crypto");
    let listed = store.list_with_delimiter(Some(&prefix)).await;
    // Count all objects under the date partition.
    use futures_util::StreamExt;
    let count = store.list(None).count().await;
    assert_eq!(count, 1, "expected exactly one object, listing: {listed:?}");
}

#[tokio::test]
async fn non_15m_event_produces_no_object() {
    let store = Arc::new(InMemory::new());
    let writer = GcsWriter::with_store(store.clone());

    // A non-15M crypto market (hourly KXBTCD), determined.
    let hourly = r#"{
        "type": "market_lifecycle_v2",
        "msg": {
            "market_ticker": "KXBTCD-26JUN0314-T100000",
            "event_type": "determined",
            "determination_ts": 1780496105,
            "result": "no",
            "settlement_value": 0
        }
    }"#;

    let map = LastTickMap::new();
    let record = build_record(hourly.as_bytes(), &map);
    assert!(record.is_none(), "non-15M event must not build a record");

    // Nothing should have been written (we never call write).
    use futures_util::StreamExt;
    let count = store.list(None).count().await;
    assert_eq!(count, 0);
    // writer is unused beyond construction; keep it referenced.
    let _ = &writer;
}
