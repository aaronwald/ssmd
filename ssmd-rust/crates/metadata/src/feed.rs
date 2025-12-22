use chrono::NaiveDate;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::Path;

use crate::error::MetadataError;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum FeedType {
    Websocket,
    Rest,
    Multicast,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum FeedStatus {
    #[default]
    Active,
    Deprecated,
    Disabled,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum AuthMethod {
    ApiKey,
    Oauth,
    Mtls,
    None,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CaptureLocation {
    pub datacenter: String,
    pub provider: Option<String>,
    pub region: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Calendar {
    pub timezone: Option<String>,
    pub holiday_calendar: Option<String>,
    pub open_time: Option<String>,
    pub close_time: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FeedVersion {
    pub version: String,
    pub effective_from: String,
    pub effective_to: Option<String>,
    pub protocol: String,
    pub endpoint: String,
    pub auth_method: Option<AuthMethod>,
    pub rate_limit_per_second: Option<i32>,
    pub max_symbols_per_connection: Option<i32>,
    pub supports_orderbook: Option<bool>,
    pub supports_trades: Option<bool>,
    pub supports_historical: Option<bool>,
    pub parser_config: Option<HashMap<String, String>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Feed {
    pub name: String,
    pub display_name: Option<String>,
    #[serde(rename = "type")]
    pub feed_type: FeedType,
    #[serde(default)]
    pub status: Option<FeedStatus>,
    pub capture_locations: Option<Vec<CaptureLocation>>,
    pub versions: Vec<FeedVersion>,
    pub calendar: Option<Calendar>,
}

impl Feed {
    pub fn load(path: &Path) -> Result<Self, MetadataError> {
        let content = std::fs::read_to_string(path)?;
        let feed: Feed = serde_yaml::from_str(&content)?;
        Ok(feed)
    }

    /// Get the version effective for a given date
    pub fn get_version_for_date(&self, date: NaiveDate) -> Option<&FeedVersion> {
        let date_str = date.format("%Y-%m-%d").to_string();

        // Sort by effective_from descending
        let mut versions: Vec<_> = self.versions.iter().collect();
        versions.sort_by(|a, b| b.effective_from.cmp(&a.effective_from));

        for v in versions {
            if v.effective_from <= date_str {
                if let Some(ref to) = v.effective_to {
                    if to >= &date_str {
                        return Some(v);
                    }
                } else {
                    return Some(v);
                }
            }
        }
        None
    }

    /// Get the most recent version
    pub fn get_latest_version(&self) -> Option<&FeedVersion> {
        self.versions
            .iter()
            .max_by(|a, b| a.effective_from.cmp(&b.effective_from))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    #[test]
    fn test_load_feed() {
        let mut file = tempfile::NamedTempFile::new().unwrap();
        writeln!(
            file,
            r#"
name: kalshi
display_name: Kalshi Exchange
type: websocket
status: active
versions:
  - version: v1
    effective_from: "2025-12-22"
    protocol: wss
    endpoint: wss://api.kalshi.com/trade-api/ws/v2
    auth_method: api_key
"#
        )
        .unwrap();

        let feed = Feed::load(file.path()).unwrap();
        assert_eq!(feed.name, "kalshi");
        assert_eq!(feed.feed_type, FeedType::Websocket);
        assert!(feed.get_latest_version().is_some());
    }

    #[test]
    fn test_version_for_date() {
        let feed = Feed {
            name: "test".to_string(),
            display_name: None,
            feed_type: FeedType::Websocket,
            status: Some(FeedStatus::Active),
            capture_locations: None,
            versions: vec![
                FeedVersion {
                    version: "v1".to_string(),
                    effective_from: "2025-01-01".to_string(),
                    effective_to: Some("2025-06-30".to_string()),
                    protocol: "wss".to_string(),
                    endpoint: "wss://v1".to_string(),
                    auth_method: None,
                    rate_limit_per_second: None,
                    max_symbols_per_connection: None,
                    supports_orderbook: None,
                    supports_trades: None,
                    supports_historical: None,
                    parser_config: None,
                },
                FeedVersion {
                    version: "v2".to_string(),
                    effective_from: "2025-07-01".to_string(),
                    effective_to: None,
                    protocol: "wss".to_string(),
                    endpoint: "wss://v2".to_string(),
                    auth_method: None,
                    rate_limit_per_second: None,
                    max_symbols_per_connection: None,
                    supports_orderbook: None,
                    supports_trades: None,
                    supports_historical: None,
                    parser_config: None,
                },
            ],
            calendar: None,
        };

        let march = NaiveDate::from_ymd_opt(2025, 3, 15).unwrap();
        let aug = NaiveDate::from_ymd_opt(2025, 8, 15).unwrap();

        assert_eq!(feed.get_version_for_date(march).unwrap().version, "v1");
        assert_eq!(feed.get_version_for_date(aug).unwrap().version, "v2");
    }
}
