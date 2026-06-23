//! Pure, side-effect-free 1-minute OHLCV aggregation.
//!
//! Inputs arrive as either massive 1-second OHLCV aggregates or kraken-spot
//! trades. Both are normalized into [`Input`] and folded into a per-symbol
//! current-minute [`Bar`] by [`MinuteAggregator`]. The aggregator is
//! deterministic and does no I/O — Redis/NATS wiring lives in the binary.
//!
//! ## Idempotency
//! Re-delivery is expected. Each [`Input`] carries a `dedup_key` that is unique
//! within a minute for a given source contribution:
//! - massive 1s bars: keyed by their 1-second start (`s`), so a resent second
//!   replaces (not re-adds) its OHLCV — volume never double-counts.
//! - kraken trades: keyed by `trade_id`, so a replayed trade is ignored.

use std::collections::HashMap;

use serde::{Deserialize, Serialize};
use tracing::warn;

/// Milliseconds in one minute.
const MINUTE_MS: i64 = 60_000;

/// Floor a millisecond epoch timestamp to the start of its minute.
pub fn minute_floor(ts_ms: i64) -> i64 {
    ts_ms - (ts_ms % MINUTE_MS)
}

/// A finalized or in-progress 1-minute OHLCV bar for one symbol.
///
/// Serialized as JSON for the Redis ring (see [`crate::store`]).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Bar {
    pub sym: String,
    pub o: f64,
    pub h: f64,
    pub l: f64,
    pub c: f64,
    pub v: f64,
    /// Minute-floored start (inclusive), epoch ms.
    pub start_ts_ms: i64,
    /// Exclusive end of the minute (`start_ts_ms + 60_000`), epoch ms.
    pub end_ts_ms: i64,
}

/// A normalized aggregation input derived from a source message.
///
/// For massive 1s bars `o/h/l/c` are the second's own OHLC and `v` is its
/// volume. For kraken trades `o == h == l == c == price` and `v == qty`.
#[derive(Debug, Clone, PartialEq)]
pub struct Input {
    pub sym: String,
    pub o: f64,
    pub h: f64,
    pub l: f64,
    pub c: f64,
    pub v: f64,
    /// Source event timestamp (epoch ms); decides which minute this belongs to
    /// and the within-minute ordering.
    pub ts_ms: i64,
    /// Key that is stable across re-deliveries of the same contribution within
    /// a minute (massive: the 1s start; kraken: the trade id).
    pub dedup_key: String,
}

// ---------------------------------------------------------------------------
// Parsers
// ---------------------------------------------------------------------------

/// Raw massive 1-second OHLCV aggregate (`ev:"A"`).
#[derive(Debug, Deserialize)]
struct MassiveRaw {
    sym: String,
    o: f64,
    h: f64,
    l: f64,
    c: f64,
    v: f64,
    /// Window start, epoch ms.
    s: i64,
    /// Window end, epoch ms.
    #[allow(dead_code)]
    e: i64,
}

/// Parse a massive 1s OHLCV aggregate payload into zero or one [`Input`].
///
/// Returns an empty `Vec` (logging a warning) on malformed JSON or missing
/// fields, rather than panicking — a bad message must never take down the
/// consumer. The `Vec` return mirrors [`parse_kraken_trade`] so both feeds share
/// one parser contract; massive payloads are always a single flat object.
pub fn parse_massive_1s(payload: &[u8]) -> Vec<Input> {
    let raw: MassiveRaw = match serde_json::from_slice(payload) {
        Ok(r) => r,
        Err(err) => {
            warn!(error = %err, "skipping malformed massive 1s aggregate");
            return Vec::new();
        }
    };

    vec![Input {
        sym: raw.sym,
        o: raw.o,
        h: raw.h,
        l: raw.l,
        c: raw.c,
        v: raw.v,
        ts_ms: raw.s,
        // The 1-second start uniquely identifies this contribution in a minute.
        dedup_key: raw.s.to_string(),
    }]
}

