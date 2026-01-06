//! Secmaster HTTP client for fetching market tickers by category

use reqwest::Client;
use serde::Deserialize;
use std::collections::HashSet;
use std::time::Duration;
use thiserror::Error;
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
}

impl SecmasterClient {
    /// Create a new secmaster client
    pub fn new(base_url: &str) -> Self {
        let client = Client::builder()
            .timeout(Duration::from_secs(30))
            .build()
            .expect("Failed to create HTTP client");

        Self {
            client,
            base_url: base_url.trim_end_matches('/').to_string(),
        }
    }

    /// Fetch market tickers for a single category
    pub async fn get_markets_by_category(
        &self,
        category: &str,
    ) -> Result<Vec<String>, SecmasterError> {
        let url = format!(
            "{}/v1/markets?category={}&status=active&limit=10000",
            self.base_url,
            urlencoding::encode(category)
        );

        debug!(url = %url, category = %category, "Fetching markets from secmaster");

        let response = self.client.get(&url).send().await?;

        if !response.status().is_success() {
            let status = response.status().as_u16();
            let message = response.text().await.unwrap_or_default();
            return Err(SecmasterError::ApiError { status, message });
        }

        let markets: Vec<MarketResponse> = response.json().await?;
        let tickers: Vec<String> = markets.into_iter().map(|m| m.ticker).collect();

        info!(
            category = %category,
            market_count = tickers.len(),
            "Fetched markets for category"
        );

        Ok(tickers)
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
    }

    #[test]
    fn test_client_strips_trailing_slash() {
        let client = SecmasterClient::new("http://localhost:3000/");
        assert_eq!(client.base_url, "http://localhost:3000");
    }
}
