use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};
use tokio::select;
use tokio::task::JoinSet;
use tracing::{error, info, warn};

use crate::error::ConnectorError;
use crate::message::Message;
use crate::metrics;
use crate::traits::{Connector, Writer};
use ssmd_middleware::{now_tsc, CLOCK};

/// Runner orchestrates the data collection pipeline
pub struct Runner<C: Connector, W: Writer> {
    feed_name: Arc<str>,
    connector: C,
    writer: W,
    connected: Arc<AtomicBool>,
    /// Unix timestamp (seconds) of last message received
    last_message_epoch_secs: Arc<AtomicU64>,
    /// Data clock used by [`Runner::data_handle`] for connectors that do not keep one
    /// of their own. Stamped when the connection comes up and then on every
    /// successful write, so it measures messages that actually reached the writer
    /// and can never be advanced by a control frame the connector dropped.
    ///
    /// Separate from `last_message_epoch_secs` only in its starting point: that one
    /// stays 0 until the first message, which the ceiling would read as
    /// "uninitialized" and skip, so a connector that never publishes anything would
    /// never trip it.
    last_data_epoch_secs: Arc<AtomicU64>,
    /// Permanently-zero stand-in returned by [`Runner::pong_handle`] for connectors
    /// that do not track pongs. Zero reads as "no pong signal", which falls back to
    /// data age alone. Held as a field so the handle is stable across calls.
    no_pong_epoch_secs: Arc<AtomicU64>,
}

/// The watchdog decision for a single tick.
///
/// Split out of the watchdog loop (which exits the process on a stale verdict) so the
/// decision itself is unit-testable, and delegated to [`crate::liveness`] so the
/// watchdog and `/health` cannot drift apart.
///
/// Stale when the minimum of the data age and the pong age reaches
/// `threshold_secs` (either signal being recent proves the socket is alive), OR
/// when the data age alone reaches `max_data_age_secs`. The second half is
/// deliberately blind to the pong, which the remote end controls.
///
/// Uses the inclusive bound to preserve the original `>=` watchdog comparison.
fn watchdog_verdict(
    data_age_secs: u64,
    pong_age_secs: Option<u64>,
    threshold_secs: u64,
    max_data_age_secs: u64,
) -> crate::liveness::StalenessVerdict {
    crate::liveness::evaluate_staleness(
        data_age_secs,
        pong_age_secs,
        threshold_secs,
        max_data_age_secs,
        crate::liveness::ThresholdBound::Inclusive,
    )
}

impl<C: Connector, W: Writer> Runner<C, W> {
    pub fn new(feed_name: impl Into<Arc<str>>, connector: C, writer: W) -> Self {
        Self {
            feed_name: feed_name.into(),
            connector,
            writer,
            connected: Arc::new(AtomicBool::new(false)),
            last_message_epoch_secs: Arc::new(AtomicU64::new(0)),
            last_data_epoch_secs: Arc::new(AtomicU64::new(0)),
            no_pong_epoch_secs: Arc::new(AtomicU64::new(0)),
        }
    }

    /// Returns whether the connector is currently connected
    pub fn is_connected(&self) -> bool {
        self.connected.load(Ordering::SeqCst)
    }

    /// Returns a handle to the connected status
    pub fn connected_handle(&self) -> Arc<AtomicBool> {
        Arc::clone(&self.connected)
    }

    /// Returns a handle to the last message timestamp
    pub fn last_message_handle(&self) -> Arc<AtomicU64> {
        Arc::clone(&self.last_message_epoch_secs)
    }

    /// Returns the connector's activity handle if available.
    /// This tracks WebSocket activity (ping/pong + data messages) for health checks.
    /// Falls back to Runner's last_message_epoch_secs if connector doesn't track activity.
    pub fn activity_handle(&self) -> Arc<AtomicU64> {
        self.connector
            .activity_handle()
            .unwrap_or_else(|| Arc::clone(&self.last_message_epoch_secs))
    }