/// The kraken-spot v2 envelope: a `channel`/`type` header wrapping a `data[]`
/// array. Only `channel == "trade"` carries trades; other channels (heartbeat,
/// ticker, subscribe acks) have no `data` we consume.
#[derive(Debug, Deserialize)]
struct KrakenEnvelope {
    channel: String,
    #[serde(default)]
    data: Vec<KrakenTradeRaw>,
}

/// One trade element inside a kraken-spot v2 `trade` envelope's `data[]` array.
#[derive(Debug, Deserialize)]
struct KrakenTradeRaw {
    symbol: String,
    price: f64,
    qty: f64,
    /// ISO-8601 timestamp string.
    timestamp: String,
    /// Trade id (string); used for de-duplication.
    trade_id: Option<String>,
}

/// Parse a kraken-spot v2 trade envelope into zero or more [`Input`]s.
///
/// The connector publishes the Kraken v2 wire envelope verbatim:
/// `{"channel":"trade","type":"update","data":[{...},{...}]}`. Each element of
/// `data[]` becomes one [`Input`]; a single message can therefore yield many
/// trades. Non-trade channels (heartbeat, ticker, subscribe acks) yield an empty
/// `Vec`. Malformed JSON yields an empty `Vec` (logged). Within a `trade`
/// envelope, an element with an unparseable timestamp is skipped (logged) while
/// its siblings still parse — one bad element never drops the whole message.
pub fn parse_kraken_trade(payload: &[u8]) -> Vec<Input> {
    let env: KrakenEnvelope = match serde_json::from_slice(payload) {
        Ok(e) => e,
        Err(err) => {
            warn!(error = %err, "skipping malformed kraken envelope");
            return Vec::new();
        }
    };

    if env.channel != "trade" {
        // Heartbeat / ticker / subscribe ack — nothing to aggregate.
        return Vec::new();
    }

    let mut inputs = Vec::with_capacity(env.data.len());
    for raw in env.data {
        let ts_ms = match parse_iso8601_ms(&raw.timestamp) {
            Some(ms) => ms,
            None => {
                warn!(ts = %raw.timestamp, "skipping kraken trade with unparseable timestamp");
                continue;
            }
        };

        // Prefer the trade id for dedup; fall back to ts+price+qty if absent.
        let dedup_key = raw
            .trade_id
            .unwrap_or_else(|| format!("{ts_ms}:{}:{}", raw.price, raw.qty));

        inputs.push(Input {
            sym: raw.symbol,
            o: raw.price,
            h: raw.price,
            l: raw.price,
            c: raw.price,
            v: raw.qty,
            ts_ms,
            dedup_key,
        });
    }

    inputs
}

/// Parse an ISO-8601 / RFC-3339 timestamp string to epoch milliseconds.
fn parse_iso8601_ms(s: &str) -> Option<i64> {
    chrono::DateTime::parse_from_rfc3339(s)
        .ok()
        .map(|dt| dt.timestamp_millis())
}

// ---------------------------------------------------------------------------
// Aggregation
// ---------------------------------------------------------------------------

/// A single de-duplicated contribution to a minute, retained so re-delivery
/// replaces rather than re-adds.
#[derive(Debug, Clone)]
struct Contribution {
    o: f64,
    h: f64,
    l: f64,
    c: f64,
    v: f64,
    ts_ms: i64,
}

/// Accumulator for one symbol's current minute.
#[derive(Debug)]
struct SymbolState {
    minute_start: i64,
    /// dedup_key -> latest contribution for that key.
    contribs: HashMap<String, Contribution>,
}

impl SymbolState {
    fn new(minute_start: i64) -> Self {
        SymbolState {
            minute_start,
            contribs: HashMap::new(),
        }
    }

    /// Upsert a contribution, keeping the latest for the dedup key.
    fn upsert(&mut self, key: String, c: Contribution) {
        self.contribs.insert(key, c);
    }

