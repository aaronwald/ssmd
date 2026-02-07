//! Polymarket market discovery via Gamma REST API
//!
//! Polls `GET https://gamma-api.polymarket.com/markets` to discover active markets.
//! This is net-new functionality — Kalshi uses CDC, Kraken uses static config.
//!
//! Returns `(condition_id, Vec<clob_token_id>)` tuples for subscription management.

use serde::Deserialize;
use std::collections::BTreeSet;
use std::time::Duration;
use thiserror::Error;
use tracing::{debug, info, warn};

/// Gamma API base URL for market discovery
pub const GAMMA_API_URL: &str = "https://gamma-api.polymarket.com";

/// Default poll interval: 5 minutes
pub const DEFAULT_POLL_INTERVAL_SECS: u64 = 300;

/// Default page size for Gamma API pagination
const PAGE_SIZE: u32 = 100;

#[derive(Error, Debug)]
pub enum MarketDiscoveryError {
    #[error("HTTP request failed: {0}")]
    Http(#[from] reqwest::Error),

    #[error("JSON parse error: {0}")]
    Json(String),
}

/// A discovered market with its token IDs
#[derive(Debug, Clone)]
pub struct DiscoveredMarket {
    pub condition_id: String,
    pub clob_token_ids: Vec<String>,
    pub question: String,
    pub active: bool,
}

/// Response from the Gamma /markets endpoint
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct GammaMarketResponse {
    #[serde(default)]
    condition_id: Option<String>,
    #[serde(default)]
    clob_token_ids: Option<Vec<String>>,
    #[serde(default)]
    question: Option<String>,
    #[serde(default)]
    active: Option<bool>,
    #[serde(default)]
    closed: Option<bool>,
}

/// Client for discovering active markets via the Gamma REST API
pub struct MarketDiscovery {
    client: reqwest::Client,
    base_url: String,
    min_volume: Option<f64>,
    min_liquidity: Option<f64>,
}

impl Default for MarketDiscovery {
    fn default() -> Self {
        Self::new()
    }
}

impl MarketDiscovery {
    /// Create a new MarketDiscovery client
    pub fn new() -> Self {
        Self {
            client: reqwest::Client::builder()
                .timeout(Duration::from_secs(30))
                .build()
                .expect("Failed to build HTTP client"),
            base_url: GAMMA_API_URL.to_string(),
            min_volume: None,
            min_liquidity: None,
        }
    }

    /// Set minimum volume filter
    pub fn with_min_volume(mut self, min_volume: f64) -> Self {
        self.min_volume = Some(min_volume);
        self
    }

    /// Set minimum liquidity filter
    pub fn with_min_liquidity(mut self, min_liquidity: f64) -> Self {
        self.min_liquidity = Some(min_liquidity);
        self
    }

    /// Override base URL (for testing)
    #[cfg(test)]
    pub fn with_base_url(mut self, url: impl Into<String>) -> Self {
        self.base_url = url.into();
        self
    }

    /// Fetch all active markets from the Gamma API (paginated).
    /// Returns a list of discovered markets with their token IDs.
    pub async fn fetch_active_markets(&self) -> Result<Vec<DiscoveredMarket>, MarketDiscoveryError> {
        let mut all_markets = Vec::new();
        let mut offset: u32 = 0;

        loop {
            let mut url = format!(
                "{}/markets?active=true&closed=false&limit={}&offset={}",
                self.base_url, PAGE_SIZE, offset
            );

            if let Some(min_vol) = self.min_volume {
                url.push_str(&format!("&volume_num_min={}", min_vol));
            }
            if let Some(min_liq) = self.min_liquidity {
                url.push_str(&format!("&liquidity_num_min={}", min_liq));
            }

            debug!(url = %url, offset = offset, "Fetching markets page");

            let response = self.client.get(&url).send().await?;

            if !response.status().is_success() {
                warn!(
                    status = %response.status(),
                    "Gamma API returned non-success status"
                );
                break;
            }

            let body = response.text().await?;
            let markets: Vec<GammaMarketResponse> = serde_json::from_str(&body)
                .map_err(|e| MarketDiscoveryError::Json(format!("{}: {}", e, &body[..body.len().min(200)])))?;

            let page_count = markets.len();
            debug!(count = page_count, offset = offset, "Fetched markets page");

            for market in markets {
                if let (Some(condition_id), Some(token_ids)) =
                    (market.condition_id, market.clob_token_ids)
                {
                    if token_ids.is_empty() {
                        continue;
                    }

                    // Skip closed markets
                    if market.closed.unwrap_or(false) {
                        continue;
                    }

                    all_markets.push(DiscoveredMarket {
                        condition_id,
                        clob_token_ids: token_ids,
                        question: market.question.unwrap_or_default(),
                        active: market.active.unwrap_or(true),
                    });
                }
            }

            // If we got fewer results than page size, we've reached the end
            if page_count < PAGE_SIZE as usize {
                break;
            }

            offset += PAGE_SIZE;
        }

        info!(
            total_markets = all_markets.len(),
            "Market discovery complete"
        );
        Ok(all_markets)
    }

