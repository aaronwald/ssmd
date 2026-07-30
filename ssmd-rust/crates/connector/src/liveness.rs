//! Shared liveness rules for connector staleness checks.
//!
//! A WebSocket connection is alive if EITHER market data OR a protocol pong
//! arrived recently. Sparse feeds (Kalshi 15-minute crypto markets, for
//! example) go legitimately quiet for minutes between contract windows while
//! the socket stays healthy, so a data-only staleness check kills a working
//! connector.
//!
//! These helpers are the single definition of that rule, shared by the outer
//! watchdog (`crate::runner`) and the `/health` endpoint (`crate::server`).
//! The per-shard check in `kalshi::connector` implements the data-or-pong half
//! of the rule inline against its shard-local pong tracker. It deliberately has
//! no ceiling of its own: at a 90s threshold it is the fast detector, and the
//! two layers here are the ones that bound total silent data loss.
//!
//! # Why a pong alone is not enough
//!
//! The pong signal is *peer controlled*: the exchange keeps the connection
//! looking alive simply by emitting one control frame per interval, and
//! `kalshi::websocket` also bumps the tracker on a peer-initiated Ping. A peer
//! that acks our subscribe and then publishes nothing while still answering
//! control frames would keep every layer green forever, so the connector would
//! capture nothing, log nothing, and never restart.
//!
//! So every staleness check also applies an absolute ceiling on the *data* age
//! that no pong can suppress ([`evaluate_staleness`]). The ceiling is a
//! backstop, never an accelerant: it is raised to the normal threshold when it
//! is configured below it, so it can never fire earlier than the plain
//! data-only rule did before pongs were taken into account.
//!
//! # What "data age" means here
//!
//! The ceiling is only that backstop if the clock it reads cannot be reset by
//! the peer's control frames. A socket that answers our subscribe with `ok`,
//! rejects something with `error`, or emits a frame type we do not recognise is
//! producing *traffic*, not data: none of it is forwarded downstream and none of
//! it is archived. Kalshi resubscribes on every 15-minute rollover, so that
//! traffic arrives on a roughly 900s cadence against a 1800s ceiling — enough to
//! hold a ceiling built on "any parseable frame" open indefinitely, during
//! exactly the drought it exists to end.
//!
//! So `data_age_secs` throughout this module means the age of the last message a
//! connector actually forwarded downstream. Callers must feed it a clock stamped
//! only on that event ([`DataAgeSignal`]), never the any-frame activity clock that
//! the per-shard check and the shard metrics use.

use std::sync::atomic::{AtomicU64, Ordering};
use tracing::warn;

/// Absolute ceiling on the age of the most recent market data, in seconds.
///
/// No pong can suppress this check, so it is the last independent backstop
/// against a peer that answers control frames while publishing nothing. Thirty
/// minutes caps silent loss at about two fifteen-minute archive slots while
/// leaving roughly twice the headroom over the longest legitimately quiet gap
/// observed on the sparse Kalshi feeds (under fifteen minutes).
pub const DEFAULT_MAX_DATA_AGE_SECS: u64 = 1800;

/// Lower bound accepted for an operator-supplied staleness threshold.
///
/// Below this the kubelet restarts the pod faster than a connection can be
/// established and resubscribed, which turns a misconfiguration into a restart
/// loop that resubscribes hundreds of markets every couple of minutes.
pub const MIN_THRESHOLD_SECS: u64 = 30;

/// Upper bound accepted for an operator-supplied staleness threshold.
///
/// Above this the check stops being a check: `u64::MAX` in particular makes
/// every comparison false and silently disables the liveness probe.
pub const MAX_THRESHOLD_SECS: u64 = 3600;

/// How a pong epoch reads against the current clock.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PongSignal {
    /// Epoch 0: no pong has been received yet, or the connector does not track
    /// pongs at all.
    Absent,
    /// A pong was received this many seconds ago.
    Age(u64),
    /// The pong epoch is ahead of the current clock by `skew_secs`, so the
    /// clock stepped backwards (NTP correction, VM migration, host suspend).
    FutureEpoch { skew_secs: u64 },
}

impl PongSignal {
    /// Usable pong age, or `None` when there is no trustworthy pong signal.
    ///
    /// Both `Absent` and `FutureEpoch` return `None` so callers fall back to the
    /// data age. A future epoch is evidence that the clock cannot be trusted,
    /// not evidence that the socket is healthy.
    pub fn age_secs(self) -> Option<u64> {
        match self {
            PongSignal::Age(age_secs) => Some(age_secs),
            PongSignal::Absent | PongSignal::FutureEpoch { .. } => None,
        }
    }
}

