//! Pure symbology helpers: extract series ticker and coin from a Kalshi
//! market ticker, and detect 15-minute series.

/// Series ticker = the segment before the first '-'.
///
/// Pure structural parse. A ticker with no '-' separator IS its own series
/// (this is the defined contract, not an error fallback). Callers gate on
/// [`is_15m`] before trusting that a ticker is a real 15-minute market.
pub fn series_of(ticker: &str) -> &str {
    ticker.split('-').next().unwrap_or(ticker)
}

/// Coin = series with leading "KX" and trailing "15M" removed.
///
/// Prefix and suffix are stripped independently so that removing one does not
/// cause the other to fall back to the full series. A series missing the
/// "KX" prefix or "15M" suffix passes that segment through unchanged by
/// design — this is a pure transform over already-validated 15M series.
pub fn coin_of(series: &str) -> String {
    let no_prefix = series.strip_prefix("KX").unwrap_or(series);
    let coin = no_prefix.strip_suffix("15M").unwrap_or(no_prefix);
    coin.to_string()
}

/// True if the ticker's series ends with "15M". This is the validation gate:
/// callers MUST check this before treating a ticker as a 15-minute market.
pub fn is_15m(ticker: &str) -> bool {
    series_of(ticker).ends_with("15M")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn series_of_extracts_first_segment() {
        assert_eq!(series_of("KXBTC15M-26JUN031400-15"), "KXBTC15M");
        assert_eq!(series_of("KXBTC15M"), "KXBTC15M");
    }

    #[test]
    fn coin_of_strips_kx_and_suffix() {
        assert_eq!(coin_of("KXBTC15M"), "BTC");
        assert_eq!(coin_of("KXHYPE15M"), "HYPE");
    }

    #[test]
    fn is_15m_matches_series_suffix() {
        assert!(is_15m("KXBTC15M-26JUN031400-15"));
        assert!(!is_15m("KXBTCD-26JUN0314"));
    }
}
