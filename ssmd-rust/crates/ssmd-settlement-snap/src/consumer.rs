//! The two NATS consumers (ephemeral LastPerSubject ticker + durable lifecycle
//! trigger) plus the run loop that wires the foundation modules together.
//!
//! Mirrors `ssmd-snap` for the LastPerSubject ticker consumer (AckPolicy::None,
//! `{type, sid, msg:{...}}` envelope unwrap) and `ssmd-cache` for the durable
//! JetStream lifecycle consumer (durable name, explicit ack).
//!
//! Crash-cascade compliance: the lifecycle message is acked ONLY after the GCS
//! object is written or confirmed present. A persistent GCS write failure does
//! NOT ack — the durable consumer redelivers — and after N consecutive failures
//! the process crashes so K8s restarts it.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Duration;

use anyhow::{anyhow, Context as _, Result};
use async_nats::jetstream::{self, consumer::pull::Config as PullConfig, Context};
use futures_util::StreamExt;

use crate::config::Config;
use crate::gcs::{GcsWriter, WriteOutcome};
use crate::lifecycle::{self, is_settlement_trigger};
use crate::reconcile;
use crate::record::{SettlementRecord, SettlementTrigger, SnapSource};
use crate::symbology::is_15m;
use crate::ticker::{LastTick, LastTickMap};

/// Max consecutive GCS write failures before we crash (let K8s restart).
const MAX_CONSECUTIVE_GCS_ERRORS: u32 = 5;
/// Max consecutive lifecycle receive errors before we restart the consumer.
const MAX_CONSECUTIVE_RECEIVE_ERRORS: u32 = 5;
/// Log a progress summary every N processed lifecycle messages.
const PROGRESS_EVERY: u64 = 100;

/// Running counters for the lifecycle consumer, surfaced in progress logs.
#[derive(Default)]
pub struct Progress {
    pub processed: AtomicU64,
    pub written: AtomicU64,
    pub exists: AtomicU64,
    pub redis_fallback: AtomicU64,
    pub missing: AtomicU64,
}

impl Progress {
    fn log(&self) {
        tracing::info!(
            processed = self.processed.load(Ordering::Relaxed),
            written = self.written.load(Ordering::Relaxed),
            exists = self.exists.load(Ordering::Relaxed),
            redis_fallback = self.redis_fallback.load(Ordering::Relaxed),
            missing = self.missing.load(Ordering::Relaxed),
            "settlement-snap progress",
        );
    }
}

/// Parse a ticker NATS payload (the `{type, sid, msg:{...}}` envelope or a flat
/// ticker object) into `(market_ticker, LastTick)`. Pure and unit-testable.
///
/// Mirrors the connector `TickerData` shape: prices are native cents, `ts` is a
/// Unix epoch-seconds integer (the connector serializes the `DateTime<Utc>` via
/// its `.timestamp()`), `last_price` accepts the `price` alias.
pub fn parse_ticker(payload: &[u8]) -> Option<(String, LastTick)> {
    let v: serde_json::Value = serde_json::from_slice(payload).ok()?;
    // Unwrap the connector envelope `{ "msg": { ... } }` if present; otherwise
    // treat the top-level object as the ticker body.
    let body = v.get("msg").unwrap_or(&v);
    let obj = body.as_object()?;

    let market_ticker = obj.get("market_ticker")?.as_str()?.to_string();

    let as_i64 = |key: &str| obj.get(key).and_then(|x| x.as_i64());

    // `ts` may arrive as an integer epoch-seconds or, defensively, a string.
    let ts = obj.get("ts").and_then(|x| {
        x.as_i64()
            .or_else(|| x.as_str().and_then(|s| s.parse().ok()))
    })?;

    let tick = LastTick {
        yes_bid: as_i64("yes_bid"),
        yes_ask: as_i64("yes_ask"),
        no_bid: as_i64("no_bid"),
        no_ask: as_i64("no_ask"),
        last_price: as_i64("last_price").or_else(|| as_i64("price")),
        volume: as_i64("volume"),
        open_interest: as_i64("open_interest"),
        ts,
    };
    Some((market_ticker, tick))
}