/// Interpret a pong epoch against `now_epoch`.
///
/// Callers that can log (the watchdog) should match on the returned signal so a
/// `FutureEpoch` anomaly is visible instead of being silently swallowed.
pub fn pong_signal(last_pong_epoch: u64, now_epoch: u64) -> PongSignal {
    if last_pong_epoch == 0 {
        return PongSignal::Absent;
    }
    if last_pong_epoch > now_epoch {
        return PongSignal::FutureEpoch {
            skew_secs: last_pong_epoch.saturating_sub(now_epoch),
        };
    }
    PongSignal::Age(now_epoch.saturating_sub(last_pong_epoch))
}

/// Age in seconds of a pong epoch relative to `now_epoch`.
///
/// Returns `None` when `last_pong_epoch` is 0, which means no pong has been
/// received yet (or the connector does not track pongs at all), and also when
/// the epoch is in the future, which means the clock is untrustworthy. Callers
/// must treat `None` as "no pong signal available" and fall back to the data
/// age: a zero epoch must never be read as an infinitely old pong, nor as a
/// fresh one.
pub fn pong_age_secs(last_pong_epoch: u64, now_epoch: u64) -> Option<u64> {
    pong_signal(last_pong_epoch, now_epoch).age_secs()
}

/// How a data epoch reads against the current clock.
///
/// Unlike [`PongSignal`] there is no second signal behind this one: the data clock
/// is what the ceiling exists to measure. So when its age cannot be trusted the
/// caller has nothing to fall back to and must say so, never substitute a number.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DataAgeSignal {
    /// Epoch 0: the connector has not started its data clock yet.
    ///
    /// Connectors stamp the clock once when the connection comes up, precisely so
    /// this state ends at connect rather than at the first message. If it ended at
    /// the first message, a peer that acked our subscribe and then published
    /// nothing would sit in it forever and no ceiling could ever fire on it.
    Uninitialized,
    /// A message was last forwarded downstream this many seconds ago.
    Age(u64),
    /// The data epoch is ahead of the current clock by `skew_secs`, so the clock
    /// stepped backwards (NTP correction, VM migration, host suspend).
    ///
    /// Reading this as age 0 is what makes clock skew dangerous here: the normal
    /// rule goes green, the ceiling goes green, and `pong_suppressed` stays false,
    /// so even the "quiet but alive" warning falls silent. Every liveness layer is
    /// suppressed at once and nothing says so. Callers must surface it instead.
    FutureEpoch { skew_secs: u64 },
}

impl DataAgeSignal {
    /// Usable data age, or `None` when no trustworthy age can be computed.
    pub fn age_secs(self) -> Option<u64> {
        match self {
            DataAgeSignal::Age(age_secs) => Some(age_secs),
            DataAgeSignal::Uninitialized | DataAgeSignal::FutureEpoch { .. } => None,
        }
    }
}

/// Interpret a data epoch against `now_epoch`.
///
/// Callers that can log (the watchdog) should match on the returned signal so a
/// `FutureEpoch` anomaly is visible instead of being silently swallowed.
pub fn data_age_signal(last_data_epoch: u64, now_epoch: u64) -> DataAgeSignal {
    if last_data_epoch == 0 {
        return DataAgeSignal::Uninitialized;
    }
    if last_data_epoch > now_epoch {
        return DataAgeSignal::FutureEpoch {
            skew_secs: last_data_epoch.saturating_sub(now_epoch),
        };
    }
    DataAgeSignal::Age(now_epoch.saturating_sub(last_data_epoch))
}

/// Stamp the data clock at `now_epoch`.
///
/// Call this ONLY where a message is actually handed downstream. Every frame that
/// is counted and dropped — subscribe/unsubscribe acks, `ok`, `error`, unrecognised
/// types — must leave this clock untouched, or the ceiling stops being a backstop
/// against a peer that emits nothing else.
///
/// `fetch_max` rather than `store` because the connector-wide clock is written by
/// every shard: two shards forwarding at the same moment must not be able to step
/// it backwards by whichever one lands second.
pub fn record_data(tracker: &AtomicU64, now_epoch: u64) {
    tracker.fetch_max(now_epoch, Ordering::SeqCst);
}

