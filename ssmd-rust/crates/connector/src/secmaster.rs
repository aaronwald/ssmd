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

/// Market response from secmaster API (minimal fields needed)
#[derive(Debug, Deserialize)]
struct MarketResponse {
    ticker: String,
}

/// Client for querying secmaster API
pub struct SecmasterClient {
    client: Client,
    base_url: String,
    retry_attempts: u32,
    retry_delay_ms: u64,
}

impl SecmasterClient {
    /// Create a new secmaster client with default retry config (3 attempts, 1000ms base delay)
    pub fn new(base_url: &str) -> Self {
        Self::with_retry(base_url, 3, 1000)
    }

    /// Create a new secmaster client with custom retry configuration
    pub fn with_retry(base_url: &str, retry_attempts: u32, retry_delay_ms: u64) -> Self {
        let client = Client::builder()
            .timeout(Duration::from_secs(30))
            .build()
            .expect("Failed to create HTTP client");

        Self {
            client,
            base_url: base_url.trim_end_matches('/').to_string(),
            retry_attempts,
            retry_delay_ms,
        }
    }

    /// Fetch market tickers for a single category with retry logic
    pub async fn get_markets_by_category(
        &self,
        category: &str,
    ) -> Result<Vec<String>, SecmasterError> {
        let url = format!(
            "{}/v1/markets?category={}&status=active&limit=10000",
            self.base_url,
            urlencoding::encode(category)
        );

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

            match self.client.get(&url).send().await {
                Ok(response) => {
                    if response.status().is_success() {
                        let markets: Vec<MarketResponse> = response.json().await?;
                        let tickers: Vec<String> = markets.into_iter().map(|m| m.ticker).collect();

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
    pub async fn get_markets_by_categories(
        &self,
        categories: &[String],
    ) -> Result<Vec<String>, SecmasterError> {
        if categories.is_empty() {
            return Ok(Vec::new());
        }

        info!(categories = ?categories, "Loading markets from secmaster");

        let mut all_tickers = HashSet::new();

        for category in categories {
            match self.get_markets_by_category(category).await {
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
    }
}