/// Parse a Redis snap value (written by `ssmd-snap`, same envelope shape with an
/// injected `_snap_at`) into a `LastTick`. Pure and unit-testable. Reuses
/// [`parse_ticker`] and discards the ticker key.
pub fn parse_snap_value(payload: &[u8]) -> Option<LastTick> {
    parse_ticker(payload).map(|(_, tick)| tick)
}

/// Spawn the ephemeral `LastPerSubject` ticker consumer. It owns no ack
/// responsibility (AckPolicy::None) and feeds the shared last-tick map. The
/// task loops forever, recreating the consumer on stream end / receive error.
fn spawn_ticker_task(js: Context, ticker_subject: String, map: Arc<LastTickMap>) {
    tokio::spawn(async move {
        loop {
            if let Err(e) = run_ticker_inner(&js, &ticker_subject, &map).await {
                tracing::warn!(error = %e, "ticker consumer error, restarting in 5s");
                tokio::time::sleep(Duration::from_secs(5)).await;
            } else {
                tracing::info!("ticker consumer stream ended, restarting");
            }
        }
    });
}

async fn run_ticker_inner(js: &Context, ticker_subject: &str, map: &LastTickMap) -> Result<()> {
    // The ticker subject `prod.kalshi.crypto.json.ticker.>` is carried on the
    // crypto stream. We discover the stream name from the subject so the service
    // has no extra config surface.
    let stream_name = js
        .stream_by_subject(ticker_subject.to_string())
        .await
        .with_context(|| format!("resolve ticker stream for {ticker_subject}"))?;
    let stream = js
        .get_stream(&stream_name)
        .await
        .with_context(|| format!("get ticker stream {stream_name}"))?;

    let consumer = stream
        .create_consumer(PullConfig {
            filter_subject: ticker_subject.to_string(),
            deliver_policy: jetstream::consumer::DeliverPolicy::LastPerSubject,
            ack_policy: jetstream::consumer::AckPolicy::None,
            ..Default::default()
        })
        .await
        .with_context(|| format!("create ticker consumer for {ticker_subject}"))?;

    let mut messages = consumer
        .messages()
        .await
        .context("open ticker message stream")?;

    tracing::info!(subject = %ticker_subject, "ticker consumer connected");

    let mut consecutive_errors: u32 = 0;
    while let Some(msg_result) = messages.next().await {
        let msg = match msg_result {
            Ok(m) => {
                consecutive_errors = 0;
                m
            }
            Err(e) => {
                consecutive_errors += 1;
                tracing::warn!(error = %e, consecutive_errors, "ticker receive error");
                if consecutive_errors >= MAX_CONSECUTIVE_RECEIVE_ERRORS {
                    return Err(anyhow!(
                        "{consecutive_errors} consecutive ticker receive errors: {e}"
                    ));
                }
                continue;
            }
        };

        if let Some((ticker, tick)) = parse_ticker(&msg.payload) {
            map.update(ticker, tick);
        }
        // AckPolicy::None — nothing to ack.
    }
    Ok(())
}

/// Look up the final tick for a settling market, preferring the in-process map
/// (Memory), then an optional Redis snap (Redis), else None (Missing).
async fn resolve_final_tick(
    ticker: &str,
    map: &LastTickMap,
    redis: Option<&redis::aio::MultiplexedConnection>,
) -> (Option<LastTick>, SnapSource) {
    if let Some(tick) = map.get(ticker) {
        return (Some(tick), SnapSource::Memory);
    }
    if let Some(conn) = redis {
        let key = format!("snap:kalshi:{ticker}");
        let mut conn = conn.clone();
        let raw: Option<Vec<u8>> = redis::cmd("GET")
            .arg(&key)
            .query_async(&mut conn)
            .await
            .unwrap_or(None);
        if let Some(bytes) = raw {
            if let Some(tick) = parse_snap_value(&bytes) {
                return (Some(tick), SnapSource::Redis);
            }
        }
    }
    (None, SnapSource::Missing)
}

/// Current wall-clock time in epoch milliseconds.
fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