    /// Returns the connector's data handle if available, otherwise the runner's own
    /// data clock (stamped at connect and on every successful write).
    ///
    /// This is the clock the absolute data-age ceiling reads, and it must never be
    /// the activity handle. Activity is advanced by any received frame, including
    /// the subscribe acks a peer emits while publishing nothing, so a ceiling built
    /// on it can be held open indefinitely by the very failure it exists to catch.
    /// See [`crate::liveness`].
    ///
    /// Re-reads the connector on every call for the same reason as
    /// [`Runner::pong_handle`]: a connector that allocates its tracker inside
    /// `connect()` must not be snapshotted at construction time.
    pub fn data_handle(&self) -> Arc<AtomicU64> {
        self.connector
            .data_handle()
            .unwrap_or_else(|| Arc::clone(&self.last_data_epoch_secs))
    }

    /// Returns a handle to the last WebSocket pong timestamp (epoch seconds).
    /// A value of 0 means no pong has been received yet, or the connector does not
    /// track pongs at all; both cases fall back to data age in the staleness rule.
    pub fn pong_handle(&self) -> Arc<AtomicU64> {
        // Re-read the connector on every call, exactly like activity_handle. A
        // snapshot taken in new() would hand a connector that allocates its tracker
        // inside connect() a permanently zero handle, silently degrading liveness to
        // data-only with nothing to fail on.
        self.connector
            .pong_handle()
            .unwrap_or_else(|| Arc::clone(&self.no_pong_epoch_secs))
    }

