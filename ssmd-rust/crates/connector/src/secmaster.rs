//! Secmaster HTTP client for fetching market tickers by category

use reqwest::Client;
use serde::Deserialize;
use std::collections::HashSet;
use std::time::Duration;
use thiserror::Error;
use tokio::time::sleep;
use tracing::{debug, info, warn};

#[derive(Error, Debug)]
pub enum SecmasterError {
    #[error("HTTP request failed: {0}")]
    Request(#[from] reqwest::Error),

    #[error("No markets found for categories: {0:?}")]
    NoMarketsFound(Vec<String>),

    #[error("Secmaster returned error: {status} - {message}")]
    ApiError { status: u16, message: String },
}

/// Market item from secmaster API (minimal fields needed)
#[derive(Debug, Deserialize)]
struct MarketItem {
    ticker: String,
}

/// Wrapper for markets API response
#[derive(Debug, Deserialize)]
struct MarketsResponse {
    markets: Vec<MarketItem>,
    /// PostgreSQL WAL LSN at snapshot time (for CDC filtering)
    #[serde(default)]
    snapshot_lsn: Option<String>,
    /// ISO timestamp when snapshot was taken (for CDC ByStartTime)
    #[serde(default)]
    snapshot_time: Option<String>,
}

/// Result of fetching markets with CDC snapshot metadata
#[derive(Debug, Clone)]
pub struct MarketsWithSnapshot {
    pub tickers: Vec<String>,
    /// PostgreSQL WAL LSN at snapshot time (for CDC filtering)
    pub snapshot_lsn: String,
    /// ISO timestamp when snapshot was taken (for CDC ByStartTime)
    pub snapshot_time: String,
}

/// Event data from secmaster API (for category lookup)
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EventData {
    pub event_ticker: String,
    pub title: String,
    pub category: String,
    #[serde(default)]
    pub series_ticker: Option<String>,
    #[serde(default)]
    pub status: Option<String>,
}

/// Series item from secmaster API
#[derive(Debug, Clone, Deserialize)]
pub struct SeriesItem {
    pub ticker: String,
    pub title: String,
    pub category: String,
    pub is_game: bool,
}

/// Wrapper for series API response
#[derive(Debug, Deserialize)]
struct SeriesResponse {
    series: Vec<SeriesItem>,
}

/// Client for querying secmaster API
pub struct SecmasterClient {
    client: Client,
    base_url: String,
    api_key: Option<String>,
    retry_attempts: u32,
    retry_delay_ms: u64,
}

impl SecmasterClient {
    /// Create a new secmaster client with default retry config (3 attempts, 1000ms base delay)
    pub fn new(base_url: &str) -> Self {
        Self::with_config(base_url, None, 3, 1000)
    }

    /// Create a new secmaster client with custom retry configuration
    pub fn with_retry(base_url: &str, retry_attempts: u32, retry_delay_ms: u64) -> Self {
        Self::with_config(base_url, None, retry_attempts, retry_delay_ms)
    }

    /// Create a new secmaster client with full configuration including API key
    pub fn with_config(
        base_url: &str,
        api_key: Option<String>,
        retry_attempts: u32,
        retry_delay_ms: u64,
    ) -> Self {
        let client = Client::builder()
            .timeout(Duration::from_secs(30))
            .build()
            .expect("Failed to create HTTP client");

        Self {
            client,
            base_url: base_url.trim_end_matches('/').to_string(),
            api_key,
            retry_attempts,
            retry_delay_ms,
        }
    }

