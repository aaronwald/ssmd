use thiserror::Error;

#[derive(Error, Debug)]
pub enum Error {
    #[error("PostgreSQL error: {0}")]
    Postgres(#[from] tokio_postgres::Error),

    #[error("Database error: {0}")]
    Database(String),

    #[error("NATS error: {0}")]
    Nats(String),

    #[error("Redis error: {0}")]
    Redis(#[from] redis::RedisError),

    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error),

    #[error("Configuration error: {0}")]
    Config(String),
}