    /// Update last message timestamp to current time
    fn update_last_message_time(&self) {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);
        self.last_message_epoch_secs.store(now, Ordering::SeqCst);
        // A message reached the writer, which is the definition of data the ceiling
        // uses. Only relevant for connectors without a data handle of their own —
        // the rest ignore this clock entirely.
        crate::liveness::record_data(&self.last_data_epoch_secs, now);
    }

    /// Start the fallback data clock so the ceiling measures silence from connect.
    ///
    /// Left at 0 it reads as "uninitialized" and every staleness check skips, which
    /// would exempt precisely the connector that comes up and never publishes
    /// anything. Connectors with a data handle of their own do the same thing when
    /// their receive loop starts.
    fn start_data_clock(&self) {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);
        crate::liveness::record_data(&self.last_data_epoch_secs, now);
    }

    /// Run the collection pipeline until cancelled or disconnected
    pub async fn run(&mut self, shutdown: tokio::sync::watch::Receiver<bool>) -> Result<(), ConnectorError> {
        // Connect
        self.connector.connect().await?;
        self.connected.store(true, Ordering::SeqCst);
        // Harmless for connectors that publish their own data handle: nothing reads
        // this clock in that case.
        self.start_data_clock();
        info!(feed = %self.feed_name, "Connected to data source");

        let mut rx = self.connector.messages();
        let mut connector_tasks: Option<JoinSet<()>> = self.connector.tasks();
        let mut shutdown = shutdown;

        // Spawn independent watchdog — detects ALL shards going silent.
        // Runs as a separate tokio::spawn (NOT in the JoinSet) so it cannot
        // be blocked by hung shard tasks or a stuck recv_raw() future.
        // The per-shard staleness check (90s in connector.rs) is the primary
        // defense. This watchdog is the last-resort safety net at 120s.
        // "Silent" means no data AND no pong: a quiet market on a socket that is
        // still answering pings is healthy and must not be restarted. A pong can
        // only postpone the restart though, never cancel it: the remote end emits
        // it, so an absolute data-age ceiling (HEALTH_MAX_DATA_AGE_SECS) still
        // fires on data age alone. "Data" here means messages the connector
        // actually forwarded — the peer's own subscribe acks do not count, or the
        // ceiling would be suppressible by the peer it is meant to catch. See
        // crate::liveness.
        {
            let watchdog_activity = self.activity_handle();
            // The verdict reads this clock, not the activity one. See
            // Runner::data_handle and crate::liveness.
            let watchdog_data = self.data_handle();
            let watchdog_pong = self.pong_handle();
            let watchdog_feed = Arc::clone(&self.feed_name);
            tokio::spawn(async move {
                const WATCHDOG_CHECK_INTERVAL_SECS: u64 = 15;
                const DEFAULT_WATCHDOG_STALENESS_SECS: u64 = 120;
                // Both thresholds are range-checked: a value of 0 means permanent
                // restart and u64::MAX means the check can never fire, and either
                // silently supplied would be worse than no watchdog at all.
                let watchdog_staleness_secs = crate::liveness::threshold_from_env(
                    "WATCHDOG_STALENESS_SECS",
                    DEFAULT_WATCHDOG_STALENESS_SECS,
                );
                // DECISION: the ceiling is whole-connector, not per-shard. It reads
                // the data handle, which every shard writes to, so a connector with
                // one silently dead shard and one streaming shard does not trip it.
                // That is deliberate:
                //   1. For the connector this exists for, the two are the same
                //      thing: kalshi-crypto runs SHARD_SIZE=150 against 9 active 15M
                //      markets, so there is exactly one shard (shard_id=0 in prod).
                //   2. A per-shard ceiling would have to be gated on the shard
                //      having a non-empty subscription set, because headroom shards
                //      start empty and would trip it immediately.
                //   3. Shard-level detection is not lost. It lives in the per-shard
                //      90s check and in the 120s READ_TIMEOUT inside recv_raw, both
                //      of which exit from inside the shard task itself.
                // Do not narrow this to per-shard without addressing point 2.
                let max_data_age_secs = crate::liveness::threshold_from_env(
                    "HEALTH_MAX_DATA_AGE_SECS",
                    crate::liveness::DEFAULT_MAX_DATA_AGE_SECS,
                );
                info!(
                    feed = %watchdog_feed,
                    staleness_secs = watchdog_staleness_secs,
                    max_data_age_secs,
                    "Watchdog started"
                );

                // Two independent rate limiters so a persistent anomaly stays visible
                // in any five-minute slice of logs without one warn per 15s tick.
                let mut quiet_warn = crate::liveness::SuppressionWarnGate::new();
                let mut skew_warn = crate::liveness::SuppressionWarnGate::new();
                let mut data_skew_warn = crate::liveness::SuppressionWarnGate::new();

                let mut check_interval = tokio::time::interval(
                    std::time::Duration::from_secs(WATCHDOG_CHECK_INTERVAL_SECS),
                );
                check_interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);

                // Skip the immediate first tick
                check_interval.tick().await;

                loop {
                    check_interval.tick().await;

                    let last_epoch = watchdog_activity.load(Ordering::SeqCst);
                    if last_epoch == 0 {
                        // Not yet initialized — shards haven't started receiving
                        continue;
                    }

                    let now_epoch = SystemTime::now()
                        .duration_since(UNIX_EPOCH)
                        .map(|d| d.as_secs())
                        .unwrap_or(0);
                    // Age of the last message actually forwarded downstream, NOT of
                    // the last frame received. Control frames — subscribe and
                    // unsubscribe acks above all, which this connector emits on
                    // every 15-minute rollover — must not be able to hold the
                    // ceiling open through a data drought.
                    let data_epoch = watchdog_data.load(Ordering::SeqCst);
                    let data_signal = crate::liveness::data_age_signal(data_epoch, now_epoch);
                    if let crate::liveness::DataAgeSignal::FutureEpoch { skew_secs } = data_signal {
                        if data_skew_warn.should_warn(true, now_epoch) {
                            warn!(
                                feed = %watchdog_feed,
                                skew_secs,
                                data_epoch,
                                now_epoch,
                                reason = "data_clock_skew",
                                "WATCHDOG: data timestamp is {skew_secs}s ahead of the system clock, so the clock stepped backwards; no trustworthy data age can be computed, skipping this check"
                            );
                        }
                    } else {
                        data_skew_warn.should_warn(false, now_epoch);
                    }
                    // Uninitialized (nothing forwarded yet, and no data clock started)
                    // and FutureEpoch both yield no age. Neither is evidence that the
                    // socket is dead, and reading either as age 0 is what silently
                    // disables every layer at once, so skip the tick instead — loudly
                    // in the skew case, which the warn above covers.
                    let Some(data_age_secs) = data_signal.age_secs() else {
                        continue;
                    };
                    // A received pong proves the remote end is alive even when the
                    // market is quiet: sparse feeds (Kalshi 15-minute contracts) idle
                    // for minutes between windows on a perfectly healthy socket.
                    // Absent (no pong yet) and FutureEpoch (untrustworthy clock) both
                    // fall back to the data age.
                    let pong = crate::liveness::pong_signal(
                        watchdog_pong.load(Ordering::SeqCst),
                        now_epoch,
                    );
                    if let crate::liveness::PongSignal::FutureEpoch { skew_secs } = pong {
                        if skew_warn.should_warn(true, now_epoch) {
                            warn!(
                                feed = %watchdog_feed,
                                skew_secs,
                                data_age_secs,
                                reason = "pong_clock_skew",
                                "WATCHDOG: pong timestamp is {skew_secs}s ahead of the system clock, so the clock stepped backwards; ignoring the pong and using data age alone"
                            );
                        }
                    } else {
                        // Clock is sane again: re-arm the gate so the next skew
                        // episode warns immediately instead of waiting out the
                        // repeat interval of the previous one.
                        skew_warn.should_warn(false, now_epoch);
                    }
                    let pong_age_secs = pong.age_secs();

                    let verdict = watchdog_verdict(
                        data_age_secs,
                        pong_age_secs,
                        watchdog_staleness_secs,
                        max_data_age_secs,
                    );

                    // Quiet but alive: no data past the threshold, held up only by a
                    // pong the exchange controls. Legitimate for a sparse feed, and
                    // also the exact shape of a peer that acks our subscribe and then
                    // publishes nothing, so it must never be silent in the logs.
                    if quiet_warn.should_warn(verdict.pong_suppressed, now_epoch) {
                        warn!(
                            feed = %watchdog_feed,
                            data_age_secs,
                            pong_age_secs = ?pong_age_secs,
                            threshold = watchdog_staleness_secs,
                            max_data_age_secs = verdict.effective_ceiling_secs,
                            reason = "quiet_but_alive",
                            "WATCHDOG: no data for {data_age_secs}s (threshold {watchdog_staleness_secs}s) but the pong is fresh; will exit if data stays absent for {}s",
                            verdict.effective_ceiling_secs
                        );
                    }

                    if verdict.stale {
                        error!(
                            feed = %watchdog_feed,
                            data_age_secs,
                            pong_age_secs = ?pong_age_secs,
                            threshold = watchdog_staleness_secs,
                            max_data_age_secs = verdict.effective_ceiling_secs,
                            ceiling_breached = verdict.ceiling_breached,
                            data_epoch,
                            // Logged alongside data_epoch because the gap between
                            // them is the diagnosis: frames still arriving while no
                            // data flows is a peer that acks and publishes nothing.
                            activity_epoch = last_epoch,
                            reason = if verdict.ceiling_breached {
                                "watchdog_max_data_age"
                            } else {
                                "watchdog_stale"
                            },
                            "WATCHDOG: No data ({data_age_secs}s) or pong within {watchdog_staleness_secs}s, exiting for restart"
                        );
                        std::process::exit(1);
                    }
                }
            });
        }

        loop {
            select! {
                _ = shutdown.changed() => {
                    if *shutdown.borrow() {
                        info!("Shutdown signal received");
                        break;
                    }
                }
                msg = rx.recv() => {
                    match msg {
                        Some((ws_tsc, data)) => {
                            // Pass raw bytes through - no JSON parsing in hot path.
                            // Parsing/validation happens at I/O boundary (flusher, gateway).
                            let message = Message::new_with_tsc(Arc::clone(&self.feed_name), data, ws_tsc);
                            let write_start = now_tsc();

                            match tokio::time::timeout(
                                std::time::Duration::from_secs(10),
                                self.writer.write(&message),
                            ).await {
                                Ok(Ok(())) => {} // success
                                Ok(Err(e)) => {
                                    error!(error = %e, "Failed to write message - exiting to trigger restart");
                                    return Err(ConnectorError::WriteFailed(e.to_string()));
                                }
                                Err(_) => {
                                    error!("NATS publish timed out after 10s - exiting to trigger restart");
                                    return Err(ConnectorError::WriteFailed("publish timeout".to_string()));
                                }
                            }
                            let write_end = now_tsc();
                            metrics::observe_nats_publish_duration(
                                &self.feed_name,
                                CLOCK.delta(write_start, write_end).as_secs_f64(),
                            );
                            // ws_tsc captured at WS receive in connector shard
                            metrics::observe_ws_process_duration(
                                &self.feed_name,
                                CLOCK.delta(ws_tsc, write_end).as_secs_f64(),
                            );
                            // Update last message time on successful write
                            self.update_last_message_time();
                        }
                        None => {
                            // Channel closed - connector disconnected unexpectedly
                            self.connected.store(false, Ordering::SeqCst);
                            error!("Connector disconnected unexpectedly - exiting to trigger restart");
                            return Err(ConnectorError::Disconnected("channel closed".to_string()));
                        }
                    }
                }
                // Monitor background tasks — detect panics/exits instead of silent data loss
                result = async {
                    match connector_tasks.as_mut() {
                        Some(tasks) => tasks.join_next().await,
                        None => std::future::pending().await,
                    }
                } => {
                    match result {
                        Some(Ok(())) => {
                            error!("Connector task exited unexpectedly");
                            return Err(ConnectorError::Disconnected("task exited".to_string()));
                        }
                        Some(Err(e)) => {
                            error!(error = %e, "Connector task panicked");
                            return Err(ConnectorError::Disconnected(format!("task panic: {}", e)));
                        }
                        None => {
                            error!("All connector tasks completed unexpectedly");
                            return Err(ConnectorError::Disconnected("all tasks done".to_string()));
                        }
                    }
                }
            }
        }

        // Cleanup
        self.connected.store(false, Ordering::SeqCst);
        self.writer.close().await.ok();
        self.connector.close().await?;

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::traits::TimestampedMsg;
    use async_trait::async_trait;
    use std::sync::atomic::AtomicUsize;
    use tokio::sync::mpsc;

    struct MockConnector {
        #[allow(dead_code)]
        tx: mpsc::Sender<TimestampedMsg>,
        rx: Option<mpsc::Receiver<TimestampedMsg>>,
        pong: Option<Arc<AtomicU64>>,
        data: Option<Arc<AtomicU64>>,
    }

    impl MockConnector {
        fn new() -> (Self, mpsc::Sender<TimestampedMsg>) {
            Self::build(None, None)
        }

        /// Mock a connector that tracks pongs, like the Kalshi connector does.
        fn with_pong(pong: Arc<AtomicU64>) -> (Self, mpsc::Sender<TimestampedMsg>) {
            Self::build(Some(pong), None)
        }

        /// Mock a connector that keeps its own forwarded-data clock, like Kalshi.
        fn with_data(data: Arc<AtomicU64>) -> (Self, mpsc::Sender<TimestampedMsg>) {
            Self::build(None, Some(data))
        }

        fn build(
            pong: Option<Arc<AtomicU64>>,
            data: Option<Arc<AtomicU64>>,
        ) -> (Self, mpsc::Sender<TimestampedMsg>) {
            let (tx, rx) = mpsc::channel(10);
            let tx_clone = tx.clone();
            (
                Self {
                    tx,
                    rx: Some(rx),
                    pong,
                    data,
                },
                tx_clone,
            )
        }
    }

    #[async_trait]
    impl Connector for MockConnector {
        async fn connect(&mut self) -> Result<(), ConnectorError> {
            Ok(())
        }
        fn messages(&mut self) -> mpsc::Receiver<TimestampedMsg> {
            self.rx.take().unwrap()
        }
        async fn close(&mut self) -> Result<(), ConnectorError> {
            Ok(())
        }
        fn pong_handle(&self) -> Option<Arc<AtomicU64>> {
            self.pong.clone()
        }
        fn data_handle(&self) -> Option<Arc<AtomicU64>> {
            self.data.clone()
        }
    }

    /// A connector that publishes its pong tracker only once connected, which is
    /// how a future connector might allocate one inside connect().
    struct LatePongConnector {
        rx: Option<mpsc::Receiver<TimestampedMsg>>,
        pong: Arc<std::sync::Mutex<Option<Arc<AtomicU64>>>>,
    }

    impl LatePongConnector {
        fn new(
            pong: Arc<std::sync::Mutex<Option<Arc<AtomicU64>>>>,
        ) -> (Self, mpsc::Sender<TimestampedMsg>) {
            let (tx, rx) = mpsc::channel(10);
            (Self { rx: Some(rx), pong }, tx)
        }
    }

    #[async_trait]
    impl Connector for LatePongConnector {
        async fn connect(&mut self) -> Result<(), ConnectorError> {
            Ok(())
        }
        fn messages(&mut self) -> mpsc::Receiver<TimestampedMsg> {
            self.rx.take().unwrap()
        }
        async fn close(&mut self) -> Result<(), ConnectorError> {
            Ok(())
        }
        fn pong_handle(&self) -> Option<Arc<AtomicU64>> {
            self.pong.lock().unwrap().clone()
        }
    }

    struct MockWriter {
        write_count: Arc<AtomicUsize>,
    }

    impl MockWriter {
        fn new() -> (Self, Arc<AtomicUsize>) {
            let count = Arc::new(AtomicUsize::new(0));
            (
                Self {
                    write_count: Arc::clone(&count),
                },
                count,
            )
        }
    }

    #[async_trait]
    impl Writer for MockWriter {
        async fn write(&mut self, _msg: &Message) -> Result<(), crate::error::WriterError> {
            self.write_count.fetch_add(1, Ordering::SeqCst);
            Ok(())
        }
        async fn close(&mut self) -> Result<(), crate::error::WriterError> {
            Ok(())
        }
    }

    /// Prod watchdog threshold for the Kalshi crypto feed (WATCHDOG_STALENESS_SECS=300).
    const THRESHOLD: u64 = 300;
    /// Absolute data-age ceiling (HEALTH_MAX_DATA_AGE_SECS default).
    const CEILING: u64 = crate::liveness::DEFAULT_MAX_DATA_AGE_SECS;

    /// The watchdog exit decision at prod settings.
    fn stale(data_age_secs: u64, pong_age_secs: Option<u64>) -> bool {
        watchdog_verdict(data_age_secs, pong_age_secs, THRESHOLD, CEILING).stale
    }

    #[test]
    fn watchdog_alive_when_data_idle_but_pong_fresh() {
        // The outage case: sparse 15-minute markets produced no data for 400s while
        // the socket answered every ping. Killing here loses archive slots.
        assert!(!stale(400, Some(10)));
    }

    #[test]
    fn watchdog_exits_when_data_idle_and_pong_stale() {
        // The guard that keeps the fix honest: a genuinely hung socket has neither
        // data nor pongs, and must still be caught.
        assert!(stale(400, Some(400)));
    }

    #[test]
    fn watchdog_alive_when_data_flowing() {
        assert!(!stale(5, Some(5)));
        assert!(!stale(5, Some(400)), "data alone proves liveness");
        assert!(!stale(5, None));
    }

    #[test]
    fn watchdog_without_pong_falls_back_to_data_age() {
        // No pong tracked (epoch 0 -> None): behaviour is exactly the old data-only
        // rule. It must not look alive...
        assert!(stale(400, None));
        // ...nor spuriously dead.
        assert!(!stale(30, None));
    }

    #[test]
    fn watchdog_pong_epoch_zero_is_not_a_fresh_pong() {
        // A zero pong epoch is "never received", not "pong at the unix epoch" and not
        // "pong just now" — either misreading would break the watchdog.
        let now_epoch = 1_000_000;
        assert_eq!(crate::liveness::pong_age_secs(0, now_epoch), None);
        assert!(stale(400, crate::liveness::pong_age_secs(0, now_epoch)));
    }

    #[test]
    fn watchdog_threshold_comparison_is_inclusive() {
        // Preserves the original >= comparison, so the threshold value itself fires.
        assert!(stale(THRESHOLD, None));
        assert!(!stale(THRESHOLD - 1, None));
    }

    #[test]
    fn watchdog_kills_a_ponging_but_silent_peer_at_the_ceiling() {
        // REGRESSION GUARD for the peer-controlled liveness hole. The pong signal is
        // emitted by the exchange, so a peer that acks our subscribe and then
        // publishes nothing while still answering control frames would keep the
        // watchdog, /health and the per-shard check green forever. The data-age
        // ceiling is the one check it cannot suppress.
        assert!(
            !stale(CEILING - 1, Some(1)),
            "a quiet feed is tolerated up to the ceiling"
        );
        assert!(
            stale(CEILING, Some(1)),
            "but a fresh pong cannot buy more silence than the ceiling"
        );
        assert!(
            stale(CEILING * 10, Some(1)),
            "and it stays dead however long the silence lasts"
        );
    }

    #[test]
    fn watchdog_flags_the_quiet_but_alive_state_before_killing() {
        // The state between the normal threshold and the ceiling is exactly the
        // shape of the silent-peer failure, so it must be visible in the logs.
        let v = watchdog_verdict(400, Some(1), THRESHOLD, CEILING);
        assert!(!v.stale);
        assert!(v.pong_suppressed, "this is what the operator warn keys off");
        assert!(
            !watchdog_verdict(5, Some(1), THRESHOLD, CEILING).pong_suppressed,
            "a healthy feed must not warn"
        );
    }

    #[test]
    fn watchdog_ignores_a_pong_from_a_backwards_stepped_clock() {
        // A pong stamped in the future used to saturate to age 0, disabling the
        // watchdog for the whole skew. It must fall back to the data age instead.
        let now_epoch = 1_000_000;
        let skewed = crate::liveness::pong_signal(now_epoch + 600, now_epoch);
        assert!(matches!(
            skewed,
            crate::liveness::PongSignal::FutureEpoch { .. }
        ));
        assert!(stale(400, skewed.age_secs()));
    }

    #[test]
    fn runner_exposes_connector_pong_handle() {
        let pong = Arc::new(AtomicU64::new(0));
        let (connector, _tx) = MockConnector::with_pong(Arc::clone(&pong));
        let (writer, _count) = MockWriter::new();
        let runner = Runner::new("test-feed", connector, writer);

        // The watchdog must observe the same counter the shards write to.
        assert!(Arc::ptr_eq(&runner.pong_handle(), &pong));
        pong.store(1_234, Ordering::SeqCst);
        assert_eq!(runner.pong_handle().load(Ordering::SeqCst), 1_234);
    }

    #[test]
    fn runner_pong_handle_is_zero_when_connector_has_none() {
        let (connector, _tx) = MockConnector::new();
        let (writer, _count) = MockWriter::new();
        let runner = Runner::new("test-feed", connector, writer);

        // Zero reads as "no pong signal", so those connectors keep data-only staleness.
        assert_eq!(runner.pong_handle().load(Ordering::SeqCst), 0);
    }

    #[test]
    fn runner_pong_handle_rereads_the_connector() {
        // A connector that allocates its pong tracker inside connect() must not be
        // snapshotted at construction time: the runner would hold a permanently zero
        // handle and silently degrade to data-only liveness with nothing failing.
        let cell: Arc<std::sync::Mutex<Option<Arc<AtomicU64>>>> =
            Arc::new(std::sync::Mutex::new(None));
        let (connector, _tx) = LatePongConnector::new(Arc::clone(&cell));
        let (writer, _count) = MockWriter::new();
        let runner = Runner::new("test-feed", connector, writer);

        assert_eq!(
            runner.pong_handle().load(Ordering::SeqCst),
            0,
            "before connect there is no pong signal"
        );

        let tracker = Arc::new(AtomicU64::new(4_242));
        *cell.lock().unwrap() = Some(Arc::clone(&tracker));

        assert!(
            Arc::ptr_eq(&runner.pong_handle(), &tracker),
            "after connect the runner must see the real tracker"
        );
        assert_eq!(runner.pong_handle().load(Ordering::SeqCst), 4_242);
    }

    #[test]
    fn runner_exposes_connector_data_handle() {
        let data = Arc::new(AtomicU64::new(0));
        let (connector, _tx) = MockConnector::with_data(Arc::clone(&data));
        let (writer, _count) = MockWriter::new();
        let runner = Runner::new("test-feed", connector, writer);

        // The watchdog must read the connector's forwarded-data clock, not a copy
        // and not the activity clock.
        assert!(Arc::ptr_eq(&runner.data_handle(), &data));
    }

    #[test]
    fn runner_data_handle_is_distinct_from_its_activity_handle() {
        // If these ever collapse to one Arc the ceiling is back on any-frame age,
        // which is the bug this whole layer exists to close.
        let data = Arc::new(AtomicU64::new(0));
        let (connector, _tx) = MockConnector::with_data(Arc::clone(&data));
        let (writer, _count) = MockWriter::new();
        let runner = Runner::new("test-feed", connector, writer);

        assert!(!Arc::ptr_eq(
            &runner.data_handle(),
            &runner.activity_handle()
        ));
    }

    #[test]
    fn runner_data_handle_falls_back_to_its_own_clock() {
        // A connector without its own data clock gets the runner's, which is stamped
        // on successful writes only, so a dropped control frame can never advance it.
        let (connector, _tx) = MockConnector::new();
        let (writer, _count) = MockWriter::new();
        let runner = Runner::new("test-feed", connector, writer);

        let handle = runner.data_handle();
        assert_eq!(
            handle.load(Ordering::SeqCst),
            0,
            "uninitialized until connect, which suppresses the check rather than failing it"
        );

        runner.start_data_clock();
        assert!(
            runner.data_handle().load(Ordering::SeqCst) > 0,
            "connect must start the clock, or a connector that never publishes is exempt forever"
        );
    }

    #[test]
    fn watchdog_skips_the_tick_when_the_data_clock_is_uninitialized() {
        // Before connect there is no age to compare. Treating that as age 0 would be
        // a silent green light; treating it as infinite would kill every connector at
        // startup. Neither: no verdict.
        let now_epoch = 1_000_000;
        assert_eq!(
            crate::liveness::data_age_signal(0, now_epoch).age_secs(),
            None
        );
    }

    #[test]
    fn watchdog_skips_the_tick_when_the_data_clock_stepped_backwards() {
        // Matches the pong treatment: a future epoch is evidence the clock is
        // untrustworthy, not evidence the feed is healthy. The watchdog warns with
        // reason = "data_clock_skew" and makes no verdict.
        let now_epoch = 1_000_000;
        let skewed = crate::liveness::data_age_signal(now_epoch + 600, now_epoch);
        assert!(matches!(
            skewed,
            crate::liveness::DataAgeSignal::FutureEpoch { skew_secs: 600 }
        ));
        assert_eq!(skewed.age_secs(), None);
    }

    #[tokio::test]
    async fn runner_write_advances_the_data_clock() {
        // End to end: the fallback clock must actually move when data flows, or
        // every connector without its own clock trips the ceiling 30 minutes in.
        let (connector, msg_tx) = MockConnector::new();
        let (writer, _count) = MockWriter::new();
        let mut runner = Runner::new("test-feed", connector, writer);
        let data = runner.data_handle();
        let (shutdown_tx, shutdown_rx) = tokio::sync::watch::channel(false);

        let handle = tokio::spawn(async move { runner.run(shutdown_rx).await });
        msg_tx.send((now_tsc(), b"{}".to_vec())).await.unwrap();
        tokio::time::sleep(tokio::time::Duration::from_millis(50)).await;
        shutdown_tx.send(true).unwrap();
        handle.await.unwrap().unwrap();

        assert!(data.load(Ordering::SeqCst) > 0);
    }

    #[tokio::test]
    async fn test_runner_processes_messages() {
        let (connector, msg_tx) = MockConnector::new();
        let (writer, write_count) = MockWriter::new();

        let mut runner = Runner::new("test-feed", connector, writer);
        let (shutdown_tx, shutdown_rx) = tokio::sync::watch::channel(false);

        // Spawn runner
        let handle = tokio::spawn(async move {
            runner.run(shutdown_rx).await
        });

        // Send a message with TSC timestamp
        msg_tx.send((now_tsc(), b"{\"test\":true}".to_vec())).await.unwrap();

        // Wait a bit for processing
        tokio::time::sleep(tokio::time::Duration::from_millis(50)).await;

        // Shutdown
        shutdown_tx.send(true).unwrap();
        handle.await.unwrap().unwrap();

        assert!(write_count.load(Ordering::SeqCst) >= 1);
    }
}
