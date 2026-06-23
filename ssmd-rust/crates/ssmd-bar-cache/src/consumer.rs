//! NATS JetStream consumer loop driving the [`MinuteAggregator`] into Redis.
//!
//! Two subscriptions feed one aggregator: massive 1s OHLCV aggregates and
//! kraken-spot trades. They differ only in their wire format, so each
//! subscription carries its own parser (`parse_massive_1s` / `parse_kraken_trade`)
//! and feed label — that is the entirety of the per-feed routing. Everything
//! after parsing (aggregation, Redis ring write, freshness gauge) is identical.

use std::sync::Arc;
use std::time::Duration;

use async_nats::jetstream::{self, consumer::pull::Config as ConsumerConfig, Context};
use futures_util::StreamExt;
use tokio::sync::Mutex;
use tracing::{info, warn};

use crate::agg::{Bar, Input, MinuteAggregator};
use crate::metrics::Metrics;
use crate::store::upsert_bar;

/// Parser for a subscription's wire format → zero or more normalized [`Input`]s.
///
/// Massive 1s aggregates yield 0 or 1 input per message; the kraken-spot v2
/// trade envelope wraps a `data[]` array and yields 0..N.
type Parser = fn(&[u8]) -> Vec<Input>;

/// A single JetStream subscription: which stream/subject to read, how to parse
/// it, and the feed label it writes under.
pub struct Subscription {
    pub stream_name: String,
    pub filter_subject: String,
    pub feed: String,
    pub parse: Parser,
}

/// Run a subscription forever, restarting on stream end or error (fail-loud:
/// after exhausting in-loop retries the process exits so K8s restarts it).
///
/// The aggregator is shared across both subscriptions behind a `Mutex`: a
/// symbol could in principle appear on either feed, and the aggregator's
/// per-symbol state must be single-writer. Contention is negligible at these
/// message rates.
pub async fn run_subscription(
    js: Context,
    redis_conn: redis::aio::MultiplexedConnection,
    agg: Arc<Mutex<MinuteAggregator>>,
    sub: Subscription,
    ring_len: usize,
    ttl_secs: u64,
    metrics: Arc<Metrics>,
) {
    let feed = sub.feed.clone();
    info!(
        stream = %sub.stream_name,
        feed = %feed,
        filter = %sub.filter_subject,
        ring_len,
        ttl_secs,
        "starting bar-cache consumer"
    );

    let mut consecutive_failures: u32 = 0;
    const MAX_CONSECUTIVE_FAILURES: u32 = 10;

    loop {
        match run_inner(&js, &redis_conn, &agg, &sub, ring_len, ttl_secs, &metrics).await {
            Ok(()) => {
                // A clean stream end is unexpected for an ephemeral consumer but
                // recoverable: re-create and continue. Reset the failure count.
                consecutive_failures = 0;
                info!(feed = %feed, "consumer stream ended, restarting");
            }
            Err(e) => {
                consecutive_failures += 1;
                metrics.errors.with_label_values(&[&feed, "consumer"]).inc();
                warn!(
                    feed = %feed,
                    error = %e,
                    consecutive_failures,
                    "consumer error, restarting in 5s"
                );
                if consecutive_failures >= MAX_CONSECUTIVE_FAILURES {
                    // Fail loud per the synchronization architecture: a feed we
                    // cannot consume must crash the pod, not limp silently.
                    tracing::error!(
                        feed = %feed,
                        consecutive_failures,
                        "exceeded max consecutive consumer failures; exiting for K8s restart"
                    );
                    std::process::exit(1);
                }
                tokio::time::sleep(Duration::from_secs(5)).await;
            }
        }
    }
}

