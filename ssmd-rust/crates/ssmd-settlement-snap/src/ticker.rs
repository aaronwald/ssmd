//! In-process last-tick-per-market map, fed by the `LastPerSubject` ticker
//! consumer. Reading the final tick from here at settlement is race-free,
//! unlike a short-TTL Redis snap key that expires seconds after close.

use dashmap::DashMap;

/// Top-of-book + summary fields from a Kalshi ticker message. Prices are in
/// native Kalshi cents; volume / open_interest are contract counts; `ts` is
/// the exchange epoch-second timestamp of the tick.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LastTick {
    pub yes_bid: Option<i64>,
    pub yes_ask: Option<i64>,
    pub no_bid: Option<i64>,
    pub no_ask: Option<i64>,
    pub last_price: Option<i64>,
    pub volume: Option<i64>,
    pub open_interest: Option<i64>,
    /// Exchange time of the tick (epoch seconds).
    pub ts: i64,
}

/// Concurrent last-tick map keyed by market ticker.
#[derive(Default)]
pub struct LastTickMap {
    inner: DashMap<String, LastTick>,
}

impl LastTickMap {
    pub fn new() -> Self {
        Self {
            inner: DashMap::new(),
        }
    }

    /// Insert or overwrite the last tick for `ticker`.
    pub fn update(&self, ticker: impl Into<String>, tick: LastTick) {
        self.inner.insert(ticker.into(), tick);
    }

    /// Merge `tick` into the stored last tick for `ticker`, preserving prior
    /// non-null fields. `parse_ticker` can return a `LastTick` with some price /
    /// size fields `None` (a degraded or partial ticker near settlement); a plain
    /// overwrite would null out a previously complete snap. So for each optional
    /// field the new value wins when present (`new.or(prior)`), otherwise the
    /// prior non-null value is kept, and `ts` advances to the newer of the two.
    /// With no prior tick this is a plain insert.
    ///
    /// # Concurrency
    /// This get-then-insert read-modify-write is NOT atomic. It is race-free only
    /// because there is exactly ONE writer task (the single `spawn_ticker_task`
    /// consumer); readers merely `get()` a clone. If a second writer is ever
    /// introduced, switch to an atomic `entry().and_modify()`/`alter` instead.
    pub fn merge_update(&self, ticker: impl Into<String>, tick: LastTick) {
        let key = ticker.into();
        // Clone the prior out and DROP the read guard before insert — holding a
        // dashmap `Ref` across an `insert` on the same shard would deadlock.
        let prior = self.inner.get(&key).map(|e| e.value().clone());
        let merged = match prior {
            Some(prior) => LastTick {
                yes_bid: tick.yes_bid.or(prior.yes_bid),
                yes_ask: tick.yes_ask.or(prior.yes_ask),
                no_bid: tick.no_bid.or(prior.no_bid),
                no_ask: tick.no_ask.or(prior.no_ask),
                last_price: tick.last_price.or(prior.last_price),
                volume: tick.volume.or(prior.volume),
                open_interest: tick.open_interest.or(prior.open_interest),
                ts: tick.ts.max(prior.ts),
            },
            None => tick,
        };
        self.inner.insert(key, merged);
    }

    /// Return a clone of the last tick for `ticker`, or `None` if unseen.
    pub fn get(&self, ticker: &str) -> Option<LastTick> {
        self.inner.get(ticker).map(|e| e.value().clone())
    }

    /// Number of distinct tickers currently held.
    pub fn len(&self) -> usize {
        self.inner.len()
    }

    pub fn is_empty(&self) -> bool {
        self.inner.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_tick(last: i64, ts: i64) -> LastTick {
        LastTick {
            yes_bid: Some(48),
            yes_ask: Some(52),
            no_bid: Some(48),
            no_ask: Some(52),
            last_price: Some(last),
            volume: Some(1000),
            open_interest: Some(500),
            ts,
        }
    }

    #[test]
    fn update_then_get_returns_last_tick() {
        let map = LastTickMap::new();
        let tick = sample_tick(50, 1717424100);
        map.update("KXBTC15M-26JUN031400-15", tick.clone());
        assert_eq!(map.get("KXBTC15M-26JUN031400-15"), Some(tick));
    }

    #[test]
    fn second_update_overwrites() {
        let map = LastTickMap::new();
        map.update("KXBTC15M-26JUN031400-15", sample_tick(50, 1717424100));
        let newer = sample_tick(97, 1717424105);
        map.update("KXBTC15M-26JUN031400-15", newer.clone());
        assert_eq!(map.get("KXBTC15M-26JUN031400-15"), Some(newer));
        assert_eq!(map.len(), 1);
    }

    #[test]
    fn get_unknown_ticker_is_none() {
        let map = LastTickMap::new();
        assert_eq!(map.get("KXNOPE15M-1-15"), None);
    }

    #[test]
    fn merge_update_preserves_prior_non_null_on_partial_tick() {
        let map = LastTickMap::new();
        // A complete snap arrives first.
        let complete = LastTick {
            yes_bid: Some(48),
            yes_ask: Some(52),
            no_bid: Some(48),
            no_ask: Some(52),
            last_price: Some(50),
            volume: Some(1000),
            open_interest: Some(500),
            ts: 1717424100,
        };
        map.merge_update("KXBTC15M-1-15", complete);
        // A later PARTIAL/degraded tick: most price/size fields None, newer ts.
        let partial = LastTick {
            yes_bid: None,
            yes_ask: Some(55), // one field updates
            no_bid: None,
            no_ask: None,
            last_price: None,
            volume: None,
            open_interest: None,
            ts: 1717424105,
        };
        map.merge_update("KXBTC15M-1-15", partial);
        let got = map.get("KXBTC15M-1-15").expect("present");
        // The new non-null field wins; every other field keeps the prior value.
        assert_eq!(got.yes_ask, Some(55)); // new non-null wins
        assert_eq!(got.yes_bid, Some(48)); // preserved from complete snap
        assert_eq!(got.no_bid, Some(48)); // preserved
        assert_eq!(got.no_ask, Some(52)); // preserved
        assert_eq!(got.last_price, Some(50)); // preserved (not nulled)
        assert_eq!(got.volume, Some(1000)); // preserved
        assert_eq!(got.open_interest, Some(500)); // preserved
        assert_eq!(got.ts, 1717424105); // advanced to newer
    }

    #[test]
    fn merge_update_inserts_when_no_prior() {
        let map = LastTickMap::new();
        let tick = sample_tick(50, 1717424100);
        map.merge_update("KXBTC15M-1-15", tick.clone());
        assert_eq!(map.get("KXBTC15M-1-15"), Some(tick));
    }

    #[test]
    fn distinct_tickers_kept_separately() {
        let map = LastTickMap::new();
        map.update("A", sample_tick(10, 1));
        map.update("B", sample_tick(20, 2));
        assert_eq!(map.get("A").unwrap().last_price, Some(10));
        assert_eq!(map.get("B").unwrap().last_price, Some(20));
        assert_eq!(map.len(), 2);
    }
}