/// Effective staleness of a connection in seconds.
///
/// The connection is alive if either signal is recent, so staleness is the
/// minimum of the two ages. `pong_age_secs = None` (no pong yet) falls back to
/// the data age, which reproduces the pre-pong behaviour exactly.
pub fn effective_staleness_secs(data_age_secs: u64, pong_age_secs: Option<u64>) -> u64 {
    match pong_age_secs {
        Some(pong_age) => data_age_secs.min(pong_age),
        None => data_age_secs,
    }
}

/// Which comparison a call site applies against the normal threshold.
///
/// The watchdog and `/health` historically disagreed by one second; keeping both
/// bounds lets them share this module without changing either behaviour.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ThresholdBound {
    /// Stale at `age >= threshold`, the watchdog comparison.
    Inclusive,
    /// Stale at `age > threshold`, the `/health` comparison.
    Exclusive,
}

impl ThresholdBound {
    fn exceeded(self, age_secs: u64, threshold_secs: u64) -> bool {
        match self {
            ThresholdBound::Inclusive => age_secs >= threshold_secs,
            ThresholdBound::Exclusive => age_secs > threshold_secs,
        }
    }
}

/// Full staleness verdict for one connection, as seen at one instant.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StalenessVerdict {
    /// The connection should be treated as dead: either neither signal is recent,
    /// or the data-age ceiling was breached regardless of the pong.
    pub stale: bool,
    /// No market data within the threshold, whatever the socket is doing. This is
    /// the readiness verdict; it never restarts anything on its own.
    pub data_stale: bool,
    /// The data age reached the absolute ceiling, so no pong could vouch for it.
    pub ceiling_breached: bool,
    /// Data is past the threshold but a fresh pong is holding the connection
    /// alive. Healthy for a sparse feed, and the exact shape of a peer that acks
    /// and then goes silent, so it must be logged rather than hidden.
    pub pong_suppressed: bool,
    /// Minimum of the data age and the pong age.
    pub effective_staleness_secs: u64,
    /// The ceiling actually applied, after the never-an-accelerant guard.
    pub effective_ceiling_secs: u64,
}

/// Single definition of the staleness decision, shared by the watchdog and
/// `/health` so the two layers cannot drift apart.
///
/// Stale when the normal rule fires (`min(data_age, pong_age)` past
/// `threshold_secs`) OR when `data_age` alone reaches `max_data_age_secs`. The
/// second half is deliberately blind to the pong, which makes it the only check a
/// peer that answers control frames but publishes nothing cannot suppress —
/// provided `data_age_secs` comes from a clock stamped only on forwarded messages
/// ([`data_age_signal`]). Hand it the any-frame activity clock instead and the
/// property is gone: the peer's own subscribe acks reset it.
///
/// Misconfiguration precedence: `max_data_age_secs` is raised to
/// `threshold_secs` when it is configured below it. A ceiling under the normal
/// threshold would kill a quiet but healthy feed *earlier* than the original
/// data-only rule did, reintroducing the very crashloop the pong signal exists
/// to prevent. The ceiling may only ever extend the deadline, never shorten it.
pub fn evaluate_staleness(
    data_age_secs: u64,
    pong_age_secs: Option<u64>,
    threshold_secs: u64,
    max_data_age_secs: u64,
    bound: ThresholdBound,
) -> StalenessVerdict {
    let effective_secs = effective_staleness_secs(data_age_secs, pong_age_secs);
    let data_stale = bound.exceeded(data_age_secs, threshold_secs);
    let normal_stale = bound.exceeded(effective_secs, threshold_secs);

    let effective_ceiling_secs = max_data_age_secs.max(threshold_secs);
    // Always inclusive: the ceiling is a hard cap on tolerated silent data loss,
    // so reaching it is enough. It is far from the normal threshold in any sane
    // configuration, which makes the off-by-one immaterial here.
    let ceiling_breached = data_age_secs >= effective_ceiling_secs;

    StalenessVerdict {
        stale: normal_stale || ceiling_breached,
        data_stale,
        ceiling_breached,
        pong_suppressed: data_stale && !normal_stale,
        effective_staleness_secs: effective_secs,
        effective_ceiling_secs,
    }
}

/// Why an operator-supplied threshold was refused.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ThresholdRejection {
    /// Not a non-negative integer, or too large for `u64`.
    Unparsable,
    /// Below [`MIN_THRESHOLD_SECS`].
    BelowMin,
    /// Above [`MAX_THRESHOLD_SECS`].
    AboveMax,
}

/// A threshold after validation, plus the reason a supplied value was refused.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ResolvedThreshold {
    /// The value to use: the supplied value when it was accepted, the violated
    /// bound when it was out of range, the built-in default when it was garbage.
    pub secs: u64,
    /// `Some` only when a value was supplied and refused. `None` covers both
    /// "nothing supplied" and "supplied and accepted".
    pub rejected: Option<ThresholdRejection>,
}