    /// Fetch market tickers for a single category with retry logic
    /// If close_within_hours is Some, only returns markets closing within that many hours
    pub async fn get_markets_by_category(
        &self,
        category: &str,
        close_within_hours: Option<u32>,
    ) -> Result<Vec<String>, SecmasterError> {
        let mut url = format!(
            "{}/v1/markets?category={}&status=active&limit=10000",
            self.base_url,
            urlencoding::encode(category)
        );
        if let Some(hours) = close_within_hours {
            url.push_str(&format!("&close_within_hours={}", hours));
        }

        let mut last_error = None;

        for attempt in 0..=self.retry_attempts {
            if attempt > 0 {
                let delay = self.retry_delay_ms * 2u64.pow(attempt - 1);
                warn!(
                    attempt = attempt,
                    delay_ms = delay,
                    category = %category,
                    "Retrying secmaster request"
                );
                sleep(Duration::from_millis(delay)).await;
            }

            debug!(url = %url, category = %category, attempt = attempt, "Fetching markets from secmaster");

            let mut request = self.client.get(&url);
            if let Some(ref api_key) = self.api_key {
                request = request.header("X-Api-Key", api_key);
            }

            match request.send().await {
                Ok(response) => {
                    if response.status().is_success() {
                        let response_body: MarketsResponse = response.json().await?;
                        let tickers: Vec<String> = response_body.markets.into_iter().map(|m| m.ticker).collect();

                        info!(
                            category = %category,
                            market_count = tickers.len(),
                            "Fetched markets for category"
                        );
                        return Ok(tickers);
                    } else {
                        let status = response.status().as_u16();
                        let message = response.text().await.unwrap_or_default();
                        last_error = Some(SecmasterError::ApiError { status, message });
                        // Retry on 5xx errors, fail fast on 4xx
                        if status < 500 {
                            break;
                        }
                    }
                }
                Err(e) => {
                    last_error = Some(SecmasterError::Request(e));
                }
            }
        }

        Err(last_error.unwrap_or_else(|| SecmasterError::NoMarketsFound(vec![category.to_string()])))
    }

    /// Fetch market tickers for multiple categories, merged and deduped
    /// If close_within_hours is Some, only returns markets closing within that many hours
    pub async fn get_markets_by_categories(
        &self,
        categories: &[String],
        close_within_hours: Option<u32>,
    ) -> Result<Vec<String>, SecmasterError> {
        if categories.is_empty() {
            return Ok(Vec::new());
        }

        info!(categories = ?categories, close_within_hours = ?close_within_hours, "Loading markets from secmaster");

        let mut all_tickers = HashSet::new();

        for category in categories {
            match self.get_markets_by_category(category, close_within_hours).await {
                Ok(tickers) => {
                    all_tickers.extend(tickers);
                }
                Err(e) => {
                    warn!(category = %category, error = %e, "Failed to fetch category, continuing");
                }
            }
        }

        if all_tickers.is_empty() {
            return Err(SecmasterError::NoMarketsFound(categories.to_vec()));
        }

        let mut tickers: Vec<String> = all_tickers.into_iter().collect();
        tickers.sort();

        info!(
            total_markets = tickers.len(),
            "Combined unique markets for subscription"
        );

        Ok(tickers)
    }

    /// Fetch market tickers with CDC snapshot metadata (snapshot_time, snapshot_lsn)
    /// Used by connectors with CDC enabled to synchronize market fetch with CDC stream
    pub async fn get_markets_with_snapshot(
        &self,
        category: &str,
        close_within_hours: Option<u32>,
    ) -> Result<MarketsWithSnapshot, SecmasterError> {
        let mut url = format!(
            "{}/v1/markets?category={}&status=active&limit=10000&include_snapshot=true",
            self.base_url,
            urlencoding::encode(category)
        );
        if let Some(hours) = close_within_hours {
            url.push_str(&format!("&close_within_hours={}", hours));
        }

        let mut last_error = None;

        for attempt in 0..=self.retry_attempts {
            if attempt > 0 {
                let delay = self.retry_delay_ms * 2u64.pow(attempt - 1);
                warn!(
                    attempt = attempt,
                    delay_ms = delay,
                    category = %category,
                    "Retrying secmaster request"
                );
                sleep(Duration::from_millis(delay)).await;
            }

            debug!(url = %url, category = %category, attempt = attempt, "Fetching markets with snapshot from secmaster");

            let mut request = self.client.get(&url);
            if let Some(ref api_key) = self.api_key {
                request = request.header("X-Api-Key", api_key);
            }

            match request.send().await {
                Ok(response) => {
                    if response.status().is_success() {
                        let response_body: MarketsResponse = response.json().await?;
                        let tickers: Vec<String> = response_body.markets.into_iter().map(|m| m.ticker).collect();

                        // Use defaults if snapshot not returned (backwards compatibility)
                        let snapshot_lsn = response_body.snapshot_lsn.unwrap_or_else(|| "0/0".to_string());
                        let snapshot_time = response_body.snapshot_time.unwrap_or_else(|| {
                            chrono::Utc::now().to_rfc3339()
                        });

                        info!(
                            category = %category,
                            market_count = tickers.len(),
                            snapshot_lsn = %snapshot_lsn,
                            snapshot_time = %snapshot_time,
                            "Fetched markets with snapshot for category"
                        );

                        return Ok(MarketsWithSnapshot {
                            tickers,
                            snapshot_lsn,
                            snapshot_time,
                        });
                    } else {
                        let status = response.status().as_u16();
                        let message = response.text().await.unwrap_or_default();
                        last_error = Some(SecmasterError::ApiError { status, message });
                        if status < 500 {
                            break;
                        }
                    }
                }
                Err(e) => {
                    last_error = Some(SecmasterError::Request(e));
                }
            }
        }

        Err(last_error.unwrap_or_else(|| SecmasterError::NoMarketsFound(vec![category.to_string()])))
    }