async fn run_inner(
    js: &Context,
    redis_conn: &redis::aio::MultiplexedConnection,
    agg: &Arc<Mutex<MinuteAggregator>>,
    sub: &Subscription,
    ring_len: usize,
    ttl_secs: u64,
    metrics: &Metrics,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let feed = &sub.feed;

    let stream = js
        .get_stream(&sub.stream_name)
        .await
        .map_err(|e| format!("get stream {}: {e}", sub.stream_name))?;

    // Ephemeral pull consumer. We must observe EVERY message (each 1s bar / each
    // trade) to aggregate correctly, so DeliverPolicy::All — never LastPerSubject.
    let consumer = stream
        .create_consumer(ConsumerConfig {
            filter_subject: sub.filter_subject.clone(),
            deliver_policy: jetstream::consumer::DeliverPolicy::All,
            ack_policy: jetstream::consumer::AckPolicy::None,
            ..Default::default()
        })
        .await
        .map_err(|e| format!("create consumer for {}: {e}", sub.stream_name))?;

    let mut messages = consumer
        .messages()
        .await
        .map_err(|e| format!("get messages for {}: {e}", sub.stream_name))?;

    info!(feed = %feed, "consumer connected, processing messages");

    let mut consecutive_receive_errors: u32 = 0;
    const MAX_CONSECUTIVE_RECEIVE_ERRORS: u32 = 5;

    while let Some(msg_result) = messages.next().await {
        let msg = match msg_result {
            Ok(m) => {
                consecutive_receive_errors = 0;
                m
            }
            Err(e) => {
                consecutive_receive_errors += 1;
                metrics.errors.with_label_values(&[feed, "receive"]).inc();
                warn!(feed = %feed, error = %e, consecutive_receive_errors, "message receive error");
                if consecutive_receive_errors >= MAX_CONSECUTIVE_RECEIVE_ERRORS {
                    return Err(format!(
                        "{feed}: {consecutive_receive_errors} consecutive receive errors: {e}"
                    )
                    .into());
                }
                continue;
            }
        };

        metrics.messages_received.with_label_values(&[feed]).inc();

        // Route by the subscription's own parser. One message yields zero or
        // more inputs (a kraken envelope carries a `data[]` array of trades; a
        // massive aggregate is a single object). An empty result is a parse
        // "miss": either a malformed payload (the parser logged) or a non-trade
        // kraken control frame (heartbeat/ticker/ack). Count it and move on —
        // never crash the consumer over one payload.
        let inputs = (sub.parse)(&msg.payload);
        if inputs.is_empty() {
            metrics.errors.with_label_values(&[feed, "parse"]).inc();
            continue;
        }

        // Aggregate each input. A rollover finalizes the prior minute; always
        // write the current (forming) minute too so consumers see live updates.
        // The per-input rollover + current-write logic is unchanged from the
        // single-input path.
        for input in inputs {
            let result = {
                let mut guard = agg.lock().await;
                guard.ingest(input)
            };

            if let Some(finalized) = result.finalized {
                write_bar(redis_conn, feed, ring_len, ttl_secs, finalized, metrics).await;
            }
            write_bar(
                redis_conn,
                feed,
                ring_len,
                ttl_secs,
                result.current,
                metrics,
            )
            .await;
        }
    }

    Ok(())
}

/// Write one bar to the Redis ring and update the freshness gauge + counter.
///
/// A Redis write failure is logged and counted but does not crash here — the
/// `redis_health` watchdog is responsible for crashing on sustained Redis loss,
/// keeping that policy in exactly one place.
async fn write_bar(
    redis_conn: &redis::aio::MultiplexedConnection,
    feed: &str,
    ring_len: usize,
    ttl_secs: u64,
    bar: Bar,
    metrics: &Metrics,
) {
    let sym = bar.sym.clone();
    let end_ts_ms = bar.end_ts_ms;

    match upsert_bar(redis_conn, feed, ring_len, ttl_secs, bar).await {
        Ok(()) => {
            metrics.bars_written.with_label_values(&[feed]).inc();
            metrics
                .last_bar_ts
                .with_label_values(&[feed, &sym])
                .set(end_ts_ms / 1000);
        }
        Err(e) => {
            metrics.errors.with_label_values(&[feed, "redis"]).inc();
            warn!(feed = %feed, sym = %sym, error = %e, "redis ring write failed");
        }
    }
}