/// Validate a threshold configuration value, failing closed toward operator intent.
///
/// An unparsable value carries no intent, so it falls back to `default_secs`. An
/// out-of-range value does carry one — "longer" or "shorter" — so it is clamped to
/// the bound it violated rather than snapped back to a default that may sit far on
/// the other side of the supplied value.
///
/// That distinction is the whole point of clamping. The watchdog default is 120s
/// while the value deployed for the sparse Kalshi feeds is 3600s, exactly
/// [`MAX_THRESHOLD_SECS`]. An operator nudging it to 5400 to buy a little more
/// tolerance would, under fall-back-to-default, get 120s instead: a thirtyfold
/// *reduction* on a feed picked for hour-long quiet periods, and a restart loop
/// that an unvalidated parse would never have produced. Clamping to 3600 still
/// fails closed — the operator does not get the unbounded value they asked for —
/// but it fails in the direction they were reaching.
///
/// Split from the environment lookup so the policy is testable without touching
/// process-global state.
pub fn resolve_threshold_secs(raw: Option<&str>, default_secs: u64) -> ResolvedThreshold {
    debug_assert!(
        (MIN_THRESHOLD_SECS..=MAX_THRESHOLD_SECS).contains(&default_secs),
        "built-in defaults must satisfy the range they enforce"
    );
    let reject = |reason: ThresholdRejection, secs: u64| ResolvedThreshold {
        secs,
        rejected: Some(reason),
    };

    let Some(raw) = raw else {
        return ResolvedThreshold {
            secs: default_secs,
            rejected: None,
        };
    };
    let Ok(value) = raw.trim().parse::<u64>() else {
        // Garbage says nothing about which direction the operator wanted.
        return reject(ThresholdRejection::Unparsable, default_secs);
    };
    if value < MIN_THRESHOLD_SECS {
        return reject(ThresholdRejection::BelowMin, MIN_THRESHOLD_SECS);
    }
    if value > MAX_THRESHOLD_SECS {
        return reject(ThresholdRejection::AboveMax, MAX_THRESHOLD_SECS);
    }
    ResolvedThreshold {
        secs: value,
        rejected: None,
    }
}

/// Read a validated threshold from the environment, warning when a supplied
/// value is refused so the substitution is never silent.
pub fn threshold_from_env(var_name: &str, default_secs: u64) -> u64 {
    let (supplied, resolved) = match std::env::var(var_name) {
        Ok(value) => {
            let resolved = resolve_threshold_secs(Some(&value), default_secs);
            (value, resolved)
        }
        Err(std::env::VarError::NotPresent) => {
            (String::new(), resolve_threshold_secs(None, default_secs))
        }
        // A value that is not valid UTF-8 is one we were handed and cannot read,
        // not one that was never supplied. `std::env::var().ok()` collapses the two,
        // which would make this the single rejection class that substitutes a
        // threshold with nothing in the logs.
        Err(std::env::VarError::NotUnicode(raw)) => (
            raw.to_string_lossy().into_owned(),
            ResolvedThreshold {
                secs: default_secs,
                rejected: Some(ThresholdRejection::Unparsable),
            },
        ),
    };
    if let Some(reason) = resolved.rejected {
        warn!(
            env_var = var_name,
            supplied = %supplied,
            reason = ?reason,
            min_secs = MIN_THRESHOLD_SECS,
            max_secs = MAX_THRESHOLD_SECS,
            using_secs = resolved.secs,
            "Refusing the supplied staleness threshold, using {}s instead",
            resolved.secs
        );
    }
    resolved.secs
}

/// Rate limiter for the "quiet but alive" warning.
///
/// The watchdog ticks every 15s, so warning on every tick would emit 240 lines
/// per hour of a legitimately sparse market and bury the signal it exists to
/// raise. Warning only on the transition into the state would instead let a
/// silence that started before the current log window look like silence that
/// never happened. So: warn on entry, then repeat at most every
/// [`SuppressionWarnGate::REPEAT_SECS`], which guarantees any five-minute slice
/// of logs shows the state while cutting the volume by a factor of twenty.
#[derive(Debug, Default)]
pub struct SuppressionWarnGate {
    /// `None` while the state is not active, which is what re-arms the gate.
    last_warn_epoch: Option<u64>,
}

impl SuppressionWarnGate {
    /// Minimum spacing between repeated warnings for one continuous episode.
    pub const REPEAT_SECS: u64 = 300;