/// Run the durable lifecycle consumer loop. Returns `Err` on a fatal condition
/// (NATS disconnect after retries, or persistent GCS failure) so the caller can
/// crash the process.
async fn run_lifecycle(
    js: &Context,
    config: &Config,
    map: Arc<LastTickMap>,
    gcs: Arc<GcsWriter>,
    redis: Option<redis::aio::MultiplexedConnection>,
    progress: Arc<Progress>,
) -> Result<()> {
    let stream = js
        .get_stream(&config.lifecycle_stream)
        .await
        .with_context(|| format!("get lifecycle stream {}", config.lifecycle_stream))?;

    let consumer = stream
        .get_or_create_consumer(
            &config.consumer_name,
            PullConfig {
                durable_name: Some(config.consumer_name.clone()),
                filter_subject: config.lifecycle_subject.clone(),
                ..Default::default()
            },
        )
        .await
        .with_context(|| format!("create lifecycle consumer {}", config.consumer_name))?;

    let mut messages = consumer
        .stream()
        .heartbeat(Duration::from_secs(5))
        .messages()
        .await
        .context("open lifecycle message stream")?;

    tracing::info!(
        stream = %config.lifecycle_stream,
        consumer = %config.consumer_name,
        subject = %config.lifecycle_subject,
        "lifecycle consumer connected",
    );

    let mut consecutive_receive_errors: u32 = 0;
    let mut consecutive_gcs_errors: u32 = 0;

    while let Some(msg_result) = messages.next().await {
        let msg = match msg_result {
            Ok(m) => {
                consecutive_receive_errors = 0;
                m
            }
            Err(e) => {
                consecutive_receive_errors += 1;
                tracing::warn!(error = %e, consecutive_receive_errors, "lifecycle receive error");
                if consecutive_receive_errors >= MAX_CONSECUTIVE_RECEIVE_ERRORS {
                    return Err(anyhow!(
                        "{consecutive_receive_errors} consecutive lifecycle receive errors: {e}"
                    ));
                }
                continue;
            }
        };

        // Parse + filter. Malformed / non-trigger / non-15M events are acked and
        // skipped (not data loss of a real settlement).
        let parsed = lifecycle::parse(&msg.payload);
        let lc = match parsed {
            Some(lc) => lc,
            None => {
                ack(&msg).await?;
                continue;
            }
        };

        if !is_15m(&lc.market_ticker) || !is_settlement_trigger(&lc.event_type) {
            ack(&msg).await?;
            continue;
        }

        let nats_seq = msg
            .info()
            .map(|i| i.stream_sequence as i64)
            .unwrap_or_default();
        let trigger = SettlementTrigger::from_lifecycle(&lc, nats_seq);

        let (tick, source) = resolve_final_tick(&lc.market_ticker, &map, redis.as_ref()).await;
        let record = SettlementRecord::build_with_source(&trigger, tick, source, now_ms());

        match gcs.write_if_absent(&record).await {
            Ok(outcome) => {
                consecutive_gcs_errors = 0;
                match outcome {
                    WriteOutcome::Written => {
                        progress.written.fetch_add(1, Ordering::Relaxed);
                    }
                    WriteOutcome::Exists => {
                        progress.exists.fetch_add(1, Ordering::Relaxed);
                    }
                }
                match source {
                    SnapSource::Redis => {
                        progress.redis_fallback.fetch_add(1, Ordering::Relaxed);
                    }
                    SnapSource::Missing => {
                        progress.missing.fetch_add(1, Ordering::Relaxed);
                    }
                    _ => {}
                }
                // Only ack AFTER a durable write/confirm.
                ack(&msg).await?;

                let n = progress.processed.fetch_add(1, Ordering::Relaxed) + 1;
                // `is_multiple_of` is unstable on the older Rust in CI images.
                #[allow(clippy::manual_is_multiple_of)]
                if n % PROGRESS_EVERY == 0 {
                    progress.log();
                }
            }
            Err(e) => {
                // Do NOT ack — the durable consumer will redeliver. Crash after
                // N consecutive failures so K8s restarts us.
                consecutive_gcs_errors += 1;
                tracing::error!(
                    market_ticker = %lc.market_ticker,
                    error = %e,
                    consecutive_gcs_errors,
                    "GCS write failed; not acking (will redeliver)",
                );
                if consecutive_gcs_errors >= MAX_CONSECUTIVE_GCS_ERRORS {
                    return Err(anyhow!(
                        "{consecutive_gcs_errors} consecutive GCS write failures: {e}"
                    ));
                }
                // Brief backoff before the next redelivery attempt.
                tokio::time::sleep(Duration::from_secs(2)).await;
            }
        }
    }

    Err(anyhow!("lifecycle message stream ended unexpectedly"))
}

