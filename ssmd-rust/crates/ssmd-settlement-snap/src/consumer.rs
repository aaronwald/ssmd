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
use crate::metrics;
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

/// Convert a fractional-dollar string like `"0.9990"` to native Kalshi cents,
/// rounding to the nearest cent, then clamping to the valid Kalshi price domain
/// `[0, 100]`. Defensive: a non-numeric or non-finite string yields `None` so
/// one malformed field never poisons the whole tick (or panics on untrusted
/// exchange bytes). The clamp bounds every price cent, rejecting absurd values
/// (e.g. `"9e99"` → `i64::MAX`) into the boundary instead of storing garbage —
/// this also keeps the NO-side complement (`100 - yes`) overflow-proof.
fn dollars_to_cents(s: &str) -> Option<i64> {
    let dollars: f64 = s.trim().parse().ok()?;
    if !dollars.is_finite() {
        return None;
    }
    Some(((dollars * 100.0).round() as i64).clamp(0, 100))
}

/// Convert a fixed-point string like `"2233487.48"` to a rounded integer count
/// (volume / open interest), clamped to non-negative. Defensive like
/// [`dollars_to_cents`]: malformed or non-finite input yields `None`, never a
/// panic. No upper bound — volume / open interest are legitimately large.
fn fp_to_i64(s: &str) -> Option<i64> {
    let val: f64 = s.trim().parse().ok()?;
    if !val.is_finite() {
        return None;
    }
    Some((val.round() as i64).max(0))
}