    pub fn new() -> Self {
        Self::default()
    }

    /// Record the current state and report whether to emit a warning now.
    pub fn should_warn(&mut self, suppressed: bool, now_epoch: u64) -> bool {
        if !suppressed {
            self.last_warn_epoch = None;
            return false;
        }
        let due = match self.last_warn_epoch {
            None => true,
            Some(last) => now_epoch.saturating_sub(last) >= Self::REPEAT_SECS,
        };
        if due {
            self.last_warn_epoch = Some(now_epoch);
        }
        due
    }
}

/// Record a pong at `now_epoch` into the shard-local tracker and, when present,
/// the connector-wide shared tracker.
///
/// The shared tracker uses `fetch_max` so concurrent shards can only move it
/// forward: the shared value is the most recent pong seen on any shard.
pub fn record_pong(local: &AtomicU64, shared: Option<&AtomicU64>, now_epoch: u64) {
    local.store(now_epoch, Ordering::SeqCst);
    if let Some(shared) = shared {
        shared.fetch_max(now_epoch, Ordering::SeqCst);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    #[test]
    fn pong_age_none_when_never_received() {
        assert_eq!(pong_age_secs(0, 1_000), None);
    }

    #[test]
    fn pong_age_is_difference_from_now() {
        assert_eq!(pong_age_secs(940, 1_000), Some(60));
    }

    // A future pong epoch used to saturate to Some(0), i.e. a maximally fresh
    // pong, which disabled the watchdog for the duration of the skew. That case
    // now lives in pong_age_is_none_when_epoch_is_in_the_future and
    // pong_signal_reports_skew_when_epoch_is_in_the_future.

    #[test]
    fn staleness_uses_pong_when_data_is_idle() {
        // Sparse market: no data for 400s but a pong 10s ago, so the socket is alive.
        assert_eq!(effective_staleness_secs(400, Some(10)), 10);
    }

    #[test]
    fn staleness_uses_data_when_pong_is_old() {
        // Data flowing but no recent pong: still alive via data.
        assert_eq!(effective_staleness_secs(5, Some(400)), 5);
    }

    #[test]
    fn staleness_is_old_when_both_signals_are_old() {
        // Genuinely hung socket: neither signal is recent.
        assert_eq!(effective_staleness_secs(400, Some(400)), 400);
    }

    #[test]
    fn staleness_falls_back_to_data_age_without_pong() {
        assert_eq!(effective_staleness_secs(400, None), 400);
        assert_eq!(effective_staleness_secs(5, None), 5);
    }

    #[test]
    fn record_pong_updates_local_tracker() {
        let local = AtomicU64::new(0);
        record_pong(&local, None, 1_000);
        assert_eq!(local.load(Ordering::SeqCst), 1_000);
    }

    #[test]
    fn record_pong_updates_shared_tracker() {
        let local = AtomicU64::new(0);
        let shared = Arc::new(AtomicU64::new(0));
        record_pong(&local, Some(&shared), 1_000);
        assert_eq!(local.load(Ordering::SeqCst), 1_000);
        assert_eq!(shared.load(Ordering::SeqCst), 1_000);
    }

    #[test]
    fn record_pong_never_moves_shared_tracker_backwards() {
        // Two shards racing: an older pong must not overwrite a newer one.
        let local = AtomicU64::new(0);
        let shared = Arc::new(AtomicU64::new(2_000));
        record_pong(&local, Some(&shared), 1_000);
        assert_eq!(local.load(Ordering::SeqCst), 1_000, "shard-local is exact");
        assert_eq!(
            shared.load(Ordering::SeqCst),
            2_000,
            "shared tracker only moves forward"
        );
    }

    // ---- clock skew (a pong epoch ahead of now) --------------------------------

    #[test]
    fn pong_signal_absent_when_never_received() {
        assert_eq!(pong_signal(0, 1_000), PongSignal::Absent);
    }

    #[test]
    fn pong_signal_reports_skew_when_epoch_is_in_the_future() {
        // A pong stamped in the future means the clock stepped backwards (NTP
        // correction, VM migration). That is evidence the clock cannot be trusted,
        // NOT evidence that the socket is healthy.
        assert_eq!(
            pong_signal(1_100, 1_000),
            PongSignal::FutureEpoch { skew_secs: 100 }
        );
    }

    #[test]
    fn pong_age_is_none_when_epoch_is_in_the_future() {
        // Fails open to the data age instead of reporting a maximally fresh pong,
        // which would disable the watchdog for the whole duration of the skew.
        assert_eq!(pong_age_secs(1_100, 1_000), None);
        assert_eq!(
            effective_staleness_secs(400, pong_age_secs(1_100, 1_000)),
            400
        );
    }

    // ---- absolute data-age ceiling ---------------------------------------------

    const THRESHOLD: u64 = 300;
    const CEILING: u64 = 1_800;

    #[test]
    fn ceiling_kills_a_ponging_but_silent_peer() {
        // THE regression guard. Kalshi acks our subscribe and then publishes
        // nothing while still answering control frames. The pong keeps the normal
        // rule happy forever, so the data-age ceiling is the only thing that fires.
        let v = evaluate_staleness(
            CEILING,
            Some(5),
            THRESHOLD,
            CEILING,
            ThresholdBound::Inclusive,
        );
        assert!(
            v.stale,
            "a ponging but silent peer must eventually be killed"
        );
        assert!(v.ceiling_breached);
        assert!(v.data_stale);
    }

    #[test]
    fn ceiling_kills_a_ponging_but_silent_peer_on_the_exclusive_bound_too() {
        // /health compares with > for the normal threshold; the ceiling still fires.
        let v = evaluate_staleness(
            CEILING + 1,
            Some(5),
            THRESHOLD,
            CEILING,
            ThresholdBound::Exclusive,
        );
        assert!(v.stale);
        assert!(v.ceiling_breached);
    }

    #[test]
    fn quiet_feed_below_the_ceiling_stays_alive() {
        // The point of the pong rule: a 15-minute Kalshi window can idle well past
        // the 300s threshold on a healthy socket without being restarted.
        let v = evaluate_staleness(
            CEILING - 1,
            Some(5),
            THRESHOLD,
            CEILING,
            ThresholdBound::Inclusive,
        );
        assert!(
            !v.stale,
            "under the ceiling a fresh pong still proves liveness"
        );
        assert!(!v.ceiling_breached);
        assert!(
            v.pong_suppressed,
            "but an operator must be able to see the suppressed state"
        );
    }

    #[test]
    fn ceiling_never_fires_before_the_normal_threshold() {
        // Misconfiguration guard: a ceiling below the normal threshold would make a
        // quiet but ponging feed die EARLIER than the unfixed code did, which is the
        // crashloop this change exists to remove. The ceiling is a backstop, never
        // an accelerant.
        let v = evaluate_staleness(100, Some(5), THRESHOLD, 60, ThresholdBound::Inclusive);
        assert!(!v.stale);
        assert!(!v.ceiling_breached);
        assert_eq!(v.effective_ceiling_secs, THRESHOLD);
    }

    #[test]
    fn ceiling_is_irrelevant_while_data_is_flowing() {
        // The ceiling measures data age, so fresh data can never breach it.
        let v = evaluate_staleness(5, Some(5), THRESHOLD, CEILING, ThresholdBound::Inclusive);
        assert!(!v.stale);
        assert!(!v.data_stale);
        assert!(!v.pong_suppressed);
    }

    #[test]
    fn normal_threshold_still_fires_without_a_pong() {
        // Pre-pong behaviour is preserved exactly: with no pong signal the data-only
        // rule fires at the normal threshold, long before the ceiling.
        let v = evaluate_staleness(
            THRESHOLD,
            None,
            THRESHOLD,
            CEILING,
            ThresholdBound::Inclusive,
        );
        assert!(v.stale);
        assert!(
            !v.ceiling_breached,
            "the normal rule fired, not the ceiling"
        );
    }

    #[test]
    fn bounds_preserve_each_call_site_comparison() {
        // The watchdog fires AT the threshold (>=), /health only past it (>).
        assert!(
            evaluate_staleness(
                THRESHOLD,
                None,
                THRESHOLD,
                CEILING,
                ThresholdBound::Inclusive
            )
            .stale
        );
        assert!(
            !evaluate_staleness(
                THRESHOLD,
                None,
                THRESHOLD,
                CEILING,
                ThresholdBound::Exclusive
            )
            .stale
        );
        assert!(
            evaluate_staleness(
                THRESHOLD + 1,
                None,
                THRESHOLD,
                CEILING,
                ThresholdBound::Exclusive
            )
            .stale
        );
    }

    #[test]
    fn data_stale_is_independent_of_the_pong() {
        // /ready keeps the data-only verdict even while /health stays 200.
        let v = evaluate_staleness(400, Some(1), THRESHOLD, CEILING, ThresholdBound::Exclusive);
        assert!(v.data_stale, "no data is no data");
        assert!(!v.stale, "but the socket is alive, so do not restart it");
    }

    // ---- threshold configuration validation -------------------------------------

    #[test]
    fn threshold_unset_uses_the_default_without_complaint() {
        let r = resolve_threshold_secs(None, 300);
        assert_eq!(r.secs, 300);
        assert!(r.rejected.is_none());
    }

    #[test]
    fn threshold_accepts_values_inside_the_range() {
        assert_eq!(resolve_threshold_secs(Some("900"), 300).secs, 900);
        assert_eq!(resolve_threshold_secs(Some(" 900 "), 300).secs, 900);
        assert_eq!(
            resolve_threshold_secs(Some("30"), 300).secs,
            MIN_THRESHOLD_SECS
        );
        assert_eq!(
            resolve_threshold_secs(Some("3600"), 300).secs,
            MAX_THRESHOLD_SECS
        );
    }

    #[test]
    fn threshold_zero_is_clamped_to_the_minimum() {
        // 0 means a permanent 503: the kubelet restarts every 90-120s and every
        // restart resubscribes hundreds of markets, which can trip exchange-side
        // rate limits on our own trading account. Clamped to the floor, not to the
        // default, so an operator asking for "as tight as possible" gets the
        // tightest value that is safe.
        let r = resolve_threshold_secs(Some("0"), 300);
        assert_eq!(r.secs, MIN_THRESHOLD_SECS);
        assert_eq!(r.rejected, Some(ThresholdRejection::BelowMin));
    }

    #[test]
    fn threshold_u64_max_is_clamped_to_the_maximum() {
        // u64::MAX means no age can ever exceed it, so the probe never fails: the
        // liveness check would be permanently disabled.
        let r = resolve_threshold_secs(Some(&u64::MAX.to_string()), 300);
        assert_eq!(r.secs, MAX_THRESHOLD_SECS);
        assert_eq!(r.rejected, Some(ThresholdRejection::AboveMax));
    }

    #[test]
    fn above_max_clamps_down_to_the_bound_not_back_to_the_default() {
        // The one direction in which validation could restart something the
        // unvalidated parse would not have. WATCHDOG_STALENESS_SECS ships at 3600,
        // exactly MAX, on feeds chosen for hour-long quiet periods, and its built-in
        // default is 120. Nudging it to 5400 must not collapse it to 120 — a
        // thirtyfold cut and a crashloop.
        let watchdog_default = 120;
        let r = resolve_threshold_secs(Some("5400"), watchdog_default);
        assert_eq!(r.secs, MAX_THRESHOLD_SECS);
        assert_ne!(r.secs, watchdog_default, "must not collapse to the default");
        assert!(r.secs > 3_600 - 1);
    }

    #[test]
    fn below_min_clamps_up_to_the_bound_not_back_to_the_default() {
        // Symmetric case: an operator asking for 5s wants a tight check, so give
        // them the tightest safe one rather than a default that may be far looser
        // than what they asked for.
        let r = resolve_threshold_secs(Some("5"), 1_800);
        assert_eq!(r.secs, MIN_THRESHOLD_SECS);
        assert_ne!(r.secs, 1_800, "must not jump to a much looser default");
    }

    #[test]
    fn threshold_garbage_and_overflow_fail_closed_to_the_default() {
        for raw in ["", "abc", "-1", "5.5", "99999999999999999999999999"] {
            let r = resolve_threshold_secs(Some(raw), 300);
            assert_eq!(r.secs, 300, "raw = {raw:?}");
            assert_eq!(
                r.rejected,
                Some(ThresholdRejection::Unparsable),
                "raw = {raw:?}"
            );
        }
    }

    #[test]
    fn threshold_just_outside_the_range_is_rejected() {
        assert_eq!(
            resolve_threshold_secs(Some("29"), 300).rejected,
            Some(ThresholdRejection::BelowMin)
        );
        assert_eq!(
            resolve_threshold_secs(Some("3601"), 300).rejected,
            Some(ThresholdRejection::AboveMax)
        );
    }

    #[test]
    fn threshold_from_env_uses_the_default_when_unset() {
        std::env::remove_var("SSMD_TEST_THRESHOLD_UNSET");
        assert_eq!(threshold_from_env("SSMD_TEST_THRESHOLD_UNSET", 300), 300);
    }

    #[test]
    fn threshold_from_env_clamps_an_out_of_range_value() {
        std::env::set_var("SSMD_TEST_THRESHOLD_HIGH", "5400");
        assert_eq!(
            threshold_from_env("SSMD_TEST_THRESHOLD_HIGH", 120),
            MAX_THRESHOLD_SECS
        );
        std::env::remove_var("SSMD_TEST_THRESHOLD_HIGH");
    }

    #[test]
    #[cfg(unix)]
    fn threshold_from_env_rejects_a_non_utf8_value_instead_of_ignoring_it() {
        use std::ffi::OsString;
        use std::os::unix::ffi::OsStringExt;

        // std::env::var().ok() maps NotUnicode to None, i.e. "nothing supplied", so
        // this was the only rejection class that substituted a threshold with
        // nothing in the logs. It must land on the same path as any other garbage.
        std::env::set_var(
            "SSMD_TEST_THRESHOLD_NON_UTF8",
            OsString::from_vec(vec![0x39, 0x30, 0xff]),
        );
        assert_eq!(
            threshold_from_env("SSMD_TEST_THRESHOLD_NON_UTF8", 300),
            300,
            "unreadable input carries no intent, so it falls back to the default"
        );
        std::env::remove_var("SSMD_TEST_THRESHOLD_NON_UTF8");
    }

    // ---- data clock -------------------------------------------------------------

    #[test]
    fn data_age_is_uninitialized_at_epoch_zero() {
        assert_eq!(data_age_signal(0, 1_000), DataAgeSignal::Uninitialized);
        assert_eq!(data_age_signal(0, 1_000).age_secs(), None);
    }

    #[test]
    fn data_age_is_the_difference_from_now() {
        assert_eq!(data_age_signal(400, 1_000), DataAgeSignal::Age(600));
    }

    #[test]
    fn data_age_reports_skew_when_the_epoch_is_in_the_future() {
        // A backwards clock step used to saturate to age 0. That reads as perfectly
        // fresh data: the normal rule, the ceiling and the pong_suppressed warn all
        // go quiet at once, so every liveness layer is disabled with nothing said.
        assert_eq!(
            data_age_signal(1_600, 1_000),
            DataAgeSignal::FutureEpoch { skew_secs: 600 }
        );
        assert_eq!(
            data_age_signal(1_600, 1_000).age_secs(),
            None,
            "callers must skip the check, not read it as age zero"
        );
    }

    #[test]
    fn record_data_starts_and_advances_the_clock() {
        let clock = AtomicU64::new(0);
        record_data(&clock, 1_000);
        assert_eq!(clock.load(Ordering::SeqCst), 1_000);
        record_data(&clock, 1_500);
        assert_eq!(clock.load(Ordering::SeqCst), 1_500);
    }

    #[test]
    fn record_data_never_moves_the_clock_backwards() {
        // Shards share one connector-wide clock; a late write must not buy silence.
        let clock = AtomicU64::new(2_000);
        record_data(&clock, 1_000);
        assert_eq!(clock.load(Ordering::SeqCst), 2_000);
    }

    // ---- suppressed-state warn gate ---------------------------------------------

    #[test]
    fn warn_gate_fires_on_entering_the_suppressed_state() {
        let mut gate = SuppressionWarnGate::new();
        assert!(gate.should_warn(true, 1_000));
    }

    #[test]
    fn warn_gate_is_quiet_while_the_state_persists() {
        let mut gate = SuppressionWarnGate::new();
        assert!(gate.should_warn(true, 1_000));
        // The watchdog ticks every 15s; a warn per tick would bury the log.
        assert!(!gate.should_warn(true, 1_015));
        assert!(!gate.should_warn(true, 1_299));
    }

    #[test]
    fn warn_gate_repeats_so_a_long_silence_stays_visible() {
        let mut gate = SuppressionWarnGate::new();
        assert!(gate.should_warn(true, 1_000));
        assert!(
            gate.should_warn(true, 1_000 + SuppressionWarnGate::REPEAT_SECS),
            "any 5-minute window of logs shows the suppressed state"
        );
    }

    #[test]
    fn warn_gate_rearms_after_recovery() {
        let mut gate = SuppressionWarnGate::new();
        assert!(gate.should_warn(true, 1_000));
        assert!(!gate.should_warn(false, 1_010), "recovery is not a warn");
        assert!(
            gate.should_warn(true, 1_020),
            "re-entering the state warns again immediately"
        );
    }

    #[test]
    fn warn_gate_never_warns_while_healthy() {
        let mut gate = SuppressionWarnGate::new();
        assert!(!gate.should_warn(false, 1_000));
        assert!(!gate.should_warn(false, 9_000));
    }
}
