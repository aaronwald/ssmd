//! Configuration module for ssmd-cdc

use crate::{Error, Result};

#[derive(Debug, Clone)]
pub struct Config {
    pub database_url: String,
    pub nats_url: String,
    pub slot_name: String,
    pub publication_name: String,
    pub tables: Vec<String>,
}

impl Config {
    pub fn from_env() -> Result<Self> {
        let database_url = std::env::var("DATABASE_URL")
            .map_err(|_| Error::Config("DATABASE_URL not set".into()))?;

        let nats_url = std::env::var("NATS_URL")
            .unwrap_or_else(|_| "nats://localhost:4222".into());

        let slot_name = std::env::var("REPLICATION_SLOT")
            .unwrap_or_else(|_| "ssmd_cdc".into());

        let publication_name = std::env::var("PUBLICATION_NAME")
            .unwrap_or_else(|_| "ssmd_cdc_pub".into());

        let tables = std::env::var("CDC_TABLES")
            .map(|s| s.split(',').map(|t| t.trim().to_string()).collect())
            .unwrap_or_else(|_| vec![
                "events".into(),
                "markets".into(),
                "series_fees".into(),
            ]);

        Ok(Self {
            database_url,
            nats_url,
            slot_name,
            publication_name,
            tables,
        })
    }
}