/// Parse a ticker NATS payload (the `{type, sid, msg:{...}}` envelope or a flat
/// ticker object) into `(market_ticker, LastTick)`. Pure and unit-testable.
///
/// The Kalshi crypto ticker feed migrated to a fractional dollar/fp string
/// format (`price_dollars`, `yes_bid_dollars`, `volume_fp`, ...). We prefer the
/// new string fields, converting to native Kalshi cents, and fall back to the
/// legacy integer names (`yes_bid`, `price`, `volume`, ...) so either wire shape
/// parses. `LastTick` fields stay native cents; `ts` is a Unix epoch-seconds
/// integer (the connector serializes `DateTime<Utc>` via `.timestamp()`).
pub fn parse_ticker(payload: &[u8]) -> Option<(String, LastTick)> {
    let v: serde_json::Value = serde_json::from_slice(payload).ok()?;
    // Unwrap the connector envelope `{ "msg": { ... } }` if present; otherwise
    // treat the top-level object as the ticker body.
    let body = v.get("msg").unwrap_or(&v);
    let obj = body.as_object()?;

    let market_ticker = obj.get("market_ticker")?.as_str()?.to_string();

    // Legacy integer field (old wire) — absent on the new dollar/fp wire.
    let as_i64 = |key: &str| obj.get(key).and_then(|x| x.as_i64());
    // New fractional-dollar string field → cents.
    let dollars = |key: &str| {
        obj.get(key)
            .and_then(|x| x.as_str())
            .and_then(dollars_to_cents)
    };
    // New fixed-point string field → rounded integer.
    let fp = |key: &str| obj.get(key).and_then(|x| x.as_str()).and_then(fp_to_i64);

    // `ts` may arrive as an integer epoch-seconds or, defensively, a string.
    let ts = obj.get("ts").and_then(|x| {
        x.as_i64()
            .or_else(|| x.as_str().and_then(|s| s.parse().ok()))
    })?;

    // Normalize EVERY price cent into the valid Kalshi domain [0, 100],
    // regardless of source. `dollars_to_cents` already clamps the new string
    // path, but the legacy-int fallback (`as_i64`) is unclamped — a malformed or
    // negative legacy value would otherwise flow into the complement `100 - yes`
    // and persist an out-of-domain (>100) NO price, which is worse than null.
    let clamp_price = |c: i64| c.clamp(0, 100);
    // Prefer the new `*_dollars` string; fall back to the legacy int; then clamp.
    let yes_bid = dollars("yes_bid_dollars")
        .or_else(|| as_i64("yes_bid"))
        .map(clamp_price);
    let yes_ask = dollars("yes_ask_dollars")
        .or_else(|| as_i64("yes_ask"))
        .map(clamp_price);

    // The new wire does NOT carry no_bid/no_ask. For a binary market the NO side
    // is the complement of the YES side: no_bid = 100 - yes_ask, no_ask =
    // 100 - yes_bid. Prefer a legacy explicit value if present (clamped), else
    // derive from the complement only when the corresponding YES side is known.
    // `saturating_sub` is belt-and-suspenders: the yes side is already clamped to
    // [0, 100], so the complement is provably in [0, 100].
    let no_bid = as_i64("no_bid")
        .map(clamp_price)
        .or_else(|| yes_ask.map(|a| 100i64.saturating_sub(a)));
    let no_ask = as_i64("no_ask")
        .map(clamp_price)
        .or_else(|| yes_bid.map(|b| 100i64.saturating_sub(b)));

    let last_price = dollars("price_dollars")
        .or_else(|| as_i64("last_price"))
        .or_else(|| as_i64("price"))
        .map(clamp_price);
    // Volume / open interest: clamp the legacy-int fallback non-negative too (the
    // new fp path already does this via `fp_to_i64`). No upper bound.
    let volume = fp("volume_fp").or_else(|| as_i64("volume").map(|v| v.max(0)));
    let open_interest =
        fp("open_interest_fp").or_else(|| as_i64("open_interest").map(|v| v.max(0)));

    let tick = LastTick {
        yes_bid,
        yes_ask,
        no_bid,
        no_ask,
        last_price,
        volume,
        open_interest,
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
                let outcome_label = match outcome {
                    WriteOutcome::Written => {
                        progress.written.fetch_add(1, Ordering::Relaxed);
                        metrics::OUTCOME_WRITTEN
                    }
                    WriteOutcome::Exists => {
                        progress.exists.fetch_add(1, Ordering::Relaxed);
                        metrics::OUTCOME_EXISTS
                    }
                };
                metrics::inc_record_written(&record.coin, outcome_label);
                let source_label = match source {
                    SnapSource::Memory => metrics::SOURCE_MEMORY,
                    SnapSource::Redis => {
                        progress.redis_fallback.fetch_add(1, Ordering::Relaxed);
                        metrics::SOURCE_REDIS
                    }
                    SnapSource::Secmaster => metrics::SOURCE_SECMASTER,
                    SnapSource::Missing => {
                        progress.missing.fetch_add(1, Ordering::Relaxed);
                        metrics::SOURCE_MISSING
                    }
                };
                metrics::inc_lookup(source_label);
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

/// Render the default global Prometheus registry as text for `GET /metrics`.
async fn metrics_handler() -> (
    axum::http::StatusCode,
    [(&'static str, &'static str); 1],
    String,
) {
    match metrics::encode_metrics() {
        Ok(body) => (
            axum::http::StatusCode::OK,
            [("content-type", "text/plain; version=0.0.4; charset=utf-8")],
            body,
        ),
        Err(e) => (
            axum::http::StatusCode::INTERNAL_SERVER_ERROR,
            [("content-type", "text/plain; charset=utf-8")],
            format!("Failed to encode metrics: {e}"),
        ),
    }
}

/// Spawn the `/metrics` (+ `/healthz`) HTTP server on a background task. A bind
/// failure is fatal — the process exits so K8s restarts it (we never want a
/// silently unscrapeable pod).
fn spawn_metrics_server(addr: String) {
    tokio::spawn(async move {
        let app = axum::Router::new()
            .route("/metrics", axum::routing::get(metrics_handler))
            .route("/healthz", axum::routing::get(|| async { "ok" }));
        let listener = match tokio::net::TcpListener::bind(&addr).await {
            Ok(l) => l,
            Err(e) => {
                tracing::error!(addr = %addr, error = %e, "failed to bind metrics server");
                std::process::exit(1);
            }
        };
        tracing::info!(addr = %addr, "metrics server listening");
        if let Err(e) = axum::serve(listener, app).await {
            tracing::error!(error = %e, "metrics server exited");
            std::process::exit(1);
        }
    });
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

    // Pre-initialize metric series and spawn the Prometheus /metrics server so
    // GMP can scrape (and discover the metric names) from the first moment.
    metrics::init_metrics();
    spawn_metrics_server(config.metrics_addr.clone());

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
    fn parse_ticker_new_fractional_dollar_format() {
        // REAL captured live 15M crypto ticker `msg` body (new fractional
        // dollar/fp string format), wrapped in the connector envelope.
        let payload = br#"{"type":"ticker","sid":1,"msg":{"dollar_open_interest":309350,"dollar_volume":1116743,"last_trade_size_fp":"0.11","market_id":"e8117a18-7e83-40b4-b2e8-116a8fd494f0","market_ticker":"KXBTC15M-26JUL191145-45","open_interest_fp":"618700.14","price_dollars":"0.9990","time":"2026-07-19T15:45:00.544125Z","ts":1784475900,"ts_ms":1784475900544,"volume_fp":"2233487.48","yes_ask_dollars":"1.0000","yes_ask_size_fp":"0.00","yes_bid_dollars":"0.0000","yes_bid_size_fp":"0.00"}}"#;
        let (ticker, tick) = parse_ticker(payload).expect("should parse new format");
        assert_eq!(ticker, "KXBTC15M-26JUL191145-45");
        assert_eq!(tick.yes_bid, Some(0)); // "0.0000" -> 0
        assert_eq!(tick.yes_ask, Some(100)); // "1.0000" -> 100
        assert_eq!(tick.no_bid, Some(0)); // 100 - yes_ask (100)
        assert_eq!(tick.no_ask, Some(100)); // 100 - yes_bid (0)
        assert_eq!(tick.last_price, Some(100)); // "0.9990" -> 99.9 -> round 100
        assert_eq!(tick.volume, Some(2233487)); // "2233487.48" -> round
        assert_eq!(tick.open_interest, Some(618700)); // "618700.14" -> round
        assert_eq!(tick.ts, 1784475900);
    }

    #[test]
    fn parse_ticker_malicious_huge_dollars_do_not_overflow() {
        // Absurd dollar strings must NOT panic (in debug this is where the
        // unclamped `100 - i64::MIN` complement used to overflow) and must yield
        // in-range price cents. yes_ask "-1e300" clamps to 0 -> no_bid = 100;
        // yes_bid "9e99" clamps to 100 -> no_ask = 0.
        let payload = br#"{"msg":{"market_ticker":"KXBTC15M-1-15","yes_ask_dollars":"-1e300","yes_bid_dollars":"9e99","price_dollars":"5e120","ts":1784475900}}"#;
        let (_, tick) = parse_ticker(payload).expect("should parse without panic");
        assert_eq!(tick.yes_ask, Some(0)); // clamped low
        assert_eq!(tick.yes_bid, Some(100)); // clamped high
        assert_eq!(tick.no_bid, Some(100)); // 100 - yes_ask(0)
        assert_eq!(tick.no_ask, Some(0)); // 100 - yes_bid(100)
        assert_eq!(tick.last_price, Some(100)); // clamped high
                                                // Every price cent stays within the valid Kalshi domain.
        for v in [
            tick.yes_bid,
            tick.yes_ask,
            tick.no_bid,
            tick.no_ask,
            tick.last_price,
        ]
        .into_iter()
        .flatten()
        {
            assert!((0..=100).contains(&v), "price cent {v} out of range");
        }
    }

    #[test]
    fn parse_ticker_legacy_negative_yes_ask_clamps_and_complement_in_range() {
        // Legacy integer wire (no *_dollars, no explicit no_*). A negative
        // yes_ask must clamp to 0 -> no_bid = 100 (in range), never persist a
        // >100 or negative NO price.
        let payload = br#"{"msg":{"market_ticker":"KXBTC15M-1-15","yes_ask":-50,"ts":1784475900}}"#;
        let (_, tick) = parse_ticker(payload).expect("should parse");
        assert_eq!(tick.yes_ask, Some(0)); // -50 clamped to 0
        assert_eq!(tick.no_bid, Some(100)); // 100 - 0
        assert_in_range(&tick);
    }

    #[test]
    fn parse_ticker_legacy_negative_yes_bid_clamps_and_complement_in_range() {
        let payload = br#"{"msg":{"market_ticker":"KXBTC15M-1-15","yes_bid":-50,"ts":1784475900}}"#;
        let (_, tick) = parse_ticker(payload).expect("should parse");
        assert_eq!(tick.yes_bid, Some(0)); // -50 clamped to 0
        assert_eq!(tick.no_ask, Some(100)); // 100 - 0
        assert_in_range(&tick);
    }

    #[test]
    fn parse_ticker_legacy_oversized_yes_bid_clamps_and_complement_in_range() {
        let payload = br#"{"msg":{"market_ticker":"KXBTC15M-1-15","yes_bid":150,"ts":1784475900}}"#;
        let (_, tick) = parse_ticker(payload).expect("should parse");
        assert_eq!(tick.yes_bid, Some(100)); // 150 clamped to 100
        assert_eq!(tick.no_ask, Some(0)); // 100 - 100
        assert_in_range(&tick);
    }

    /// Assert every present price cent is within the valid Kalshi domain [0,100].
    fn assert_in_range(tick: &LastTick) {
        for v in [
            tick.yes_bid,
            tick.yes_ask,
            tick.no_bid,
            tick.no_ask,
            tick.last_price,
        ]
        .into_iter()
        .flatten()
        {
            assert!((0..=100).contains(&v), "price cent {v} out of range");
        }
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
        // ssmd-snap stores the same envelope with an injected `_snap_at`. This
        // is the new fractional dollar/fp wire, proving the Redis path too.
        let payload = br#"{"type":"ticker","sid":1,"_snap_at":1784475900544,"msg":{"market_ticker":"KXBTC15M-26JUL191145-45","price_dollars":"0.9990","yes_bid_dollars":"0.0000","yes_ask_dollars":"1.0000","volume_fp":"2233487.48","open_interest_fp":"618700.14","ts":1784475900}}"#;
        let tick = parse_snap_value(payload).expect("should parse snap");
        assert_eq!(tick.yes_bid, Some(0));
        assert_eq!(tick.yes_ask, Some(100));
        assert_eq!(tick.no_bid, Some(0)); // 100 - yes_ask
        assert_eq!(tick.no_ask, Some(100)); // 100 - yes_bid
        assert_eq!(tick.last_price, Some(100));
        assert_eq!(tick.volume, Some(2233487));
        assert_eq!(tick.open_interest, Some(618700));
        assert_eq!(tick.ts, 1784475900);
    }

    #[test]
    fn parse_ticker_malformed_dollar_field_is_none_not_panic() {
        // A garbage dollar/fp string makes THAT field None; ticker + ts still
        // parse. Never panic on untrusted exchange bytes.
        let payload = br#"{"msg":{"market_ticker":"KXBTC15M-1-15","price_dollars":"NaNnope","volume_fp":"","yes_bid_dollars":"0.5000","ts":1784475900}}"#;
        let (ticker, tick) = parse_ticker(payload).expect("should still parse");
        assert_eq!(ticker, "KXBTC15M-1-15");
        assert_eq!(tick.last_price, None); // malformed price_dollars
        assert_eq!(tick.volume, None); // empty volume_fp
        assert_eq!(tick.yes_bid, Some(50)); // "0.5000" -> 50
        assert_eq!(tick.ts, 1784475900);
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