    /// Extract all unique token IDs from discovered markets.
    /// These are the asset_ids needed for WebSocket subscription.
    /// Uses BTreeSet for deduplication and deterministic ordering (consistent sharding).
    pub fn extract_token_ids(markets: &[DiscoveredMarket]) -> Vec<String> {
        markets
            .iter()
            .flat_map(|m| m.clob_token_ids.iter().cloned())
            .collect::<BTreeSet<_>>()
            .into_iter()
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_token_ids() {
        let markets = vec![
            DiscoveredMarket {
                condition_id: "0xabc".to_string(),
                clob_token_ids: vec!["token_yes_1".to_string(), "token_no_1".to_string()],
                question: "Will X?".to_string(),
                active: true,
            },
            DiscoveredMarket {
                condition_id: "0xdef".to_string(),
                clob_token_ids: vec!["token_yes_2".to_string(), "token_no_2".to_string()],
                question: "Will Y?".to_string(),
                active: true,
            },
        ];

        let token_ids = MarketDiscovery::extract_token_ids(&markets);
        assert_eq!(token_ids.len(), 4);
        assert!(token_ids.contains(&"token_yes_1".to_string()));
        assert!(token_ids.contains(&"token_no_1".to_string()));
        assert!(token_ids.contains(&"token_yes_2".to_string()));
        assert!(token_ids.contains(&"token_no_2".to_string()));
    }

    #[test]
    fn test_extract_token_ids_deduplicates() {
        let markets = vec![
            DiscoveredMarket {
                condition_id: "0xabc".to_string(),
                clob_token_ids: vec!["shared_token".to_string(), "token_no_1".to_string()],
                question: "Will X?".to_string(),
                active: true,
            },
            DiscoveredMarket {
                condition_id: "0xdef".to_string(),
                clob_token_ids: vec!["shared_token".to_string(), "token_no_2".to_string()],
                question: "Will Y?".to_string(),
                active: true,
            },
        ];

        let token_ids = MarketDiscovery::extract_token_ids(&markets);
        assert_eq!(token_ids.len(), 3); // shared_token appears once, not twice
        assert!(token_ids.contains(&"shared_token".to_string()));
        assert!(token_ids.contains(&"token_no_1".to_string()));
        assert!(token_ids.contains(&"token_no_2".to_string()));
    }

    #[test]
    fn test_market_discovery_builder() {
        let discovery = MarketDiscovery::new()
            .with_min_volume(10000.0)
            .with_min_liquidity(5000.0);

        assert_eq!(discovery.min_volume, Some(10000.0));
        assert_eq!(discovery.min_liquidity, Some(5000.0));
    }

    #[test]
    fn test_parse_gamma_response() {
        let json = r#"[
            {
                "conditionId": "0x1234",
                "clobTokenIds": ["token_yes", "token_no"],
                "question": "Will BTC hit 100k?",
                "active": true,
                "closed": false
            },
            {
                "conditionId": "0x5678",
                "clobTokenIds": [],
                "question": "Empty market",
                "active": true,
                "closed": false
            }
        ]"#;

        let markets: Vec<GammaMarketResponse> = serde_json::from_str(json).unwrap();
        assert_eq!(markets.len(), 2);
        assert_eq!(
            markets[0].condition_id,
            Some("0x1234".to_string())
        );
        assert_eq!(
            markets[0].clob_token_ids.as_ref().unwrap().len(),
            2
        );
        assert_eq!(
            markets[1].clob_token_ids.as_ref().unwrap().len(),
            0
        );
    }

    #[test]
    fn test_gamma_api_url() {
        assert!(GAMMA_API_URL.starts_with("https://"));
        assert!(GAMMA_API_URL.contains("gamma"));
        assert!(GAMMA_API_URL.contains("polymarket"));
    }
}
