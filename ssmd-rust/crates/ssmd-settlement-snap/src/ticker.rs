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
    fn distinct_tickers_kept_separately() {
        let map = LastTickMap::new();
        map.update("A", sample_tick(10, 1));
        map.update("B", sample_tick(20, 2));
        assert_eq!(map.get("A").unwrap().last_price, Some(10));
        assert_eq!(map.get("B").unwrap().last_price, Some(20));
        assert_eq!(map.len(), 2);
    }
}
