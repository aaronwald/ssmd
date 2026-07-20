//! Idempotent per-market GCS writer using `object_store` conditional-create.
//!
//! Each settled market produces exactly one immutable JSON object. Redelivery
//! and consumer restart are absorbed by `PutMode::Create` (first writer wins).

use std::sync::Arc;

use anyhow::Result;
use chrono::{DateTime, Utc};
use object_store::gcp::GoogleCloudStorageBuilder;
use object_store::{ObjectStore, ObjectStoreExt, PutMode, PutOptions, PutPayload, UpdateVersion};

use crate::metrics;
use crate::record::SettlementRecord;

/// Outcome of a conditional write.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WriteOutcome {
    /// The object was created by this call.
    Written,
    /// The object already existed; this call was a no-op (idempotent).
    Exists,
    /// A lower-fidelity null-price object existed and was replaced by this
    /// higher-fidelity record (fidelity-ranked conditional update).
    Replaced,
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

    /// Write the record, replacing an existing object ONLY when this record is
    /// strictly higher fidelity (`snap_source.rank()`) than the stored one, the
    /// stored object carries null prices (a `Missing`/reconcile placeholder), AND
    /// this record actually carries prices (a genuine upgrade).
    ///
    /// This closes the "first-writer-wins freezes a bad object" gap in the
    /// immutable archive without ever downgrading a good snap:
    /// - Absent → [`WriteOutcome::Written`] via `PutMode::Create` (no read on the
    ///   common first-write path).
    /// - Existing, and `rec` outranks it AND existing has null prices AND `rec`
    ///   has real prices → an atomic conditional [`PutMode::Update`] (etag/version
    ///   guarded for race safety) → [`WriteOutcome::Replaced`].
    /// - Otherwise (equal/lower rank, existing already has real prices, or `rec`
    ///   itself carries only null prices) → [`WriteOutcome::Exists`] (idempotent
    ///   no-op, never a downgrade, never a needless rewrite).
    ///
    /// Safety / robustness:
    /// - If the existing object fails to deserialize (corrupt or foreign), it is
    ///   NOT clobbered — a warning is logged, the dedicated
    ///   `ssmd_settlement_corrupt_existing_total` metric is incremented (so DQ can
    ///   alert on the archive gap), and [`WriteOutcome::Exists`] is returned.
    /// - If the conditional update loses a race (precondition failure), the
    ///   object is re-read ONCE and the decision re-made; a second conflict
    ///   returns [`WriteOutcome::Exists`] (no clobber, no unbounded retry).
    /// - Any other store error propagates as `Err` (caller must NOT ack —
    ///   crash-cascade policy).
    pub async fn write_if_higher_fidelity(&self, rec: &SettlementRecord) -> Result<WriteOutcome> {
        let path = object_store::path::Path::from(object_path(rec));
        let body = serde_json::to_vec(rec)?;

        // Fast path: conditional create. The overwhelmingly common first write
        // succeeds here with no read.
        let create_opts = PutOptions {
            mode: PutMode::Create,
            ..Default::default()
        };
        match self
            .store
            .put_opts(&path, PutPayload::from(body.clone()), create_opts)
            .await
        {
            Ok(_) => return Ok(WriteOutcome::Written),
            Err(object_store::Error::AlreadyExists { .. }) => {}
            Err(e) => return Err(e.into()),
        }

        // Object exists. Read → decide → conditionally replace, bounded to two
        // attempts (initial + one re-read on a lost race).
        for _ in 0..2 {
            let existing = match self.store.get(&path).await {
                Ok(res) => res,
                // Raced with a delete between our create-fail and this get. Do not
                // recreate/clobber; treat as a no-op.
                Err(object_store::Error::NotFound { .. }) => return Ok(WriteOutcome::Exists),
                Err(e) => return Err(e.into()),
            };
            let version = UpdateVersion {
                e_tag: existing.meta.e_tag.clone(),
                version: existing.meta.version.clone(),
            };
            let bytes = existing.bytes().await?;
            let existing_rec: SettlementRecord = match serde_json::from_slice(&bytes) {
                Ok(r) => r,
                Err(e) => {
                    // Corrupt or foreign object: never clobber, never panic. Emit
                    // a dedicated metric so DQ can alert on the archive gap — the
                    // incoming record is acked and dropped here, otherwise masked
                    // as a normal `exists` no-op.
                    metrics::inc_corrupt_existing(&rec.coin);
                    tracing::warn!(
                        path = %path,
                        error = %e,
                        "existing settlement object failed to deserialize; leaving it untouched"
                    );
                    return Ok(WriteOutcome::Exists);
                }
            };

            // Replace only when this is a genuine price upgrade: strictly higher
            // fidelity source, the existing object carries null prices, AND the
            // incoming record actually carries prices. The last clause prevents a
            // null Secmaster record from needlessly rewriting a null Missing
            // object (rank-higher but no new information).
            let should_replace = rec.snap_source.rank() > existing_rec.snap_source.rank()
                && existing_rec.has_null_snap_prices()
                && !rec.has_null_snap_prices();
            if !should_replace {
                return Ok(WriteOutcome::Exists);
            }

            // Preserve any label field the incoming record lacks — a `Memory`
            // record built from a `settled` event carries no result /
            // settlement_value / determination_ts, and a whole-object write would
            // otherwise regress those non-null labels (set by the earlier
            // `determined` event) to null. Merge upgrades the prices while keeping
            // the labels.
            let merged = rec.merged_preserving_labels(&existing_rec);
            let merged_body = serde_json::to_vec(&merged)?;
            let update_opts = PutOptions {
                mode: PutMode::Update(version),
                ..Default::default()
            };
            match self
                .store
                .put_opts(&path, PutPayload::from(merged_body), update_opts)
                .await
            {
                Ok(_) => return Ok(WriteOutcome::Replaced),
                // Lost the race (someone wrote between our get and put). Re-read
                // once and re-decide; the loop bound guarantees no clobber-storm.
                Err(object_store::Error::Precondition { .. }) => continue,
                Err(e) => return Err(e.into()),
            }
        }

        // Two consecutive precondition failures — a persistent race. Do not
        // clobber; report a no-op.
        Ok(WriteOutcome::Exists)
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

    // --- fidelity-ranked conditional-replace tests ---

    fn trigger() -> SettlementTrigger {
        SettlementTrigger {
            market_ticker: "KXBTC15M-26JUN031400-15".to_string(),
            event_ticker: Some("KXBTC15M-26JUN031400".to_string()),
            result: Some("yes".to_string()),
            settlement_value: Some(100),
            close_ts: Some(1780496100),
            determination_ts: Some(1780496105),
            settled_ts: None,
            nats_lifecycle_seq: 1,
        }
    }

    fn real_tick() -> LastTick {
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

    /// Build a record at the SAME object path with a given snap source and
    /// either real (non-null) or null price fields.
    fn rec_with(source: SnapSource, real_prices: bool) -> SettlementRecord {
        let tick = if real_prices { Some(real_tick()) } else { None };
        SettlementRecord::build_with_source(&trigger(), tick, source, 1780496106000)
    }

    fn writer() -> (GcsWriter, Arc<object_store::memory::InMemory>) {
        let store = Arc::new(object_store::memory::InMemory::new());
        (GcsWriter::with_store(store.clone()), store)
    }

    async fn read_back(
        store: &Arc<object_store::memory::InMemory>,
        path: &str,
    ) -> SettlementRecord {
        let p = object_store::path::Path::from(path);
        let bytes = store.get(&p).await.unwrap().bytes().await.unwrap();
        serde_json::from_slice(&bytes).unwrap()
    }

    #[tokio::test]
    async fn writes_when_absent() {
        let (w, _store) = writer();
        let out = w
            .write_if_higher_fidelity(&rec_with(SnapSource::Secmaster, false))
            .await
            .unwrap();
        assert_eq!(out, WriteOutcome::Written);
    }

    #[tokio::test]
    async fn replaces_null_secmaster_with_memory() {
        let (w, store) = writer();
        let path = object_path(&rec_with(SnapSource::Secmaster, false));

        let first = w
            .write_if_higher_fidelity(&rec_with(SnapSource::Secmaster, false))
            .await
            .unwrap();
        assert_eq!(first, WriteOutcome::Written);

        let second = w
            .write_if_higher_fidelity(&rec_with(SnapSource::Memory, true))
            .await
            .unwrap();
        assert_eq!(second, WriteOutcome::Replaced);

        let back = read_back(&store, &path).await;
        assert_eq!(back.snap_source, SnapSource::Memory);
        assert_eq!(back.final_last, Some(97));
        assert!(!back.has_null_snap_prices());
    }

    #[tokio::test]
    async fn replace_from_settled_preserves_determined_labels() {
        // Codex HIGH regression: a `determined` event with no in-memory tick
        // writes a Missing object that STILL carries result / settlement_value /
        // determination_ts. A later `settled` event (no labels) that now has a
        // Memory tick must upgrade the PRICES without regressing those labels to
        // null (a whole-object write would otherwise clobber the label).
        let (w, store) = writer();
        let path = object_path(&rec_with(SnapSource::Missing, false));

        // Existing: Missing, null prices, but WITH determined labels.
        let existing = rec_with(SnapSource::Missing, false);
        assert_eq!(existing.result.as_deref(), Some("yes"));
        assert!(existing.has_null_snap_prices());
        w.write_if_higher_fidelity(&existing).await.unwrap();

        // Incoming: Memory (real prices) built from a `settled`-only trigger — NO
        // result / settlement_value / determination_ts.
        let settled_trigger = SettlementTrigger {
            market_ticker: "KXBTC15M-26JUN031400-15".to_string(),
            event_ticker: None,
            result: None,
            settlement_value: None,
            close_ts: None,
            determination_ts: None,
            settled_ts: Some(1780496105), // same date → same object path
            nats_lifecycle_seq: 2,
        };
        let incoming = SettlementRecord::build_with_source(
            &settled_trigger,
            Some(real_tick()),
            SnapSource::Memory,
            1780496110000,
        );
        assert!(incoming.result.is_none());

        let out = w.write_if_higher_fidelity(&incoming).await.unwrap();
        assert_eq!(out, WriteOutcome::Replaced);

        let back = read_back(&store, &path).await;
        // Prices upgraded to the Memory tick ...
        assert_eq!(back.snap_source, SnapSource::Memory);
        assert_eq!(back.final_last, Some(97));
        assert!(!back.has_null_snap_prices());
        // ... AND the determined labels are preserved, not regressed to null.
        assert_eq!(back.result.as_deref(), Some("yes"));
        assert_eq!(back.settlement_value, Some(100));
        assert_eq!(back.determination_ts, Some(1780496105));
        assert_eq!(back.event_ticker.as_deref(), Some("KXBTC15M-26JUN031400"));
    }

    #[tokio::test]
    async fn replaces_secmaster_with_null_prices_but_volume() {
        // Codex HIGH regression: a Secmaster object with null bid/ask/last PRICES
        // but non-null volume / open_interest is still price-less, so a later
        // Memory record with real prices MUST replace it (volume/OI are counts,
        // not prices, and must not freeze the upgrade path).
        let (w, store) = writer();
        let path = object_path(&rec_with(SnapSource::Secmaster, false));

        let mut existing = rec_with(SnapSource::Secmaster, false);
        existing.final_volume = Some(1234);
        existing.final_open_interest = Some(567);
        assert!(existing.has_null_snap_prices());
        w.write_if_higher_fidelity(&existing).await.unwrap();

        let out = w
            .write_if_higher_fidelity(&rec_with(SnapSource::Memory, true))
            .await
            .unwrap();
        assert_eq!(out, WriteOutcome::Replaced);

        let back = read_back(&store, &path).await;
        assert_eq!(back.snap_source, SnapSource::Memory);
        assert_eq!(back.final_last, Some(97));
    }

    #[tokio::test]
    async fn replaces_null_missing_with_memory() {
        let (w, store) = writer();
        let path = object_path(&rec_with(SnapSource::Missing, false));

        w.write_if_higher_fidelity(&rec_with(SnapSource::Missing, false))
            .await
            .unwrap();
        let out = w
            .write_if_higher_fidelity(&rec_with(SnapSource::Memory, true))
            .await
            .unwrap();
        assert_eq!(out, WriteOutcome::Replaced);

        let back = read_back(&store, &path).await;
        assert_eq!(back.snap_source, SnapSource::Memory);
        assert!(!back.has_null_snap_prices());
    }

    #[tokio::test]
    async fn never_downgrades_memory_with_secmaster() {
        let (w, store) = writer();
        let path = object_path(&rec_with(SnapSource::Memory, true));

        w.write_if_higher_fidelity(&rec_with(SnapSource::Memory, true))
            .await
            .unwrap();
        let out = w
            .write_if_higher_fidelity(&rec_with(SnapSource::Secmaster, false))
            .await
            .unwrap();
        assert_eq!(out, WriteOutcome::Exists);

        let back = read_back(&store, &path).await;
        assert_eq!(back.snap_source, SnapSource::Memory);
        assert!(!back.has_null_snap_prices());
    }

    #[tokio::test]
    async fn memory_then_memory_is_idempotent_noop() {
        let (w, store) = writer();
        let path = object_path(&rec_with(SnapSource::Memory, true));

        w.write_if_higher_fidelity(&rec_with(SnapSource::Memory, true))
            .await
            .unwrap();
        let out = w
            .write_if_higher_fidelity(&rec_with(SnapSource::Memory, true))
            .await
            .unwrap();
        // Same rank -> not strictly greater -> no-op.
        assert_eq!(out, WriteOutcome::Exists);

        let back = read_back(&store, &path).await;
        assert_eq!(back.snap_source, SnapSource::Memory);
    }

    #[tokio::test]
    async fn does_not_replace_non_null_secmaster() {
        // Guard: only null-price objects may be replaced. A Secmaster object that
        // carries real prices must NOT be overwritten even by higher-rank Memory.
        let (w, store) = writer();
        let path = object_path(&rec_with(SnapSource::Secmaster, true));

        w.write_if_higher_fidelity(&rec_with(SnapSource::Secmaster, true))
            .await
            .unwrap();
        let out = w
            .write_if_higher_fidelity(&rec_with(SnapSource::Memory, true))
            .await
            .unwrap();
        assert_eq!(out, WriteOutcome::Exists);

        let back = read_back(&store, &path).await;
        assert_eq!(back.snap_source, SnapSource::Secmaster);
    }

    #[tokio::test]
    async fn does_not_replace_null_missing_with_null_secmaster() {
        // A higher-rank record that ALSO carries only null prices is not a genuine
        // upgrade: replacing a null Missing object with a null Secmaster object
        // adds no information and must be a no-op.
        let (w, store) = writer();
        let path = object_path(&rec_with(SnapSource::Missing, false));

        w.write_if_higher_fidelity(&rec_with(SnapSource::Missing, false))
            .await
            .unwrap();
        let out = w
            .write_if_higher_fidelity(&rec_with(SnapSource::Secmaster, false))
            .await
            .unwrap();
        assert_eq!(out, WriteOutcome::Exists);

        // The original Missing placeholder is untouched (not needlessly rewritten).
        let back = read_back(&store, &path).await;
        assert_eq!(back.snap_source, SnapSource::Missing);
        assert!(back.has_null_snap_prices());
    }

    #[tokio::test]
    async fn replaces_null_missing_with_real_memory_still_works() {
        // The genuine-upgrade path still replaces: null Missing → Memory carrying
        // real prices.
        let (w, store) = writer();
        let path = object_path(&rec_with(SnapSource::Missing, false));

        w.write_if_higher_fidelity(&rec_with(SnapSource::Missing, false))
            .await
            .unwrap();
        let out = w
            .write_if_higher_fidelity(&rec_with(SnapSource::Memory, true))
            .await
            .unwrap();
        assert_eq!(out, WriteOutcome::Replaced);

        let back = read_back(&store, &path).await;
        assert_eq!(back.snap_source, SnapSource::Memory);
        assert!(!back.has_null_snap_prices());
    }

    #[tokio::test]
    async fn corrupt_existing_object_is_not_clobbered() {
        let (w, store) = writer();
        let rec = rec_with(SnapSource::Memory, true);
        let path = object_store::path::Path::from(object_path(&rec));

        // Seed a corrupt (non-JSON) object at the path.
        store
            .put(&path, PutPayload::from(b"not-json{{{".to_vec()))
            .await
            .unwrap();

        let out = w.write_if_higher_fidelity(&rec).await.unwrap();
        assert_eq!(out, WriteOutcome::Exists);

        // Corrupt bytes remain untouched (no clobber).
        let bytes = store.get(&path).await.unwrap().bytes().await.unwrap();
        assert_eq!(&bytes[..], b"not-json{{{");

        // The corrupt-object path emitted the dedicated alertable metric (labelled
        // by the incoming record's coin), rather than a silent `exists` no-op.
        let output = crate::metrics::encode_metrics().expect("encode metrics");
        assert!(output.contains("ssmd_settlement_corrupt_existing_total"));
        assert!(output.contains("coin=\"BTC\""));
    }
}