    /// Derive the current [`Bar`] from all retained contributions.
    ///
    /// Contributions are ordered by `ts_ms` (ties broken by insertion is not
    /// needed: 1s bars and trades within the same ms are equivalent for OHLC
    /// purposes — open is earliest, close is latest). Returns `None` if empty.
    fn to_bar(&self, sym: &str) -> Option<Bar> {
        if self.contribs.is_empty() {
            return None;
        }

        let mut ordered: Vec<&Contribution> = self.contribs.values().collect();
        ordered.sort_by_key(|c| c.ts_ms);

        let first = ordered.first().unwrap();
        let last = ordered.last().unwrap();

        let mut high = f64::MIN;
        let mut low = f64::MAX;
        let mut vol = 0.0;
        for c in &ordered {
            if c.h > high {
                high = c.h;
            }
            if c.l < low {
                low = c.l;
            }
            vol += c.v;
        }

        Some(Bar {
            sym: sym.to_string(),
            o: first.o,
            h: high,
            l: low,
            c: last.c,
            v: vol,
            start_ts_ms: self.minute_start,
            end_ts_ms: self.minute_start + MINUTE_MS,
        })
    }
}

/// Result of ingesting a single [`Input`].
#[derive(Debug, Clone, PartialEq)]
pub struct IngestResult {
    /// The previous minute's finalized bar, emitted on a minute rollover.
    pub finalized: Option<Bar>,
    /// The updated current-minute bar after applying the input.
    pub current: Bar,
}

/// Folds normalized [`Input`]s into per-symbol 1-minute [`Bar`]s.
///
/// Keyed by symbol; each symbol tracks exactly one open minute at a time. A
/// later input in a new minute finalizes the prior minute (returned as
/// [`IngestResult::finalized`]) and starts a fresh accumulator.
#[derive(Debug, Default)]
pub struct MinuteAggregator {
    states: HashMap<String, SymbolState>,
}

impl MinuteAggregator {
    pub fn new() -> Self {
        MinuteAggregator {
            states: HashMap::new(),
        }
    }

    /// Ingest one input and return the (optionally finalized previous) and the
    /// updated current bar.
    ///
    /// Late inputs that fall *before* the symbol's current open minute are
    /// applied to that current minute's bar would be wrong, so they are dropped
    /// (the prior minute has already been finalized and emitted). They are
    /// returned as the current bar unchanged with no finalized bar.
    pub fn ingest(&mut self, input: Input) -> IngestResult {
        let minute = minute_floor(input.ts_ms);
        let contrib = Contribution {
            o: input.o,
            h: input.h,
            l: input.l,
            c: input.c,
            v: input.v,
            ts_ms: input.ts_ms,
        };

        let mut finalized = None;

        match self.states.get(&input.sym) {
            Some(state) if minute > state.minute_start => {
                // Rollover: finalize the prior minute, start the new one.
                finalized = state.to_bar(&input.sym);
                let mut fresh = SymbolState::new(minute);
                fresh.upsert(input.dedup_key, contrib);
                let current = fresh.to_bar(&input.sym).expect("just inserted");
                self.states.insert(input.sym.clone(), fresh);
                return IngestResult { finalized, current };
            }
            Some(state) if minute < state.minute_start => {
                // Late arrival for an already-finalized minute: drop it, return
                // the current bar unchanged.
                let current = state.to_bar(&input.sym).expect("non-empty state");
                return IngestResult {
                    finalized: None,
                    current,
                };
            }
            _ => {}
        }

        // Same minute (or first input for this symbol): upsert into current.
        let state = self
            .states
            .entry(input.sym.clone())
            .or_insert_with(|| SymbolState::new(minute));
        state.upsert(input.dedup_key, contrib);
        let current = state.to_bar(&input.sym).expect("just inserted");

        IngestResult { finalized, current }
    }