    /// Fetch market tickers for multiple categories with CDC snapshot metadata
    /// Returns the snapshot from the first successful category fetch
    pub async fn get_markets_by_categories_with_snapshot(
        &self,
        categories: &[String],
        close_within_hours: Option<u32>,
    ) -> Result<MarketsWithSnapshot, SecmasterError> {
        if categories.is_empty() {
            return Ok(MarketsWithSnapshot {
                tickers: Vec::new(),
                snapshot_lsn: "0/0".to_string(),
                snapshot_time: chrono::Utc::now().to_rfc3339(),
            });
        }

        info!(categories = ?categories, close_within_hours = ?close_within_hours, "Loading markets with snapshot from secmaster");

        let mut all_tickers = HashSet::new();
        let mut earliest_snapshot_lsn: Option<String> = None;
        let mut earliest_snapshot_time: Option<String> = None;

        for category in categories {
            match self.get_markets_with_snapshot(category, close_within_hours).await {
                Ok(result) => {
                    all_tickers.extend(result.tickers);
                    // Keep the earliest (first) snapshot for consistency
                    if earliest_snapshot_lsn.is_none() {
                        earliest_snapshot_lsn = Some(result.snapshot_lsn);
                        earliest_snapshot_time = Some(result.snapshot_time);
                    }
                }
                Err(e) => {
                    warn!(category = %category, error = %e, "Failed to fetch category, continuing");
                }
            }
        }

        if all_tickers.is_empty() {
            return Err(SecmasterError::NoMarketsFound(categories.to_vec()));
        }

        let mut tickers: Vec<String> = all_tickers.into_iter().collect();
        tickers.sort();

        let snapshot_lsn = earliest_snapshot_lsn.unwrap_or_else(|| "0/0".to_string());
        let snapshot_time = earliest_snapshot_time.unwrap_or_else(|| chrono::Utc::now().to_rfc3339());

        info!(
            total_markets = tickers.len(),
            snapshot_lsn = %snapshot_lsn,
            snapshot_time = %snapshot_time,
            "Combined unique markets with snapshot for subscription"
        );

        Ok(MarketsWithSnapshot {
            tickers,
            snapshot_lsn,
            snapshot_time,
        })
    }

    /// Fetch series for a category (optionally filtered to games only)
    pub async fn get_series(
        &self,
        category: &str,
        games_only: bool,
    ) -> Result<Vec<SeriesItem>, SecmasterError> {
        let mut url = format!(
            "{}/v1/series?category={}&limit=10000",
            self.base_url,
            urlencoding::encode(category)
        );
        if games_only {
            url.push_str("&games_only=true");
        }

        debug!(url = %url, category = %category, "Fetching series from secmaster");

        let mut request = self.client.get(&url);
        if let Some(ref api_key) = self.api_key {
            request = request.header("X-Api-Key", api_key);
        }

        let response = request.send().await?;

        if response.status().is_success() {
            let response_body: SeriesResponse = response.json().await?;
            info!(
                category = %category,
                series_count = response_body.series.len(),
                "Fetched series for category"
            );
            Ok(response_body.series)
        } else {
            let status = response.status().as_u16();
            let message = response.text().await.unwrap_or_default();
            Err(SecmasterError::ApiError { status, message })
        }
    }

