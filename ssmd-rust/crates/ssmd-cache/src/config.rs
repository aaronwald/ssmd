use crate::{Error, Result};

#[derive(Debug, Clone)]
pub struct Config {
    pub database_url: String,
    pub nats_url: String,
    pub redis_url: String,
    pub stream_name: String,
    pub consumer_name: String,
}

impl Config {
    pub fn from_env() -> Result<Self> {
        let database_url = std::env::var("DATABASE_URL")
            .map_err(|_| Error::Config("DATABASE_URL not set".into()))?;

        let nats_url =
            std::env::var("NATS_URL").unwrap_or_else(|_| "nats://localhost:4222".into());

        let redis_url =
            std::env::var("REDIS_URL").unwrap_or_else(|_| "redis://localhost:6379".into());

        let stream_name =
            std::env::var("NATS_STREAM").unwrap_or_else(|_| "SECMASTER_CDC".into());

        let consumer_name =
            std::env::var("CONSUMER_NAME").unwrap_or_else(|_| "ssmd-cache".into());

        Ok(Self {
            database_url,
            nats_url,
            redis_url,
            stream_name,
            consumer_name,
        })
    }
}