    /// Finalize and remove the current open bar for `sym`, if any.
    ///
    /// Retained for a future shutdown / explicit flush to emit a still-open
    /// minute; the consumer loop does not call it yet (the forming minute is
    /// written to Redis on every ingest, so an unclean stop loses at most the
    /// in-flight minute, which is recoverable from the source on restart).
    #[allow(dead_code)]
    pub fn flush(&mut self, sym: &str) -> Option<Bar> {
        self.states.remove(sym).and_then(|s| s.to_bar(sym))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn massive(sym: &str, sec_start_ms: i64, o: f64, h: f64, l: f64, c: f64, v: f64) -> Vec<u8> {
        format!(
            r#"{{"ev":"A","sym":"{sym}","o":{o},"h":{h},"l":{l},"c":{c},"v":{v},"vw":{c},"s":{sec_start_ms},"e":{}}}"#,
            sec_start_ms + 1000
        )
        .into_bytes()
    }

    /// A kraken v2 `trade` envelope wrapping a single trade in `data[]`.
    fn kraken(sym: &str, price: f64, qty: f64, ts: &str, trade_id: &str) -> Vec<u8> {
        format!(
            r#"{{"channel":"trade","type":"update","data":[{{"symbol":"{sym}","price":{price},"qty":{qty},"side":"buy","ord_type":"market","trade_id":"{trade_id}","timestamp":"{ts}"}}]}}"#
        )
        .into_bytes()
    }

    /// Build a kraken v2 `trade` envelope from N (sym, price, qty, ts, id) trades.
    fn kraken_envelope(trades: &[(&str, f64, f64, &str, &str)]) -> Vec<u8> {
        let elems: Vec<String> = trades
            .iter()
            .map(|(sym, price, qty, ts, id)| {
                format!(
                    r#"{{"symbol":"{sym}","price":{price},"qty":{qty},"side":"buy","ord_type":"market","trade_id":"{id}","timestamp":"{ts}"}}"#
                )
            })
            .collect();
        format!(
            r#"{{"channel":"trade","type":"update","data":[{}]}}"#,
            elems.join(",")
        )
        .into_bytes()
    }

    /// Parse a single-input payload (massive, or single-trade kraken) and assert
    /// exactly one [`Input`] came back, returning it.
    fn one(inputs: Vec<Input>) -> Input {
        assert_eq!(inputs.len(), 1, "expected exactly one input");
        inputs.into_iter().next().unwrap()
    }

    #[test]
    fn minute_floor_floors_to_minute() {
        assert_eq!(minute_floor(0), 0);
        assert_eq!(minute_floor(59_999), 0);
        assert_eq!(minute_floor(60_000), 60_000);
        assert_eq!(minute_floor(60_001), 60_000);
        assert_eq!(minute_floor(125_500), 120_000);
    }

    #[test]
    fn parse_massive_ok() {
        let input = one(parse_massive_1s(&massive(
            "AAPL", 60_000, 10.0, 12.0, 9.0, 11.0, 100.0,
        )));
        assert_eq!(input.sym, "AAPL");
        assert_eq!(input.o, 10.0);
        assert_eq!(input.h, 12.0);
        assert_eq!(input.l, 9.0);
        assert_eq!(input.c, 11.0);
        assert_eq!(input.v, 100.0);
        assert_eq!(input.ts_ms, 60_000);
        assert_eq!(input.dedup_key, "60000");
    }

    #[test]
    fn parse_massive_malformed_is_empty() {
        assert!(parse_massive_1s(b"not json").is_empty());
        // Missing required fields.
        assert!(parse_massive_1s(br#"{"sym":"AAPL"}"#).is_empty());
    }

    #[test]
    fn parse_kraken_ok() {
        let input = one(parse_kraken_trade(&kraken(
            "BTC/USD",
            50000.5,
            0.25,
            "2026-01-01T00:00:30.500Z",
            "uuid-1",
        )));
        assert_eq!(input.sym, "BTC/USD");
        assert_eq!(input.o, 50000.5);
        assert_eq!(input.h, 50000.5);
        assert_eq!(input.l, 50000.5);
        assert_eq!(input.c, 50000.5);
        assert_eq!(input.v, 0.25);
        assert_eq!(input.dedup_key, "uuid-1");
        // 2026-01-01T00:00:30.500Z -> epoch ms
        let expected = chrono::DateTime::parse_from_rfc3339("2026-01-01T00:00:30.500Z")
            .unwrap()
            .timestamp_millis();
        assert_eq!(input.ts_ms, expected);
    }

    #[test]
    fn parse_kraken_envelope_with_three_trades() {
        // The real connector publishes an envelope with a `data[]` array; one
        // message can carry many trades.
        let inputs = parse_kraken_trade(&kraken_envelope(&[
            ("BTC/USD", 100.0, 1.0, "2026-01-01T00:00:05Z", "t1"),
            ("BTC/USD", 110.0, 2.0, "2026-01-01T00:00:25Z", "t2"),
            ("BTC/USD", 90.0, 3.0, "2026-01-01T00:00:45Z", "t3"),
        ]));
        assert_eq!(inputs.len(), 3);

        assert_eq!(inputs[0].sym, "BTC/USD");
        assert_eq!(inputs[0].o, 100.0);
        assert_eq!(inputs[0].v, 1.0);
        assert_eq!(inputs[0].dedup_key, "t1");

        assert_eq!(inputs[1].o, 110.0);
        assert_eq!(inputs[1].v, 2.0);
        assert_eq!(inputs[1].dedup_key, "t2");

        assert_eq!(inputs[2].o, 90.0);
        assert_eq!(inputs[2].v, 3.0);
        assert_eq!(inputs[2].dedup_key, "t3");

        // Timestamps decode in order.
        assert!(inputs[0].ts_ms < inputs[1].ts_ms);
        assert!(inputs[1].ts_ms < inputs[2].ts_ms);
    }

    #[test]
    fn parse_kraken_non_trade_channels_are_empty() {
        // Heartbeat, ticker, and subscribe acks must yield no inputs — not a
        // crash, not a phantom trade.
        let heartbeat = br#"{"channel":"heartbeat","type":"update"}"#;
        let ticker = br#"{"channel":"ticker","type":"update","data":[{"symbol":"BTC/USD","bid":97000.0,"ask":97000.1,"last":97000.0}]}"#;
        let ack = br#"{"method":"subscribe","result":{"channel":"trade","symbol":"BTC/USD"},"success":true}"#;
        assert!(parse_kraken_trade(heartbeat).is_empty());
        assert!(parse_kraken_trade(ticker).is_empty());
        assert!(parse_kraken_trade(ack).is_empty());
    }

    #[test]
    fn parse_kraken_malformed_element_skips_only_that_element() {
        // A trade with an unparseable timestamp is dropped, but its good
        // siblings in the same envelope still parse.
        let inputs = parse_kraken_trade(&kraken_envelope(&[
            ("BTC/USD", 100.0, 1.0, "2026-01-01T00:00:05Z", "good-1"),
            ("BTC/USD", 110.0, 2.0, "not-a-timestamp", "bad"),
            ("BTC/USD", 90.0, 3.0, "2026-01-01T00:00:45Z", "good-2"),
        ]));
        assert_eq!(inputs.len(), 2, "only the two good trades survive");
        assert_eq!(inputs[0].dedup_key, "good-1");
        assert_eq!(inputs[1].dedup_key, "good-2");
    }

    #[test]
    fn parse_kraken_malformed_envelope_is_empty() {
        assert!(parse_kraken_trade(b"{bad").is_empty());
        // Bad timestamp on the sole element → empty.
        assert!(parse_kraken_trade(&kraken("BTC/USD", 1.0, 1.0, "not-a-timestamp", "uuid-x"))
            .is_empty());
    }

    #[test]
    fn sixty_massive_bars_make_one_minute_bar() {
        let mut agg = MinuteAggregator::new();
        let mut last_current: Option<Bar> = None;

        // 60 one-second bars in minute starting at 0. Open walks 10..=69,
        // close walks 11..=70, high peaks at 1000 in the middle, low bottoms
        // at 1 once.
        for i in 0..60i64 {
            let sec = i * 1000;
            let o = (i + 10) as f64;
            let c = (i + 11) as f64;
            let h = if i == 30 { 1000.0 } else { c };
            let l = if i == 40 { 1.0 } else { o };
            let res = agg.ingest(one(parse_massive_1s(&massive("AAPL", sec, o, h, l, c, 2.0))));
            assert!(res.finalized.is_none());
            last_current = Some(res.current);
        }

        let bar = last_current.unwrap();
        assert_eq!(bar.sym, "AAPL");
        assert_eq!(bar.o, 10.0); // first second's open
        assert_eq!(bar.c, 70.0); // last second's close
        assert_eq!(bar.h, 1000.0); // running max
        assert_eq!(bar.l, 1.0); // running min
        assert_eq!(bar.v, 120.0); // 60 * 2.0
        assert_eq!(bar.start_ts_ms, 0);
        assert_eq!(bar.end_ts_ms, 60_000);
    }

    #[test]
    fn kraken_trades_make_correct_ohlcv() {
        let mut agg = MinuteAggregator::new();
        // Three trades in the same minute (00:00), one per single-trade envelope.
        agg.ingest(one(parse_kraken_trade(&kraken(
            "BTC/USD",
            100.0,
            1.0,
            "2026-01-01T00:00:05Z",
            "t1",
        ))));
        agg.ingest(one(parse_kraken_trade(&kraken(
            "BTC/USD",
            110.0,
            2.0,
            "2026-01-01T00:00:25Z",
            "t2",
        ))));
        let res = agg.ingest(one(parse_kraken_trade(&kraken(
            "BTC/USD",
            90.0,
            3.0,
            "2026-01-01T00:00:45Z",
            "t3",
        ))));

        let bar = res.current;
        assert_eq!(bar.o, 100.0); // first trade
        assert_eq!(bar.h, 110.0); // max
        assert_eq!(bar.l, 90.0); // min
        assert_eq!(bar.c, 90.0); // last trade
        assert_eq!(bar.v, 6.0); // 1+2+3
    }

    #[test]
    fn multi_trade_envelope_aggregates_one_minute() {
        // A single envelope carrying 3 trades in the same minute aggregates to
        // one bar: sum of qty, OHLC from the prices in timestamp order. The
        // `one`/`kraken_envelope` helpers are defined at the top of this module.
        let mut agg = MinuteAggregator::new();
        let inputs = parse_kraken_trade(&kraken_envelope(&[
            ("BTC/USD", 100.0, 1.0, "2026-01-01T00:00:05Z", "t1"),
            ("BTC/USD", 110.0, 2.0, "2026-01-01T00:00:25Z", "t2"),
            ("BTC/USD", 90.0, 3.0, "2026-01-01T00:00:45Z", "t3"),
        ]));
        assert_eq!(inputs.len(), 3, "envelope must yield all three trades");

        let mut last = None;
        for input in inputs {
            let res = agg.ingest(input);
            assert!(res.finalized.is_none(), "all in minute 0");
            last = Some(res.current);
        }
        let bar = last.expect("at least one ingest produced a current bar");
        assert_eq!(bar.o, 100.0); // first trade
        assert_eq!(bar.h, 110.0); // max price
        assert_eq!(bar.l, 90.0); // min price
        assert_eq!(bar.c, 90.0); // last trade
        assert_eq!(bar.v, 6.0); // 1+2+3
        // All three trades fall in the same minute, so the bar's start is that
        // minute's floor (the 2026-01-01T00:00 minute, not epoch 0).
        let minute = minute_floor(
            chrono::DateTime::parse_from_rfc3339("2026-01-01T00:00:05Z")
                .unwrap()
                .timestamp_millis(),
        );
        assert_eq!(bar.start_ts_ms, minute);
    }

    #[test]
    fn duplicate_massive_second_does_not_double_count() {
        let mut agg = MinuteAggregator::new();
        agg.ingest(one(parse_massive_1s(&massive("AAPL", 0, 1.0, 1.0, 1.0, 1.0, 5.0))));
        // Resend the same 1s start with a revised volume — must replace, not add.
        let res = agg.ingest(one(parse_massive_1s(&massive(
            "AAPL", 0, 1.0, 1.0, 1.0, 1.0, 7.0,
        ))));
        assert_eq!(res.current.v, 7.0, "resent second replaces, not adds");

        // Add a distinct second.
        let res2 = agg.ingest(one(parse_massive_1s(&massive(
            "AAPL", 1000, 1.0, 1.0, 1.0, 1.0, 3.0,
        ))));
        assert_eq!(res2.current.v, 10.0, "7 (latest of sec0) + 3 (sec1)");
    }

    #[test]
    fn duplicate_kraken_trade_id_does_not_double_count() {
        let mut agg = MinuteAggregator::new();
        agg.ingest(one(parse_kraken_trade(&kraken(
            "BTC/USD",
            100.0,
            1.0,
            "2026-01-01T00:00:05Z",
            "dup",
        ))));
        let res = agg.ingest(one(parse_kraken_trade(&kraken(
            "BTC/USD",
            100.0,
            1.0,
            "2026-01-01T00:00:05Z",
            "dup",
        ))));
        assert_eq!(res.current.v, 1.0, "replayed trade id ignored");
    }

    #[test]
    fn minute_rollover_emits_prior_minute() {
        let mut agg = MinuteAggregator::new();
        // Minute 0.
        agg.ingest(one(parse_massive_1s(&massive("AAPL", 0, 5.0, 5.0, 5.0, 5.0, 4.0))));
        let res = agg.ingest(one(parse_massive_1s(&massive(
            "AAPL", 30_000, 6.0, 6.0, 6.0, 6.0, 2.0,
        ))));
        assert!(res.finalized.is_none(), "still minute 0");

        // First input in minute 1 finalizes minute 0.
        let res = agg.ingest(one(parse_massive_1s(&massive(
            "AAPL", 60_000, 7.0, 7.0, 7.0, 7.0, 1.0,
        ))));
        let finalized = res.finalized.expect("minute 0 should finalize");
        assert_eq!(finalized.start_ts_ms, 0);
        assert_eq!(finalized.end_ts_ms, 60_000);
        assert_eq!(finalized.o, 5.0);
        assert_eq!(finalized.c, 6.0);
        assert_eq!(finalized.v, 6.0); // 4 + 2

        // Current is the new minute.
        assert_eq!(res.current.start_ts_ms, 60_000);
        assert_eq!(res.current.o, 7.0);
        assert_eq!(res.current.v, 1.0);
    }

    #[test]
    fn late_input_for_finalized_minute_is_dropped() {
        let mut agg = MinuteAggregator::new();
        agg.ingest(one(parse_massive_1s(&massive(
            "AAPL", 60_000, 7.0, 7.0, 7.0, 7.0, 1.0,
        ))));
        // A straggler from minute 0 arrives after we moved to minute 1.
        let res = agg.ingest(one(parse_massive_1s(&massive(
            "AAPL", 0, 5.0, 5.0, 5.0, 5.0, 9.0,
        ))));
        assert!(res.finalized.is_none());
        // Current stays the minute-1 bar, unaffected by the straggler.
        assert_eq!(res.current.start_ts_ms, 60_000);
        assert_eq!(res.current.v, 1.0);
    }

    #[test]
    fn separate_symbols_are_independent() {
        let mut agg = MinuteAggregator::new();
        agg.ingest(one(parse_massive_1s(&massive("AAPL", 0, 1.0, 1.0, 1.0, 1.0, 1.0))));
        let res = agg.ingest(one(parse_massive_1s(&massive(
            "MSFT", 0, 2.0, 2.0, 2.0, 2.0, 5.0,
        ))));
        assert_eq!(res.current.sym, "MSFT");
        assert_eq!(res.current.v, 5.0);
    }

    #[test]
    fn flush_emits_open_bar() {
        let mut agg = MinuteAggregator::new();
        // `one()` asserts exactly one parsed input (fail-loud, like the old unwrap).
        agg.ingest(one(parse_massive_1s(&massive("AAPL", 0, 1.0, 1.0, 1.0, 1.0, 3.0))));
        let bar = agg.flush("AAPL").expect("open bar");
        assert_eq!(bar.v, 3.0);
        // Flushed symbol is gone.
        assert!(agg.flush("AAPL").is_none());
    }
}
