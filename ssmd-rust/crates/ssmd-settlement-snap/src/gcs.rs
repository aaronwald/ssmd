//! Idempotent per-market GCS writer using `object_store` conditional-create.
//!
//! Each settled market produces exactly one immutable JSON object. Redelivery
//! and consumer restart are absorbed by `PutMode::Create` (first writer wins).

use std::sync::Arc;

use anyhow::Result;
use chrono::{DateTime, Utc};
use object_store::gcp::GoogleCloudStorageBuilder;
use object_store::{ObjectStore, PutMode, PutOptions, PutPayload};

use crate::record::SettlementRecord;

/// Outcome of a conditional write.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WriteOutcome {
    /// The object was created by this call.
    Written,
    /// The object already existed; this call was a no-op (idempotent).
    Exists,
}

/// Build the immutable object path for a settled record:
/// `settled/kalshi/crypto/{settle_date}/{coin}/{market_ticker}.json`
/// where `settle_date` is the UTC date of `determination_ts`, falling back to
/// `settled_ts` when a `settled`-only trigger carries no `determination_ts`.
/// Only when BOTH are absent does the date partition under `unknown-date`, so
/// the record is never silently dropped and is easy to spot in a LIST.
pub fn object_path(rec: &SettlementRecord) -> String {
    let settle_date = rec
        .determination_ts
        .or(rec.settled_ts)
        .and_then(|ts| DateTime::<Utc>::from_timestamp(ts, 0))
        .map(|dt| dt.format("%Y-%m-%d").to_string())
        .unwrap_or_else(|| "unknown-date".to_string());

    format!(
        "settled/kalshi/crypto/{}/{}/{}.json",
        settle_date, rec.coin, rec.market_ticker
    )
}

/// Writes settlement records to an object store with conditional-create
/// semantics. The store is injectable so tests can use `InMemory`.
pub struct GcsWriter {
    store: Arc<dyn ObjectStore>,
}

impl GcsWriter {
    /// Build a writer backed by GCS, using Workload Identity or
    /// `GOOGLE_APPLICATION_CREDENTIALS` from the environment.
    pub fn from_env(bucket: &str) -> Result<Self> {
        let store = GoogleCloudStorageBuilder::from_env()
            .with_bucket_name(bucket)
            .build()?;
        Ok(Self {
            store: Arc::new(store),
        })
    }

    /// Build a writer over an arbitrary `ObjectStore` (test injection seam,
    /// e.g. `object_store::memory::InMemory`).
    pub fn with_store(store: Arc<dyn ObjectStore>) -> Self {
        Self { store }
    }

    /// Write the record as an immutable JSON object iff one does not already
    /// exist at its path. Returns [`WriteOutcome::Written`] on create and
    /// [`WriteOutcome::Exists`] when the object was already present. Any other
    /// store error propagates (caller must NOT ack the lifecycle message on a
    /// non-precondition error — retry/crash per the crash-cascade policy).
    pub async fn write_if_absent(&self, rec: &SettlementRecord) -> Result<WriteOutcome> {
        let path = object_store::path::Path::from(object_path(rec));
        let body = serde_json::to_vec(rec)?;
        let opts = PutOptions {
            mode: PutMode::Create,
            ..Default::default()
        };
        match self
            .store
            .put_opts(&path, PutPayload::from(body), opts)
            .await
        {
            Ok(_) => Ok(WriteOutcome::Written),
            Err(object_store::Error::AlreadyExists { .. }) => Ok(WriteOutcome::Exists),
            Err(e) => Err(e.into()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::record::{SettlementRecord, SettlementTrigger, SnapSource};
    use crate::ticker::LastTick;

    fn record() -> SettlementRecord {
        let trigger = SettlementTrigger {
            market_ticker: "KXBTC15M-26JUN031400-15".to_string(),
            event_ticker: Some("KXBTC15M-26JUN031400".to_string()),
            result: Some("yes".to_string()),
            settlement_value: Some(100),
            close_ts: Some(1780496100),
            // 2026-06-03 14:15:05 UTC
            determination_ts: Some(1780496105),
            settled_ts: None,
            nats_lifecycle_seq: 1,
        };
        let tick = LastTick {
            yes_bid: Some(96),
            yes_ask: Some(98),
            no_bid: Some(2),
            no_ask: Some(4),
            last_price: Some(97),
            volume: Some(1000),
            open_interest: Some(500),
            ts: 1780496100,
        };
        SettlementRecord::build_with_source(&trigger, Some(tick), SnapSource::Memory, 1780496106000)
    }

    #[test]
    fn object_path_uses_determination_date_coin_and_ticker() {
        let rec = record();
        assert_eq!(
            object_path(&rec),
            "settled/kalshi/crypto/2026-06-03/BTC/KXBTC15M-26JUN031400-15.json"
        );
    }

    #[test]
    fn object_path_falls_back_to_settled_ts_date() {
        // A `settled`-only trigger has no determination_ts but carries settled_ts;
        // the partition date must come from settled_ts, NOT `unknown-date`.
        let mut rec = record();
        rec.determination_ts = None;
        rec.settled_ts = Some(1780496105); // 2026-06-03 14:15:05 UTC
        assert_eq!(
            object_path(&rec),
            "settled/kalshi/crypto/2026-06-03/BTC/KXBTC15M-26JUN031400-15.json"
        );
    }

    #[test]
    fn object_path_unknown_date_only_when_both_ts_absent() {
        let mut rec = record();
        rec.determination_ts = None;
        rec.settled_ts = None;
        assert_eq!(
            object_path(&rec),
            "settled/kalshi/crypto/unknown-date/BTC/KXBTC15M-26JUN031400-15.json"
        );
    }
}