/// Ack a JetStream message, converting any ack error into an `anyhow` error so
/// it bubbles up to the crash boundary (a consumer that cannot ack is broken).
async fn ack(msg: &async_nats::jetstream::Message) -> Result<()> {
    msg.ack().await.map_err(|e| anyhow!("ack failed: {e}"))
}

/// Wire everything and run. Returns `Err` on any fatal condition; the binary
/// exits non-zero on `Err` so K8s restarts the pod (crash-cascade policy).
pub async fn run(config: Config) -> Result<()> {
    tracing::info!(
        nats_url = %config.nats_url,
        lifecycle_stream = %config.lifecycle_stream,
        ticker_subject = %config.ticker_subject,
        gcs_bucket = %config.gcs_bucket,
        redis = config.redis_url.is_some(),
        database = config.database_url.is_some(),
        "settlement-snap run starting",
    );

    // NATS / JetStream.
    let nats_client = async_nats::connect(&config.nats_url)
        .await
        .with_context(|| format!("connect NATS {}", config.nats_url))?;
    let js = jetstream::new(nats_client);

    // GCS writer (fail-loud on auth/config at startup).
    let gcs = Arc::new(
        GcsWriter::from_env(&config.gcs_bucket)
            .with_context(|| format!("init GCS writer for bucket {}", config.gcs_bucket))?,
    );

    // Optional Redis fallback connection + health check (crash on disconnect).
    let redis = match &config.redis_url {
        Some(url) => {
            let client = redis::Client::open(url.as_str())
                .with_context(|| format!("open Redis client {url}"))?;
            let conn = client
                .get_multiplexed_async_connection()
                .await
                .context("connect Redis")?;
            ssmd_middleware::redis_health::spawn_redis_health_check(conn.clone());
            Some(conn)
        }
        None => None,
    };

    // Optional Postgres pool for the startup reconciler + health check.
    let pg_pool = match &config.database_url {
        Some(url) => {
            let mut cfg = deadpool_postgres::Config::new();
            cfg.url = Some(url.clone());
            cfg.pool = Some(deadpool_postgres::PoolConfig {
                max_size: 2,
                ..Default::default()
            });
            let pool = cfg
                .create_pool(
                    Some(deadpool_postgres::Runtime::Tokio1),
                    tokio_postgres::NoTls,
                )
                .context("create Postgres pool")?;
            ssmd_middleware::postgres_health::spawn_postgres_health_check(pool.clone());
            Some(pool)
        }
        None => None,
    };

    let map = Arc::new(LastTickMap::new());
    let progress = Arc::new(Progress::default());

    // Spawn the ephemeral ticker consumer (feeds the last-tick map).
    spawn_ticker_task(js.clone(), config.ticker_subject.clone(), map.clone());

    // Startup reconciliation backfill (best-effort backstop) before the steady
    // state. A reconcile failure is logged but does not block the consumer.
    if let Some(pool) = &pg_pool {
        match reconcile::run(pool, &gcs).await {
            Ok(n) => tracing::info!(backfilled = n, "startup reconciliation complete"),
            Err(e) => tracing::error!(error = %e, "startup reconciliation failed (continuing)"),
        }
    }

    // Run the durable lifecycle consumer — its exit is fatal.
    run_lifecycle(&js, &config, map, gcs, redis, progress).await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_ticker_unwraps_envelope_and_price_alias() {
        let payload = br#"{"type":"ticker","sid":1,"msg":{"market_ticker":"KXBTC15M-26JUN031400-15","yes_bid":96,"yes_ask":98,"no_bid":2,"no_ask":4,"price":97,"volume":1000,"open_interest":500,"ts":1717424100}}"#;
        let (ticker, tick) = parse_ticker(payload).expect("should parse");
        assert_eq!(ticker, "KXBTC15M-26JUN031400-15");
        assert_eq!(tick.yes_bid, Some(96));
        assert_eq!(tick.yes_ask, Some(98));
        assert_eq!(tick.no_bid, Some(2));
        assert_eq!(tick.no_ask, Some(4));
        // last_price resolved via the "price" alias
        assert_eq!(tick.last_price, Some(97));
        assert_eq!(tick.volume, Some(1000));
        assert_eq!(tick.open_interest, Some(500));
        assert_eq!(tick.ts, 1717424100);
    }

    #[test]
    fn parse_ticker_prefers_last_price_over_price() {
        let payload =
            br#"{"msg":{"market_ticker":"KXBTC15M-1-15","last_price":50,"price":99,"ts":1}}"#;
        let (_, tick) = parse_ticker(payload).expect("should parse");
        assert_eq!(tick.last_price, Some(50));
    }

    #[test]
    fn parse_ticker_handles_flat_object() {
        let payload = br#"{"market_ticker":"KXETH15M-1-15","yes_bid":10,"ts":42}"#;
        let (ticker, tick) = parse_ticker(payload).expect("should parse flat");
        assert_eq!(ticker, "KXETH15M-1-15");
        assert_eq!(tick.yes_bid, Some(10));
        assert_eq!(tick.ts, 42);
        assert_eq!(tick.last_price, None);
    }

    #[test]
    fn parse_ticker_zero_volume_and_price_kept() {
        let payload = br#"{"msg":{"market_ticker":"KXBTC15M-1-15","price":0,"volume":0,"open_interest":0,"ts":7}}"#;
        let (_, tick) = parse_ticker(payload).expect("should parse");
        assert_eq!(tick.last_price, Some(0));
        assert_eq!(tick.volume, Some(0));
        assert_eq!(tick.open_interest, Some(0));
    }

    #[test]
    fn parse_ticker_rejects_missing_ticker() {
        let payload = br#"{"msg":{"yes_bid":1,"ts":1}}"#;
        assert!(parse_ticker(payload).is_none());
    }

    #[test]
    fn parse_ticker_rejects_missing_ts() {
        let payload = br#"{"msg":{"market_ticker":"KXBTC15M-1-15","yes_bid":1}}"#;
        assert!(parse_ticker(payload).is_none());
    }

    #[test]
    fn parse_ticker_rejects_malformed_json() {
        assert!(parse_ticker(b"not json").is_none());
    }

    #[test]
    fn parse_snap_value_reuses_ticker_parse() {
        // ssmd-snap stores the same envelope with an injected _snap_at.
        let payload = br#"{"type":"ticker","sid":1,"_snap_at":1717424100000,"msg":{"market_ticker":"KXBTC15M-1-15","yes_bid":90,"price":91,"ts":1717424100}}"#;
        let tick = parse_snap_value(payload).expect("should parse snap");
        assert_eq!(tick.yes_bid, Some(90));
        assert_eq!(tick.last_price, Some(91));
        assert_eq!(tick.ts, 1717424100);
    }

    #[tokio::test]
    async fn resolve_final_tick_prefers_memory() {
        let map = LastTickMap::new();
        let tick = LastTick {
            yes_bid: Some(1),
            yes_ask: Some(2),
            no_bid: None,
            no_ask: None,
            last_price: Some(50),
            volume: Some(10),
            open_interest: Some(5),
            ts: 100,
        };
        map.update("KXBTC15M-1-15", tick.clone());
        let (got, source) = resolve_final_tick("KXBTC15M-1-15", &map, None).await;
        assert_eq!(got, Some(tick));
        assert_eq!(source, SnapSource::Memory);
    }

    #[tokio::test]
    async fn resolve_final_tick_missing_without_redis() {
        let map = LastTickMap::new();
        let (got, source) = resolve_final_tick("KXBTC15M-1-15", &map, None).await;
        assert!(got.is_none());
        assert_eq!(source, SnapSource::Missing);
    }
}