    /// Fetch market tickers by series ticker
    pub async fn get_markets_by_series(
        &self,
        series_ticker: &str,
    ) -> Result<Vec<String>, SecmasterError> {
        // Use the /v1/markets endpoint with series filter
        let url = format!(
            "{}/v1/markets?series={}&status=active&limit=10000",
            self.base_url,
            urlencoding::encode(series_ticker)
        );

        debug!(url = %url, series_ticker = %series_ticker, "Fetching markets by series from secmaster");

        let mut request = self.client.get(&url);
        if let Some(ref api_key) = self.api_key {
            request = request.header("X-Api-Key", api_key);
        }

        let response = request.send().await?;

        if response.status().is_success() {
            let response_body: MarketsResponse = response.json().await?;
            let tickers: Vec<String> = response_body.markets.into_iter().map(|m| m.ticker).collect();
            info!(
                series_ticker = %series_ticker,
                market_count = tickers.len(),
                "Fetched markets for series"
            );
            Ok(tickers)
        } else {
            let status = response.status().as_u16();
            let message = response.text().await.unwrap_or_default();
            Err(SecmasterError::ApiError { status, message })
        }
    }

    /// Fetch market tickers using series-based approach
    /// This is faster than category-based as it only fetches series we care about
    pub async fn get_markets_by_series_list(
        &self,
        category: &str,
        games_only: bool,
    ) -> Result<Vec<String>, SecmasterError> {
        // Step 1: Get series for this category
        let series_list = self.get_series(category, games_only).await?;

        if series_list.is_empty() {
            return Err(SecmasterError::NoMarketsFound(vec![category.to_string()]));
        }

        info!(
            category = %category,
            series_count = series_list.len(),
            "Found series, fetching markets"
        );

        // Step 2: For each series, get markets
        let mut all_tickers = HashSet::new();

        for series in &series_list {
            match self.get_markets_by_series(&series.ticker).await {
                Ok(tickers) => {
                    all_tickers.extend(tickers);
                }
                Err(e) => {
                    warn!(series = %series.ticker, error = %e, "Failed to fetch series markets, continuing");
                }
            }
        }

        if all_tickers.is_empty() {
            return Err(SecmasterError::NoMarketsFound(vec![category.to_string()]));
        }

        let mut tickers: Vec<String> = all_tickers.into_iter().collect();
        tickers.sort();

        info!(
            category = %category,
            total_markets = tickers.len(),
            series_count = series_list.len(),
            "Combined markets from series"
        );

        Ok(tickers)
    }

    /// Fetch a single event by ticker (for CDC category lookup)
    ///
    /// Returns None if the event is not found (404).
    pub async fn get_event(&self, event_ticker: &str) -> Result<Option<EventData>, SecmasterError> {
        let url = format!(
            "{}/v1/events/{}",
            self.base_url,
            urlencoding::encode(event_ticker)
        );

        debug!(url = %url, event_ticker = %event_ticker, "Fetching event from secmaster");

        let mut request = self.client.get(&url);
        if let Some(ref api_key) = self.api_key {
            request = request.header("X-Api-Key", api_key);
        }

        let response = request.send().await?;

        if response.status().is_success() {
            let event: EventData = response.json().await?;
            debug!(
                event_ticker = %event_ticker,
                category = %event.category,
                "Fetched event"
            );
            Ok(Some(event))
        } else if response.status().as_u16() == 404 {
            debug!(event_ticker = %event_ticker, "Event not found");
            Ok(None)
        } else {
            let status = response.status().as_u16();
            let message = response.text().await.unwrap_or_default();
            Err(SecmasterError::ApiError { status, message })
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_client_creation() {
        let client = SecmasterClient::new("http://localhost:3000");
        assert_eq!(client.base_url, "http://localhost:3000");
        assert_eq!(client.retry_attempts, 3);
        assert_eq!(client.retry_delay_ms, 1000);
    }

    #[test]
    fn test_client_strips_trailing_slash() {
        let client = SecmasterClient::new("http://localhost:3000/");
        assert_eq!(client.base_url, "http://localhost:3000");
    }

    #[test]
    fn test_client_with_retry_config() {
        let client = SecmasterClient::with_retry("http://localhost:3000", 5, 500);
        assert_eq!(client.base_url, "http://localhost:3000");
        assert_eq!(client.retry_attempts, 5);
        assert_eq!(client.retry_delay_ms, 500);
        assert!(client.api_key.is_none());
    }

    #[test]
    fn test_client_with_api_key() {
        let client = SecmasterClient::with_config(
            "http://localhost:3000",
            Some("test-api-key".to_string()),
            3,
            1000,
        );
        assert_eq!(client.base_url, "http://localhost:3000");
        assert_eq!(client.api_key, Some("test-api-key".to_string()));
    }
}
